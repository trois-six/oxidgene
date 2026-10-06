//! Arkothèque (1 égal 2), the publishing software of many French
//! departmental archives.
//!
//! Each collection is searched by one engine of the portal's request
//! interface, `/_recherche-api/moteur`, filtered by the locality, the act
//! category and the year where the engine has those filters, some of whose
//! values are first read from the engine's own lists. The answer's result
//! rows are selected down to one register (`select`), whose viewer endpoint
//! lists its images; the target is the portal's record page opened on the
//! cited image, `<search_path>?detail=<record>#<viewer address>/<i>`, the
//! viewer address naming the image's file and `i` its zero-based position
//! in that file. A portal that refuses those requests to a script (access
//! `page`) is searched by loading its search page with the filters in its
//! address, whose scripts render the same rows, and is never sent a request.
//! Archive Portals §4.3 specifies the settings and requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
mod settings;
#[cfg(test)]
mod tests;

use super::iiif::image_info;
use super::markup::fold;
use super::select::{Candidate, Selection, period_ranges, select};
use super::view::{cited_views, view_target};
use super::{BoxFuture, Platform, PortalEndpoint, Query};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::CitationParts;
use crate::transport::{FetchError, PortalFetch};
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};
use page::Register;
use settings::{Keys, Settings};

/// The rows one search asks for: the largest page the engines accept (25,
/// 50 or 100).
const RESULT_SIZE: &str = "100";

/// The search interface every Arkothèque portal exposes.
const SEARCH_PATH: &str = "/_recherche-api/moteur";

/// The page a browser transport loads before its requests: the lightest
/// page of the portal's origin that passes its bot-mitigation check. The
/// search page would fetch the engine's whole unfiltered list on load, which
/// the portal answers before the adapter's own search, in the same session:
/// on the Sarthe portal it delayed the search by up to 30 seconds.
const START_PATH: &str = "/robots.txt";

/// The Arkothèque adapter.
pub struct Arkotheque;

impl Platform for Arkotheque {
    fn id(&self) -> &'static str {
        "arkotheque"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}{START_PATH}", settings.origin),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
            insecure_http: false,
        })
    }

    /// The search page with the filters a request can write without
    /// reading the engine's lists: a keyed filter is left out.
    fn reads_pages(&self) -> bool {
        true
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        Some(settings.results_page(citation, &Keys::default()))
    }

    fn resolve<'a>(
        &'a self,
        archive: &'a Archive,
        collection: &'a Collection,
        citation: &'a CitationParts,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<ArchiveTarget, ResolveError>> {
        Box::pin(resolve(archive, collection, citation, fetch))
    }
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("arkotheque: {detail}"))
}

