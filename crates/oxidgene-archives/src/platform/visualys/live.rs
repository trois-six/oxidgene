//! The Visualys part of the live checks (Archive Portals §9.1): a site's
//! search page, reached as a visitor whose licence is pending, lists its
//! localities (the first initial's) or offers the settings' kind of
//! register and its offices; a search lists lots. The target, a view the
//! lot's sheets of thumbnails number, stands behind the site's licence:
//! step 4 opens the entry, accepts the licence as a reader does, then the
//! target, and reads the view the viewer shows.

use super::page::{self, Locality};
use super::{Mode, Settings, Visualys, find_by_criteria, find_in_localities, get};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::forms;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Visualys settings", error.to_string()))
}

/// The first locality of the list listed without a parish, as a citation
/// writes it.
fn first(settings: &Settings, listed: &[Locality]) -> Option<String> {
    listed
        .iter()
        .filter(|row| row.parish.is_none())
        .map(|row| settings.locality_style.cited(&row.name))
        .min_by_key(|name| fold(name))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let expected = "the site's search page behind its entry";
    let fail = |error| Failure::from_error(step, expected, &error);
    get(fetch, &settings.entry_path()).await.map_err(fail)?;
    match settings.mode {
        Mode::Localities => {
            let list = get(fetch, &settings.search_path('A')).await.map_err(fail)?;
            let listed = page::localities(&list).map_err(fail)?;
            first(&settings, &listed)
                .ok_or_else(|| Failure::drift(step, "a locality under the initial A", "none"))
        }
        Mode::Search => {
            let search = get(fetch, &settings.search_path('*')).await.map_err(fail)?;
            page::form(&search, &settings.base).map_err(fail)?;
            let kinds = page::options(&search, "lstRegistre");
            if let Some(kind) = settings.acts.values().find(|kind| !kinds.contains(kind)) {
                return Err(Failure::drift(
                    step,
                    format!("the kind of register `{kind}`"),
                    "not offered",
                ));
            }
            page::options(&search, "lstBureau")
                .into_iter()
                .min_by_key(|office| fold(office))
                .ok_or_else(|| Failure::drift(step, "an office in the form", "none"))
        }
    }
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let expected = "the lots of the first listed locality";
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
    let wanted = forms(locality);
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let fail = |error| Failure::from_error(step, expected, &error);
    let found = match settings.mode {
        Mode::Localities => find_in_localities(&settings, &citation, &localities, fetch)
            .await
            .map_err(fail)?,
        Mode::Search => find_by_criteria(&settings, &citation, &localities, fetch)
            .await
            .map_err(fail)?
            .ok_or_else(|| Failure::drift(step, expected, "the office is not offered"))?,
    };
    if found.candidates.is_empty() {
        return Err(Failure::drift(step, expected, "no lot"));
    }
    Ok(found
        .candidates
        .into_iter()
        .map(|lot| Register {
            locality: lot.locality.unwrap_or_else(|| locality.to_owned()),
            call_number: lot.call_number,
            period: lot.period,
            images: lot.images,
            address: Some(lot.payload.id),
            numbers: None,
        })
        .collect())
}

impl Probe for Visualys {
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

    /// A search by criteria lists its volumes without their image counts,
    /// which only the volume's sheets of thumbnails show.
    fn counts_images(&self, collection: &Collection) -> bool {
        Settings::read(collection).is_ok_and(|settings| settings.mode == Mode::Localities)
    }
}
