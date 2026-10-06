//! The Arkothèque part of the live checks (Archive Portals §9.1).
//!
//! The search page names its engine and content components
//! (`data-moteur`, `data-contenu`); everything else the settings reference
//! is declared by the engine's bare answer, the one the page requests on
//! load, `/_recherche-api/moteur?refUnique=<engine>&<engine>--contenuIds[]=…`:
//! its `filtres` (each filter's reference and indexed field), its `restits`
//! (the display modes), and per field an aggregation of the values the
//! filter offers, the most frequent first. List values carry their record
//! keys, `Name[[arko_fiche_…]]`; a text filter's are plain. A portal read by
//! its pages refuses that answer to a script: its rendered search page
//! stands for it.

use super::page::{self, EngineAnswer, without_key};
use super::settings::{Filter, Keys, Values};
use super::{Arkotheque, Settings, keys, rows, search_answer};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::LocalityStyle;
use crate::platform::markup::fold;
use crate::platform::select::Candidate;
use crate::transport::PortalFetch;

impl Settings {
    /// What the search page lacks of the settings' references.
    fn missing_from_page(&self, page: &str) -> Vec<String> {
        let mut missing = Vec::new();
        if !page.contains(&format!("data-moteur=\"{}\"", self.engine)) {
            missing.push("engine".to_owned());
        }
        for id in &self.content_ids {
            if !page.contains(&format!("data-contenu=\"{id}\"")) {
                missing.push(format!("content {id}"));
            }
        }
        missing
    }

    /// What the engine's answer lacks of the settings' references: filters,
    /// display mode, act values. A filter whose values the engine does not
    /// list, such as a free text one, has its values checked by the search.
    fn missing_from_engine(&self, engine: &EngineAnswer) -> Vec<String> {
        let mut missing = Vec::new();
        let filters = [
            ("locality", self.fields.locality.as_ref()),
            ("act", self.fields.act.as_ref()),
            ("period", self.fields.period.as_ref()),
        ];
        for (role, filter) in filters {
            let Some(filter) = filter else { continue };
            let references = std::iter::once(&filter.reference).chain(&filter.end);
            if references
                .into_iter()
                .any(|reference| engine.field(reference).is_none())
            {
                missing.push(format!("{role} filter"));
            }
        }
        if !engine
            .restits
            .iter()
            .any(|restit| restit.reference == self.display_mode)
        {
            missing.push("display mode".to_owned());
        }
        let listed = self
            .fields
            .act
            .as_ref()
            .map(|act| engine.filter_values(&act.reference))
            .unwrap_or_default();
        if !listed.is_empty() {
            for (code, Values(values)) in &self.acts {
                if !values.iter().all(|value| listed.contains(&value.as_str())) {
                    missing.push(format!("act value of {code}"));
                }
            }
        }
        missing
    }
}

/// The rows a discovery asks for: enough to choose from, few enough for
/// the slower engines to answer within the bound of a request.
const DISCOVERY_SIZE: &str = "25";

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Arkothèque settings", error.to_string()))
}

/// The fewest records of a locality the discovery searches: one listed for
/// a handful of records only, such as a pseudo-locality grouping a few
/// registers, may hold none of the collection's first document kind.
const ORDINARY_RECORDS: u64 = 10;

/// The alphabetically first locality the locality filter lists for at least
/// [`ORDINARY_RECORDS`] records — any listed one when none is —, as a
/// citation writes it, a hamlet that keeps its qualifier left out; empty
/// for an engine without the filter, whose registers are then searched
/// everywhere.
fn first_locality(
    settings: &Settings,
    filter: Option<&Filter>,
    engine: &EngineAnswer,
) -> Option<String> {
    let Some(filter) = filter else {
        return Some(String::new());
    };
    let field = engine.field(&filter.reference)?;
    let style = settings.locality_style;
    let candidates: Vec<(String, u64)> = engine
        .counted_values(field)
        .into_iter()
        .map(|(value, count)| (style.cited(without_key(value)), count))
        .filter(|(locality, _)| {
            !locality.is_empty() && !(style == LocalityStyle::Qualified && locality.ends_with(')'))
        })
        .collect();
    let ordinary = candidates
        .iter()
        .any(|(_, count)| *count >= ORDINARY_RECORDS);
    candidates
        .into_iter()
        .filter(|(_, count)| !ordinary || *count >= ORDINARY_RECORDS)
        .map(|(locality, _)| locality)
        .min_by_key(|locality| fold(locality))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    if settings.reads_pages() {
        return rendered_search_page(&settings, fetch).await;
    }
    let page = fetch
        .get(&settings.search_path)
        .await
        .map_err(|error| Failure::fetch(step, "the collection's search page", error))?;
    let missing = settings.missing_from_page(&page);
    if !missing.is_empty() {
        return Err(Failure::unreadable(
            step,
            "the engine and content references in the search page",
            &page,
            format!("missing: {}", missing.join(", ")),
        ));
    }

    let expected = "the engine's filters, display modes and aggregations";
    let answer = fetch
        .get(&settings.engine_request())
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let engine = page::engine_answer(&answer)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let missing = settings.missing_from_engine(&engine);
    if !missing.is_empty() {
        return Err(Failure::drift(
            step,
            "the settings' references in the engine",
            format!("missing: {}", missing.join(", ")),
        ));
    }
    // The engine lists the most populated locality first, whose search is
    // the slowest; the alphabetical first is an ordinary one.
    first_locality(&settings, settings.fields.locality.as_ref(), &engine).ok_or_else(|| {
        Failure::drift(
            step,
            "the localities of the locality filter",
            "no locality listed",
        )
    })
}

