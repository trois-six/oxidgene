//! The Gers part of the live checks (Archive Portals §9.1): a module's form
//! lists its localities and has the settings' checkboxes; a search lists
//! registers with their image counts and viewer links.

use super::page;
use super::{Archives32, Settings, search, unchallenged};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::forms;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid archives32 settings", error.to_string()))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let expected = "the module's form with the settings' fields";
    let page = fetch
        .get(&settings.search_path())
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let page = unchallenged(page).map_err(|error| Failure::from_error(step, expected, &error))?;
    let fields = std::iter::once(&settings.fields.locality)
        .chain(&settings.fields.former)
        .map(|name| format!("name=\"{name}\""))
        .chain(
            settings
                .acts
                .values()
                .flatten()
                .map(|name| format!("name=\"{name}\"")),
        );
    for field in fields {
        if !page.contains(&field) {
            return Err(Failure::unreadable(
                step,
                expected,
                &page,
                format!("no {field}"),
            ));
        }
    }
    page::options(&page, &settings.fields.locality)
        .into_iter()
        .map(|label| label.trim().to_owned())
        .min_by_key(|label| fold(label))
        .ok_or_else(|| Failure::drift(step, "a locality in the form's list", "none"))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
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
    };
    let wanted = forms(locality);
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = search(&settings, &citation, &localities, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .ok_or_else(|| Failure::drift(step, expected, "the locality is not listed"))?;
    if found.rows.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(found
        .rows
        .into_iter()
        .map(|row| Register {
            locality: row
                .locality
                .map_or_else(|| locality.to_owned(), |name| name.trim().to_owned()),
            call_number: row.call_number,
            period: row.period,
            images: row.images,
            address: row.viewer,
            numbers: None,
        })
        .collect())
}

impl Probe for Archives32 {
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
}
