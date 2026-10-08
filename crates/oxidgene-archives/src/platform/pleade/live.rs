//! The Pleade part of the live checks (Archive Portals §9.1):
//!
//! - `form`: the form page has the settings' inputs and kinds, and its
//!   locality list names the localities;
//! - `tree`: the finding aid's table of contents, walked down its first
//!   branch holding the collection's first kind to the localities' depth,
//!   names them.
//!
//! Neither counts a register's images: the chosen register's manifest
//! does.

use super::page::{self, Form, Illustrated, Node};
use super::{Pleade, Search, Settings, detailed, form, tree};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::{forms, label_name};
use crate::platform::markup::fold;
use crate::platform::select::holds_act;
use crate::transport::PortalFetch;

/// The most registers of a `tree` search whose components are read for the
/// dates their nodes lack.
const MAX_COMPLETED: usize = 4;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Pleade settings", error.to_string()))
}

/// The alphabetically first of the names, a placeholder left out.
fn first(names: impl Iterator<Item = String>) -> Option<String> {
    names
        .filter(|name| !name.is_empty() && !name.contains(['(', ')', '[', ';']))
        .min_by_key(|name| fold(name))
}

/// The localities a form lists, as citations write them: its current
/// communes, a former one's label (`… ; jusqu'à 1919) [aujourd'hui : …]`)
/// and an unqualified parish left out.
fn form_localities(labels: &[String]) -> Option<String> {
    first(
        labels
            .iter()
            .filter(|label| label.ends_with(')') && !label.contains(';'))
            .map(|label| label_name(label)),
    )
}

/// The localities' names at the localities' depth of a finding aid, down
/// its first branch holding `act`.
async fn tree_localities(
    settings: &Settings,
    act: &Act,
    fetch: &dyn PortalFetch,
    step: Step,
) -> Result<Vec<String>, Failure> {
    let Search::Tree { aid, depth, label } = &settings.search else {
        return Ok(Vec::new());
    };
    let expected = "the finding aid's localities";
    let fail = |error| Failure::from_error(step, expected, &error);
    let mut level: Vec<Node> = tree::fragment(settings, aid, aid, fetch)
        .await
        .map_err(fail)?;
    let mut at = 1;
    while at < *depth {
        let keeps = |node: &&Node| {
            node.illustrated == Illustrated::Descendants
                && page::act_of(&node.title).is_none_or(|code| holds_act(Some(&code), act))
        };
        let Some(next) = level.iter().find(keeps).cloned() else {
            return Ok(Vec::new());
        };
        level = if next.children.is_empty() {
            tree::fragment(settings, aid, &next.id, fetch)
                .await
                .map_err(fail)?
        } else {
            next.children
        };
        at += 1;
    }
    Ok(level
        .iter()
        .filter(|node| node.illustrated != Illustrated::No)
        .filter_map(|node| tree::locality_of(&node.title, label))
        .collect())
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    let localities = match &settings.search {
        Search::Form { criteria, .. } => {
            let expected = "the form with the settings' inputs and kinds";
            let page = form::form_page(&settings, fetch)
                .await
                .map_err(|error| Failure::from_error(step, expected, &error))?;
            let form = Form::of(&page)
                .ok_or_else(|| Failure::unreadable(step, expected, &page, "no form"))?;
            form::check(&settings, &form)
                .map_err(|error| Failure::from_error(step, expected, &error))?;
            form_localities(&form.options(&format!("query{}", criteria.locality)))
        }
        Search::Tree { .. } => {
            let act = collection
                .acts
                .first()
                .ok_or_else(|| Failure::drift(step, "an act in the collection", "no act"))?;
            first(
                tree_localities(&settings, act, fetch, step)
                    .await?
                    .into_iter(),
            )
        }
    };
    localities.ok_or_else(|| Failure::drift(step, "a locality in the list", "none"))
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
    let fail = |error| Failure::from_error(step, expected, &error);
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
    let found = match &settings.search {
        Search::Form { .. } => form::find(&settings, &citation, &localities, fetch).await,
        Search::Tree { .. } => tree::find(&settings, &citation, &localities, fetch).await,
    }
    .map_err(fail)?;
    if found.candidates.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    // A table of contents gives neither the viewer's ARK nor, for a whole
    // collection, the dates: the first few registers' components do.
    let limit = match settings.search {
        Search::Form { .. } => found.candidates.len(),
        Search::Tree { .. } => MAX_COMPLETED,
    };
    let mut registers = Vec::new();
    for candidate in found.candidates.iter().take(limit) {
        let done = detailed(&settings, candidate, fetch).await.map_err(fail)?;
        registers.push(Register {
            locality: done.locality.unwrap_or_else(|| locality.to_owned()),
            call_number: done.call_number,
            period: done.period,
            images: None,
            address: Some(done.payload.manifest_path(&settings.path)),
            numbers: done.numbers,
        });
    }
    Ok(registers)
}

/// The views of the chosen register's manifest.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    settings(collection, step)?;
    let Some(manifest) = &register.address else {
        return Ok(None);
    };
    let expected = "the register's manifest";
    let answer = fetch
        .get(manifest)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))?;
    let count =
        page::view_count(&answer).map_err(|error| Failure::from_error(step, expected, &error))?;
    Ok(u16::try_from(count).ok())
}

impl Probe for Pleade {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_a_current_commune() {
        let labels: Vec<String> = [
            "Abbaye (Exemple, France ; jusqu'à 1919) [aujourd'hui : Sampleton (Exemple, France)]",
            "Chapelle-Exemple du Bourg",
            "Sampleton (Exemple, France)",
            "Le Bourg (Exemple, France)",
        ]
        .map(str::to_owned)
        .into();
        assert_eq!(form_localities(&labels).as_deref(), Some("Le Bourg"));
    }
}