impl Settings {
    /// What a rendered search page lacks of the settings' filters, each
    /// drawn with its reference (`aria-filtre-<filter>`). The display mode
    /// is not drawn: the rows its cells are read from check it.
    fn filters_missing_from_page(&self, page: &str) -> Vec<String> {
        let filters = [
            ("locality", self.fields.locality.as_ref()),
            ("act", self.fields.act.as_ref()),
            ("period", self.fields.period.as_ref()),
        ];
        filters
            .into_iter()
            .filter_map(|(role, filter)| Some((role, filter?)))
            .filter(|(_, filter)| {
                std::iter::once(&filter.reference)
                    .chain(&filter.end)
                    .any(|reference| !page.contains(&format!("aria-filtre-{reference}")))
            })
            .map(|(role, _)| format!("{role} filter"))
            .collect()
    }
}

/// Step 1 on a portal read by its pages, which refuses the engine's answer
/// to a script: the unfiltered search page as its scripts render it names
/// the engine, the content components and the filters, and its rows the
/// localities, of which the alphabetically first is searched (empty for a
/// collection whose rows show none). The act values are checked by the
/// search of the first one.
async fn rendered_search_page(
    settings: &Settings,
    fetch: &dyn PortalFetch,
) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let expected = "the search page rendered with its rows";
    let path = settings.page_request(&settings.bare_filters(DISCOVERY_SIZE));
    let page = fetch
        .page(&path, page::RENDERED_RESULTS)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let mut missing = settings.missing_from_page(&page);
    missing.extend(settings.filters_missing_from_page(&page));
    if !missing.is_empty() {
        return Err(Failure::unreadable(
            step,
            "the engine, content and filter references in the search page",
            &page,
            format!("missing: {}", missing.join(", ")),
        ));
    }
    let (rows, _) = page::rendered_rows(&page, &settings.cells)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if settings.cells.locality.is_none() {
        return Ok(String::new());
    }
    let style = settings.locality_style;
    rows.iter()
        .filter_map(|row| row.locality.as_deref())
        .map(|locality| style.cited(locality))
        .filter(|locality| !locality.is_empty())
        .min_by_key(|locality| fold(locality))
        .ok_or_else(|| Failure::drift(step, "the localities of the rendered rows", "none"))
}

/// The result rows the search a citation of `locality` and `act` would
/// send lists, without a year.
async fn discover(
    settings: &Settings,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Candidate<page::Register>>, Failure> {
    let step = Step::Discovery;
    let search = CitationParts {
        code: String::new(),
        locality: locality.to_owned(),
        parish: None,
        act: act.clone(),
        year: None,
        period: None,
        call_number: None,
        number: None,
        views: Vec::new(),
        view_count: None,
    };
    let expected = "the result rows of the first listed locality";
    let keys = keys(settings, &search, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .unwrap_or_else(Keys::default);
    let filters = settings.page_filters(&search, &keys, DISCOVERY_SIZE, 0);
    let answer = search_answer(settings, &filters, fetch)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    rows(settings, &answer, &search)
        .map(|(rows, _)| rows)
        .map_err(|error| Failure::from_error(step, expected, &error))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let mut searched = locality;
    let mut rows = discover(&settings, searched, act, fetch).await?;
    // A locality listed for other document kinds only: the kind anywhere.
    if rows.is_empty() && !locality.is_empty() {
        searched = "";
        rows = discover(&settings, searched, act, fetch).await?;
    }
    if rows.is_empty() {
        return Err(Failure::drift(
            step,
            "the result rows of the first listed locality",
            "no register",
        ));
    }
    // The viewer shows no view number for a single image, which step 4
    // reads: a register of several is checked where the search lists one.
    let several = rows
        .iter()
        .any(|row| row.images.is_some_and(|images| images > 1));
    let reads_pages = settings.reads_pages();
    Ok(rows
        .into_iter()
        .filter(|row| !several || row.images != Some(1))
        .map(|row| Register {
            // A row without a locality cell is the searched locality's.
            locality: row.locality.unwrap_or_else(|| searched.to_owned()),
            call_number: row.call_number,
            period: row.period,
            // Counted by the viewer, whose images a citation cites: some
            // rows count images only the reading room shows. A portal read
            // by its pages refuses the viewer's list to a script, and its
            // resolution counts by the row too.
            images: if reads_pages { row.images } else { None },
            address: row.payload.viewer,
            numbers: row.numbers,
        })
        .collect())
}

/// The image count of a register whose row does not show it: the length of
/// its viewer's image list.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let expected = "the chosen register's viewer";
    let Some(viewer) = register
        .address
        .as_deref()
        .filter(|_| !settings.reads_pages())
    else {
        return Ok(None);
    };
    let answer = fetch
        .get(viewer)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let sources = page::viewer_sources(&answer, &settings.origin)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    Ok(u16::try_from(sources.len()).ok())
}

impl Probe for Arkotheque {
    fn search_page<'a>(
        &'a self,
        collection: &'a Collection,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<String, Failure>> {
        Box::pin(search_page(collection, fetch))
    }

    fn registers<'a>(
        &'a self,
        collection: &'a Collection,
        locality: &'a str,
        act: &'a Act,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Vec<Register>, Failure>> {
        Box::pin(registers(collection, locality, act, fetch))
    }

    fn images<'a>(
        &'a self,
        collection: &'a Collection,
        register: &'a Register,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Option<u16>, Failure>> {
        Box::pin(images(collection, register, fetch))
    }
}
