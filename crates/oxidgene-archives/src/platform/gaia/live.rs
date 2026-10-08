//! The GAIA part of the live checks (Archive Portals §9.1).
//!
//! The search's first list names the localities (or, for a search without
//! localities, the kinds of registers the settings choose); the wizard run
//! without a year lists a locality's registers; a register's viewer page
//! embeds one object per view, which counts its images. The viewer has no
//! address per view: the check opens it on its first view.

use super::{Found, Gaia, MAX_OTHER_ENTRIES, Purpose, Settings, Start, page, search_from, start};
use crate::ResolveError;
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::LocalityStyle;
use crate::platform::markup::{fold, is_named, letters};
use crate::platform::select::{holds_act, period_ranges};
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid GAIA settings", error.to_string()))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let expected = "the choices of the search's first list";
    let settings = settings(collection, step)?;
    let list = fetch
        .get(&settings.list(None))
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let links = page::links(&list, &settings)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if links.is_empty() {
        return Err(Failure::drift(step, expected, "no choice"));
    }
    if !settings.localities {
        // The list offers the kinds of registers the settings choose first.
        let missing: Vec<String> = collection
            .acts
            .iter()
            .filter_map(|act| {
                let label = settings.path(act)?.first()?;
                let wanted = [letters(label)];
                (!links.iter().any(|link| is_named(&link.label, &wanted))).then(|| act.to_string())
            })
            .collect();
        if !missing.is_empty() {
            return Err(Failure::drift(
                step,
                "the settings' first labels among the list's choices",
                format!("missing for: {}", missing.join(", ")),
            ));
        }
        return Ok(String::new());
    }
    localities(&settings, &links)
        .into_iter()
        .next()
        .ok_or_else(|| Failure::drift(step, expected, "no locality written in full"))
}

/// The localities a list names, as a citation writes them, alphabetically.
/// A label the decoding left a U+FFFD in cannot be cited as written.
fn localities(settings: &Settings, links: &[page::Link]) -> Vec<String> {
    let mut names: Vec<String> = links
        .iter()
        .map(|link| page::entry(&link.label, settings.prefix.as_deref()).name)
        .filter(|name| !name.is_empty() && !name.contains('\u{fffd}'))
        .map(|name| LocalityStyle::ArticleSuffix.cited(name))
        .collect();
    names.sort_by_key(|name| fold(name));
    names.dedup();
    names
}

/// The further localities of the list the discovery searches when the first
/// holds no register of the kind (a registration office without succession
/// tables, a commune created after the parish registers).
const NEXT_LOCALITIES: usize = 2;

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let expected = "the registers of the first listed locality";
    let settings = settings(collection, step)?;
    let mut registers = registers_at(&settings, collection, locality, act, fetch).await?;
    if registers.is_empty() && settings.localities {
        let list = fetch
            .get(
                &settings.list(
                    settings
                        .letters
                        .then(|| settings.letter(locality))
                        .flatten(),
                ),
            )
            .await
            .map_err(|error| Failure::fetch(step, expected, error))?;
        let links = page::links(&list, &settings)
            .map_err(|error| Failure::from_error(step, expected, &error))?;
        let next: Vec<String> = localities(&settings, &links)
            .into_iter()
            .filter(|name| fold(name) > fold(locality))
            .take(NEXT_LOCALITIES)
            .collect();
        for name in next {
            registers = registers_at(&settings, collection, &name, act, fetch).await?;
            if !registers.is_empty() {
                break;
            }
        }
    }
    if registers.is_empty() {
        return Err(Failure::drift(
            step,
            expected,
            "no register of the kind with images",
        ));
    }
    Ok(registers)
}

/// The registers with images of a locality and a kind: from the
/// locality's own entry, or from its other entries when the own one holds
/// none of the collection's period, as a resolution searches them.
async fn registers_at(
    settings: &Settings,
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let expected = "the registers of the first listed locality";
    let citation = CitationParts {
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
        alternate_localities: Vec::new(),
    };
    let failed = |error: ResolveError| Failure::from_error(step, expected, &error);
    let (answer, others) = match start(settings, &citation, fetch).await.map_err(failed)? {
        Start::Page { answer, others } => (answer, others),
        Start::Choices(count) => {
            return Err(Failure::drift(
                step,
                expected,
                format!("{count} list entries for the locality"),
            ));
        }
    };
    let mut registers = discover(settings, &citation, fetch, answer)
        .await
        .map_err(failed)?;
    // A commune's own entry may hold its civil status alone, its parish
    // registers being listed under its parishes, which a resolution then
    // searches.
    let in_period = |register: &Register| {
        register
            .period
            .as_deref()
            .and_then(|period| period_ranges(period).first().copied())
            .is_some_and(|(first, _)| collection.covers(Some(first)))
    };
    if others.len() <= MAX_OTHER_ENTRIES {
        for other in &others {
            if registers.iter().any(in_period) {
                break;
            }
            let answer = fetch
                .get(&other.path)
                .await
                .map_err(|error| Failure::fetch(step, expected, error))?;
            registers.extend(
                discover(settings, &citation, fetch, answer)
                    .await
                    .map_err(failed)?,
            );
        }
    }
    Ok(registers)
}

/// The registers with images of the kind a search from `answer` lists, as
/// their titles read.
async fn discover(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
    answer: String,
) -> Result<Vec<Register>, ResolveError> {
    let rows = match search_from(settings, citation, fetch, Purpose::Discover, answer).await? {
        Found::Rows { rows, year, .. } => super::dated(rows, year),
        Found::Choices(_) => return Ok(Vec::new()),
    };
    Ok(rows
        .into_iter()
        .filter(|row| {
            matches!(citation.act, Act::Series(_))
                || holds_act(page::title_act(&row.title).as_deref(), &citation.act)
        })
        .filter_map(|row| {
            let viewer = row.viewer?;
            Some(Register {
                locality: citation.locality.clone(),
                numbers: page::title_numbers(&row.title),
                period: row.period.or(Some(row.title)),
                call_number: row.call_number,
                images: None,
                address: Some(settings.viewer_url(&viewer)),
            })
        })
        .collect())
}

/// The views of a register: the objects of its viewer page's `docs`.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let expected = "the views of the chosen register's viewer";
    let settings = settings(collection, step)?;
    let Some(path) = register
        .address
        .as_deref()
        .and_then(|address| address.strip_prefix(&settings.origin))
    else {
        return Ok(None);
    };
    let viewer = fetch
        .get(path)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    match page::views(&viewer) {
        Some(count) => Ok(u16::try_from(count).ok()),
        None => Err(Failure::unreadable(
            step,
            expected,
            &viewer,
            "no docs array",
        )),
    }
}

impl Probe for Gaia {
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

    fn addresses_views(&self, _collection: &Collection) -> bool {
        false
    }
}
