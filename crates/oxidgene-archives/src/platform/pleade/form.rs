//! The `form` mode: a search form whose results list the registers.
//!
//! The form's locality list is a thesaurus, which the search matches
//! exactly (`Exampleville (Exemple, France)`, a former commune with its note
//! `(Exemple, France ; jusqu'à 1919) [aujourd'hui : …]`); the results
//! fragment, the one the form's page itself requests, lists each register
//! with its locality, period, kind and acts, call number and viewer link.

use super::page::{self, Form, Row};
use super::{Found, Register, Search, Settings, get, unexpected};
use crate::ResolveError;
use crate::citation::{Act, CitationParts};
use crate::platform::Query;
use crate::platform::locality::{label_name, matching_labels};
use crate::platform::select::{Candidate, narrow};
use crate::transport::PortalFetch;

/// The most labels of the locality list searched for one cited locality: a
/// commune and its former namesakes.
const MAX_LABELS: usize = 3;

/// The most result pages read, of 20 rows each.
const MAX_PAGES: usize = 5;

/// The form page, and its form.
pub(super) async fn form_page(
    settings: &Settings,
    fetch: &dyn PortalFetch,
) -> Result<String, ResolveError> {
    let Search::Form { page, .. } = &settings.search else {
        return Err(unexpected("settings of another mode"));
    };
    get(fetch, &format!("{}/{page}", settings.path)).await
}

/// The kind-of-document value searched for a cited kind: the registers' or
/// the tables'; none for anything else.
fn kind_value<'s>(settings: &'s Settings, act: &Act) -> &'s str {
    let Search::Form { kinds, .. } = &settings.search else {
        return "";
    };
    match act {
        Act::Register(_) => &kinds.registers,
        Act::Table(_) => &kinds.tables,
        Act::Series(_) => "",
    }
}

/// The query of a search: the form's hidden inputs, the label, the kind of
/// document and the year, which the portal tests against each register's
/// period; then the page.
pub(super) fn query(
    settings: &Settings,
    form: &Form<'_>,
    label: &str,
    act: &Act,
    year: Option<u16>,
    page: usize,
) -> String {
    let Search::Form { criteria, .. } = &settings.search else {
        return String::new();
    };
    let mut query = Query::new();
    for (name, value) in form.hidden() {
        query.push(name, value);
    }
    query
        .push(format!("query{}", criteria.locality), label)
        .push(format!("query{}", criteria.kind), kind_value(settings, act))
        .push(
            format!("du{}", criteria.year),
            year.map(|year| year.to_string()).unwrap_or_default(),
        )
        .push(format!("db{}", criteria.year), "")
        .push(format!("de{}", criteria.year), "");
    if page > 1 {
        query.push("p", page.to_string());
    }
    query.to_string()
}

/// Whether the form has the settings' inputs and kinds of documents.
pub(super) fn check(settings: &Settings, form: &Form<'_>) -> Result<(), ResolveError> {
    let Search::Form {
        criteria, kinds, ..
    } = &settings.search
    else {
        return Err(unexpected("settings of another mode"));
    };
    if form
        .options(&format!("query{}", criteria.locality))
        .is_empty()
    {
        return Err(unexpected("the form has no locality list"));
    }
    let offered = form.options(&format!("query{}", criteria.kind));
    if [&kinds.registers, &kinds.tables]
        .iter()
        .any(|kind| !offered.contains(kind))
    {
        return Err(unexpected("the form does not offer the kinds of documents"));
    }
    if !form.has(&format!("du{}", criteria.year)) {
        return Err(unexpected("the form has no year input"));
    }
    Ok(())
}

/// A result row as selection reads it.
fn candidate(row: &Row) -> Candidate<Register> {
    Candidate {
        locality: row.locality.as_deref().map(label_name),
        call_number: row.call_number.clone(),
        act: page::act_of(&row.acts.join(", ")),
        parish: None,
        period: row.period.clone(),
        images: None,
        numbers: None,
        payload: Register::Ark(row.ark.clone()),
    }
}

pub(super) async fn find(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Form { results, .. } = &settings.search else {
        return Err(unexpected("settings of another mode"));
    };
    let html = form_page(settings, fetch).await?;
    let form = Form::of(&html).ok_or_else(|| {
        crate::platform::markup::unreadable(&html, "pleade: the search page has no form".to_owned())
    })?;
    check(settings, &form)?;
    let action = form
        .action()
        .ok_or_else(|| unexpected("the form has no results page"))?;
    let Search::Form { criteria, .. } = &settings.search else {
        return Err(unexpected("settings of another mode"));
    };
    let labels = form.options(&format!("query{}", criteria.locality));
    let named = matching_labels(&labels, localities);
    let mut found = Found {
        candidates: Vec::new(),
        results_url: None,
        total: named.len(),
    };
    if named.is_empty() || named.len() > MAX_LABELS {
        return Ok(found);
    }
    found.total = 0;
    let prefix = settings.prefix();
    for label in named {
        let mut read = 0;
        loop {
            read += 1;
            let search = query(settings, &form, label, &citation.act, citation.year, read);
            let answer = get(fetch, &format!("{}/{results}?{search}", settings.path)).await?;
            let page = page::results(&answer, &prefix)?;
            if read == 1 {
                found.total += page.total;
                // The page a reader opens on the same search.
                found.results_url = Some(format!("{action}?{search}"));
            }
            // A register a commune and its former namesake both list is one.
            for row in &page.rows {
                let register = candidate(row);
                if !found
                    .candidates
                    .iter()
                    .any(|known| known.payload == register.payload)
                {
                    found.candidates.push(register);
                }
            }
            let decided = narrow(&found.candidates, citation, localities).len() == 1;
            if decided || read >= page.pages.min(MAX_PAGES) {
                break;
            }
        }
    }
    Ok(found)
}
