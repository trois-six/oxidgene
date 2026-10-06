//! The older Mnesys part of the live checks (Archive Portals §9.1): the
//! guided search's form still has the settings' fields and values; its
//! place index is behind a script robots.txt disallows, so the locality
//! is read from a search of the collection's first kind at its last year
//! without one, the first hit naming a place. A search lists registers
//! without image counts: the viewer host alone knows them, and the
//! register is cited at its second view, uncounted.

use super::{MnesysInao, Settings, TITLE_MATCH, get, page, place_names, search};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::forms;
use crate::platform::markup::decode_entities;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid mnesys-inao settings", error.to_string()))
}

fn citation(locality: &str, act: &Act) -> CitationParts {
    CitationParts {
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
    }
}

/// Whether the form has the settings' fields, kinds and companion fields.
fn check_form(settings: &Settings, form: &str) -> Result<(), String> {
    let mut fields: Vec<&str> = settings.locality.iter().map(String::as_str).collect();
    fields.extend(settings.act.as_deref());
    fields.extend(settings.year.as_ref().map(|year| year.field.as_str()));
    for field in &fields {
        if !form.contains(&format!("name=\"form_search_{field}\"")) {
            return Err(format!("no field `{field}`"));
        }
    }
    let titled = settings
        .act
        .iter()
        .chain(settings.year.iter().map(|year| &year.field));
    for field in titled {
        let companion = format!("name=\"form_req_{field}\" value=\"{TITLE_MATCH}\"");
        if !form.contains(&companion) {
            return Err(format!("no title match for `{field}`"));
        }
    }
    let decoded = decode_entities(form);
    if let Some(value) = settings
        .acts
        .values()
        .find(|value| !decoded.contains(&format!("value=\"{value}\"")))
    {
        return Err(format!("no kind `{value}`"));
    }
    if settings.year.is_none() && !form.contains("name=\"form_search_unitdate3\"") {
        return Err("no date field".to_owned());
    }
    Ok(())
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let expected = "the guided search's form with the settings' fields";
    let fail = |error| Failure::from_error(step, expected, &error);
    let form = get(fetch, &format!("/?id={}", settings.form))
        .await
        .map_err(fail)?;
    check_form(&settings, &form)
        .map_err(|received| Failure::unreadable(step, expected, &form, received))?;
    if settings.locality.is_none() {
        return Ok(String::new());
    }
    let act = collection
        .acts
        .first()
        .ok_or_else(|| Failure::drift(step, "an act in the collection", "no act"))?;
    // At the collection's last year: its earliest documents, which a search
    // without a year lists first, may name no place (a census search
    // starts with the Chamber of Accounts' undated surveys).
    let latest = CitationParts {
        year: collection.period.as_ref().and_then(|period| period.last),
        ..citation("", act)
    };
    let answer = get(fetch, &settings.search_path(&latest))
        .await
        .map_err(fail)?;
    let answer = page::answer(&answer).map_err(fail)?;
    // A place the breadcrumb names, not a range of call numbers.
    answer
        .nodes
        .iter()
        .filter(|node| node.call_number.is_some())
        .find_map(|node| {
            place_names(node)
                .into_iter()
                .find(|name| !name.contains(|c: char| c.is_ascii_digit()) && !name.contains(','))
        })
        .ok_or_else(|| Failure::drift(step, "a register naming its place", "none"))
}

async fn registers(
    collection: &Collection,
    locality: &str,
    act: &Act,
    fetch: &dyn PortalFetch,
) -> Result<Vec<Register>, Failure> {
    let step = Step::Discovery;
    let settings = settings(collection, step)?;
    let expected = "the registers of the locality";
    let wanted = match settings.locality {
        Some(_) => forms(locality),
        None => vec![String::new()],
    };
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = search(&settings, &citation(locality, act), &localities, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    if found.candidates.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    Ok(found
        .candidates
        .into_iter()
        .map(|node| Register {
            locality: node.locality.unwrap_or_else(|| locality.to_owned()),
            call_number: node.call_number,
            period: node.period,
            images: None,
            address: Some(node.payload),
            numbers: node.numbers,
        })
        .collect())
}

impl Probe for MnesysInao {
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

    /// The viewer host alone knows a register's size.
    fn counts_images(&self, _collection: &Collection) -> bool {
        false
    }
}
