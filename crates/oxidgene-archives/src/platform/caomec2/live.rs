//! The CAOMEC2 part of the live checks (Archive Portals §9.1): a
//! territory's search form lists its communes and has the fields a search
//! sends; a commune's search lists registers by year, which the viewer of
//! the chosen one counts. The viewer has no address per view: step 4 checks
//! its first view and its count.

use super::page;
use super::{Caomec2, Settings, get, search};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::markup::fold;
use crate::platform::select::holds_act;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid caomec2 settings", error.to_string()))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let expected = "the territory's search form with its communes";
    let form = get(fetch, &settings.form_path())
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if !page::has_search_fields(&form) {
        return Err(Failure::unreadable(
            step,
            expected,
            &form,
            "no commune, act type or year field",
        ));
    }
    let communes =
        page::communes(&form).map_err(|error| Failure::from_error(step, expected, &error))?;
    // A hospital or a penitentiary is listed beside its commune.
    communes
        .into_iter()
        .filter(|commune| !commune.contains('('))
        .min_by_key(|commune| fold(commune))
        .ok_or_else(|| Failure::drift(step, "a commune in the form's list", "none"))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let expected = "the registers of the first listed commune";
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
    let found = search(&settings, &citation, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .ok_or_else(|| Failure::drift(step, expected, "the commune is not listed"))?;
    let registers: Vec<Register> = found
        .candidates
        .into_iter()
        .filter(|candidate| holds_act(candidate.act.as_deref(), act))
        .map(|candidate| Register {
            locality: found.commune.clone(),
            call_number: None,
            period: candidate.period,
            images: None,
            address: Some(candidate.payload),
            numbers: None,
        })
        .collect();
    if registers.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(registers)
}

async fn images(register: &Register, fetch: &dyn PortalFetch) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let expected = "the chosen register's viewer";
    let Some(viewer) = &register.address else {
        return Ok(None);
    };
    let page = get(fetch, viewer)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let count =
        page::image_count(&page).map_err(|error| Failure::from_error(step, expected, &error))?;
    Ok(u16::try_from(count).ok())
}

impl Probe for Caomec2 {
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
        _collection: &'a Collection,
        register: &'a Register,
        fetch: &'a dyn PortalFetch,
    ) -> BoxFuture<'a, Result<Option<u16>, Failure>> {
        Box::pin(images(register, fetch))
    }

    /// The viewer opens a register on its first view only.
    fn addresses_views(&self, _collection: &Collection) -> bool {
        false
    }
}