/// The keyed filters' values, read from the engine's lists: `None` when the
/// cited locality is not among those the engine lists, such as an office
/// that never kept the series.
async fn keys(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<Option<Keys>, ResolveError> {
    if !settings.needs_keys(citation) {
        return Ok(Some(Keys::default()));
    }
    let engine = page::engine_answer(&fetch.get(&settings.engine_request()).await?)?;
    let listed = |filter: &settings::Filter| {
        engine
            .field(&filter.reference)
            .map(|_| engine.filter_values(&filter.reference))
            .ok_or_else(|| unexpected("the engine lacks a keyed filter"))
    };
    let mut keys = Keys::default();
    if let Some(filter) = settings.fields.locality.as_ref().filter(|f| f.keyed)
        && !citation.locality.is_empty()
    {
        let style = settings.locality_style;
        let wanted = fold(&style.cited(&citation.locality));
        match listed(filter)?
            .into_iter()
            .find(|value| fold(&style.cited(page::without_key(value))) == wanted)
        {
            Some(value) => keys.locality = Some(value.to_owned()),
            None => return Ok(None),
        }
    }
    if let Some(filter) = settings.fields.period.as_ref().filter(|f| f.keyed)
        && let Some(year) = citation.year
    {
        // A year the engine does not list is left to the rows' periods.
        keys.period = listed(filter)?
            .into_iter()
            .find(|value| period_ranges(page::without_key(value)) == [(year, year)])
            .map(str::to_owned);
    }
    Ok(Some(keys))
}

/// The answer of a search with `filters`: the engine's, or for a portal
/// read by its pages, the search page filtered the same way as its scripts
/// render it.
async fn search_answer(
    settings: &Settings,
    filters: &Query,
    fetch: &dyn PortalFetch,
) -> Result<String, FetchError> {
    if settings.reads_pages() {
        let path = settings.page_request(filters);
        fetch.page(&path, page::RENDERED_RESULTS).await
    } else {
        fetch.get(&settings.search_request(filters)).await
    }
}

/// The registers of a search answer, read for selection — each row's
/// locality as a citation writes it, and its acts as codes where the
/// search could not single out the cited act — and the count of all the
/// search matched.
fn rows(
    settings: &Settings,
    answer: &str,
    citation: &CitationParts,
) -> Result<(Vec<Candidate<Register>>, usize), ResolveError> {
    let reads_acts = settings.reads_acts(&citation.act);
    let (mut rows, total) = if settings.reads_pages() {
        page::rendered_rows(answer, &settings.cells)?
    } else {
        page::search_rows(answer, &settings.cells)?
    };
    let style = settings.locality_style;
    let wanted: Vec<String> = settings
        .localities(citation)
        .iter()
        .map(|locality| fold(&style.cited(locality)))
        .collect();
    for row in &mut rows {
        // A row naming several localities is the cited one's when one of
        // them is.
        if let Some(named) = row
            .payload
            .localities
            .iter()
            .find(|locality| wanted.contains(&fold(&style.cited(locality))))
        {
            row.locality = Some(named.clone());
        }
        if row.parish.is_none() {
            row.parish = row
                .locality
                .as_deref()
                .and_then(|locality| style.parish(locality));
        }
        row.locality = row.locality.take().map(|locality| style.cited(&locality));
        if reads_acts {
            row.act = row
                .act
                .take()
                .map(|text| page::act_code(&text).unwrap_or(text));
        }
    }
    Ok((rows, total))
}

/// The pages a search reads at most: a cited call number missing from the
/// first page of a populated locality is looked for on the next ones.
const MAX_PAGES: usize = 3;

/// The registers the search lists for the citation: its first page, and
/// while the cited call number is on none read so far, the next pages.
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    keys: &Keys,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Candidate<Register>>, ResolveError> {
    let mut found = Vec::new();
    for _ in 0..MAX_PAGES {
        let filters = settings.page_filters(citation, keys, RESULT_SIZE, found.len());
        let answer = search_answer(settings, &filters, fetch).await?;
        let (rows, total) = rows(settings, &answer, citation)?;
        let last = rows.is_empty() || found.len() + rows.len() >= total;
        found.extend(rows);
        let cited = citation.call_number.as_ref();
        let carried = cited.is_none_or(|cited| {
            found.iter().any(|row| {
                row.call_number
                    .as_deref()
                    .is_some_and(|written| cited.matches(written))
            })
        });
        if last || carried {
            break;
        }
    }
    Ok(found)
}

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let Some(keys) = keys(&settings, citation, fetch).await? else {
        return Ok(ArchiveTarget::Results {
            url: settings.results_page(citation, &Keys::default()),
            matches: Some(0),
        });
    };
    let results = |matches| ArchiveTarget::Results {
        url: settings.results_page(citation, &keys),
        matches: Some(matches),
    };

    let rows = search(&settings, citation, &keys, fetch).await?;
    let localities = settings.localities(citation);
    let localities: Vec<&str> = localities.iter().map(String::as_str).collect();
    let row = match select(&rows, citation, &localities) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    let record = &row.payload.record;
    // A register listed without images cannot be opened on a view.
    let Some(viewer) = row.payload.viewer.as_deref() else {
        return Ok(results(1));
    };
    // A portal read by its pages refuses the viewer's image list to a
    // script, which would cost the next pages too: the row's count stands
    // for it, a row without one leaving the viewer to bound the view, and
    // the views go without their ARKs. Its archives are `portal`.
    let (sources, image_count) = if settings.reads_pages() {
        (Vec::new(), row.images.map_or(usize::MAX, usize::from))
    } else {
        let sources = page::viewer_sources(&fetch.get(viewer).await?, &settings.origin)?;
        let count = sources.len();
        (sources, count)
    };

    // The viewer's address of image `index` (zero-based): the image's own
    // file and position where the image list names them, the row's viewer
    // address and the index otherwise.
    let anchor = |index: u16| {
        sources
            .get(usize::from(index))
            .and_then(|source| source.viewer_anchor(viewer))
            .unwrap_or_else(|| format!("{viewer}/{index}"))
    };

    let cited = cited_views(citation, image_count, row.period.as_deref());
    let mut views = Vec::with_capacity(cited.len());
    for view in cited {
        let source = sources.get(usize::from(view.view) - 1);
        let image = match (archive.display, source) {
            (Display::Iiif, Some(source)) => Some(image(&settings, source, fetch).await?),
            (Display::Iiif, None) => return Err(unexpected("no image list for an iiif archive")),
            (Display::Portal, _) => None,
        };
        views.push(ArchiveView {
            view: view.view,
            url: settings.view_url(record, &anchor(view.view - 1)),
            ark: source
                .and_then(|source| source.ark.as_ref())
                .map(|ark| format!("{}{ark}", settings.origin)),
            image,
        });
    }
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        image_count,
        settings.view_url(record, &anchor(0)),
        views,
    ))
}

/// The image of one view, for a `display: "iiif"` archive: its size from the
/// service's `info.json`, and addresses built on the portal's own image path
/// rather than on the service's `@id`, which names an internal host.
async fn image(
    settings: &Settings,
    source: &page::Source,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveImage, ResolveError> {
    let info = image_info(&fetch.get(&format!("{}/info.json", source.src)).await?)?;
    Ok(info.image(&format!("{}{}", settings.origin, source.src)))
}
