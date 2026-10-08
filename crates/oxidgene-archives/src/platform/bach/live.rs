//! The Bach part of the live checks (Archive Portals §9.1).
//!
//! The localities are the classification scheme's entries of the
//! collection, or the nodes of its finding aid at the settings' level; an
//! office's node (`Bureau de recrutement d'Exampleville`) is cited by its
//! place. The registers are the finding aid's leaves that hold the
//! collection's first document kind, which a citation of it would find;
//! their show pages and the viewer count their images, the first few opened
//! until one has some, since an aid lists registers without images too.

use super::page::{self, Node};
use super::register::{self, Leaf};
use super::settings::{CLASSIFICATION_PATH, Inventory, Settings, Views};
use super::{Bach, entry_locality, find_document};
use crate::catalog::Collection;
use crate::citation::{Act, CitationParts};
use crate::live::{Failure, Probe, Register, Step};
use crate::platform::BoxFuture;
use crate::platform::locality::label_name;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;

fn settings(collection: &Collection, step: Step) -> Result<Settings, Failure> {
    Settings::read(collection)
        .map_err(|error| Failure::drift(step, "valid Bach settings", error.to_string()))
}

/// A `GET` of the probe, its failures reported for `step`.
async fn get(
    fetch: &dyn PortalFetch,
    path: &str,
    step: Step,
    expected: &str,
) -> Result<String, Failure> {
    fetch
        .get(path)
        .await
        .map_err(|error| Failure::fetch(step, expected, error))
}

/// The place a node of an office names, as a citation writes it: what
/// follows its last `de`, `d'` or `du` (`Exampleville` for `Bureau de
/// recrutement d'Exampleville`), a parenthesis after it aside; the title
/// itself for a place's own node.
fn place_of(title: &str) -> String {
    let title = label_name(title);
    let lowered = title.to_lowercase();
    let after = [" de ", " d'", " d\u{2019}", " du "]
        .iter()
        .filter_map(|separator| lowered.rfind(separator).map(|at| at + separator.len()))
        .max();
    let place = after
        .and_then(|at| title.get(at..))
        .unwrap_or(&title)
        .trim();
    label_name(place)
}

/// The alphabetically first of `names`.
fn first(names: impl Iterator<Item = String>) -> Option<String> {
    names
        .filter(|name| !name.trim().is_empty())
        .min_by_key(|name| fold(name))
}

async fn search_page(collection: &Collection, fetch: &dyn PortalFetch) -> Result<String, Failure> {
    let step = Step::SearchPage;
    let settings = settings(collection, step)?;
    match &settings.inventory {
        Inventory::Classification {
            prefix,
            locality,
            title_prefixes,
        } => {
            let expected = "the collection's finding aids in the classification scheme";
            let answer = get(fetch, CLASSIFICATION_PATH, step, expected).await?;
            let entries = page::entries(&answer, prefix)
                .map_err(|error| Failure::from_error(step, expected, &error))?;
            first(
                entries
                    .iter()
                    .filter_map(|entry| entry_locality(entry, *locality, title_prefixes))
                    .map(|name| label_name(&name)),
            )
            .ok_or_else(|| Failure::drift(step, expected, "none"))
        }
        Inventory::Document { document, level } => {
            let expected = "the finding aid's tree";
            let answer = get(fetch, &Settings::document_path(document), step, expected).await?;
            let aid = page::tree(&answer, document)
                .map_err(|error| Failure::from_error(step, expected, &error))?;
            let nodes = &aid.nodes;
            let Some(level) = level.map(usize::from) else {
                // A series of a single office: searched without a locality.
                return if nodes.iter().any(|node| node.leaf) {
                    Ok(String::new())
                } else {
                    Err(Failure::drift(step, expected, "no register"))
                };
            };
            first(
                nodes
                    .iter()
                    .filter(|node| node.depth == level && !node.leaf)
                    .map(|node: &Node| place_of(&node.title))
                    // A year's node beside the offices (a class of the
                    // conscription lists) names no place.
                    .filter(|place| place.chars().any(char::is_alphabetic)),
            )
            .ok_or_else(|| {
                Failure::drift(step, "the localities of the finding aid's level", "none")
            })
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
    let expected = "the registers of the first listed locality";
    let settings = settings(collection, step)?;
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
    let (document, level) = find_document(&settings, &citation, fetch)
        .await
        .map_err(|error| Failure::from_error(step, expected, &error))?
        .map_err(|matches| {
            Failure::drift(
                step,
                "one finding aid of the locality",
                format!("{matches}"),
            )
        })?;
    let answer = get(fetch, &Settings::document_path(&document), step, expected).await?;
    let aid = page::tree(&answer, &document)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let leaves: Vec<Leaf> = register::leaves(&aid, level, locality);
    let found = register::candidates(&leaves, &citation);
    if found.is_empty() {
        return Err(Failure::drift(step, expected, "no register"));
    }
    let mut registers: Vec<Register> = found
        .into_iter()
        .map(|candidate| Register {
            locality: locality.to_owned(),
            call_number: candidate.call_number,
            period: candidate.period,
            images: None,
            address: Some(format!(
                "/archives/show/{document}_{}",
                candidate.payload.id
            )),
            numbers: candidate.numbers,
        })
        .collect();
    // An aid lists registers without images beside the others: the first
    // registers a citation would name, call numbers first, are opened until
    // one links to its images, those without counted as having none.
    let mut order: Vec<usize> = (0..registers.len()).collect();
    order.sort_by_key(|at| registers[*at].call_number.is_none());
    for at in order.into_iter().take(PROBED) {
        let count = images(collection, &registers[at], fetch).await?;
        registers[at].images = Some(count.unwrap_or(0));
        if count.is_some() {
            break;
        }
    }
    Ok(registers)
}

/// The registers the discovery opens at most to find one with images.
const PROBED: usize = 3;

/// The images of a register, as its show page links them and the viewer
/// lists them; `None` for a register without one viewer link.
async fn images(
    collection: &Collection,
    register: &Register,
    fetch: &dyn PortalFetch,
) -> Result<Option<u16>, Failure> {
    let step = Step::Discovery;
    let expected = "the chosen register's viewer link and images";
    let settings = settings(collection, step)?;
    let Some(address) = &register.address else {
        return Ok(None);
    };
    let show = get(fetch, address, step, expected).await?;
    let links = page::viewer_links(&show, &settings.viewer)
        .map_err(|error| Failure::from_error(step, expected, &error))?;
    let [link] = links.as_slice() else {
        return Ok(None);
    };
    let count = match settings.views {
        Views::Api => {
            let list = settings
                .image_list(link)
                .ok_or_else(|| Failure::drift(step, expected, "a link without a folder"))?;
            let answer = get(fetch, &list, step, expected).await?;
            page::image_names(&answer)
                .map_err(|error| Failure::from_error(step, expected, &error))?
                .len()
        }
        Views::Range => page::range_names(link).map_or(0, |names| names.len()),
    };
    Ok(u16::try_from(count).ok().filter(|count| *count > 0))
}

impl Probe for Bach {
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
