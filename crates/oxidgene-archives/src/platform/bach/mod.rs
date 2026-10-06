//! Bach (Anaphore), the search engine of several departmental archives,
//! whose registers are the components of EAD finding aids.
//!
//! A Bach portal has no register search: its classification scheme lists
//! the finding aids, one per locality or one for every locality, and a
//! finding aid's page shows its whole tree, a register being a leaf whose
//! show page links to the portal's viewer, a separate application on the
//! portal's origin or on its own. The adapter reads the classification
//! scheme (for a collection of one aid per locality), the locality's
//! finding aid, the chosen register's show page and the viewer's image
//! list, then opens the viewer on the cited image by its name. Archive
//! Portals §4.8 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
mod register;
mod settings;
#[cfg(test)]
mod tests;

use super::query::encode;
use super::select::{Candidate, Selection, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::CitationParts;
use crate::platform::markup::fold;
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ArchiveView, ResolveError};
use page::{Entry, unexpected};
use register::{Register, same_place};
use settings::{CLASSIFICATION_PATH, Inventory, Label, Settings, Views};

/// The Bach adapter.
pub struct Bach;

impl Platform for Bach {
    fn id(&self) -> &'static str {
        "bach"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        let viewer = settings.viewer_origin()?.to_owned();
        // Where an anti-bot check guards the portal, a page it guards, so
        // that the browser passes it before the requests; the lightest page
        // otherwise.
        let start = match settings.transport {
            Access::Browser | Access::Page => CLASSIFICATION_PATH,
            Access::Any => "/robots.txt",
        };
        Some(PortalEndpoint {
            start: format!("{}{start}", settings.origin),
            other_origins: if viewer == settings.origin {
                Vec::new()
            } else {
                vec![viewer]
            },
            origin: settings.origin,
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The portal's search is the full-text engine its robots.txt keeps
        // robots from: the locality's finding aid is the closest page a
        // reader can open without a request, or the list of finding aids.
        Some(Settings::read(collection).ok()?.landing(None))
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

fn results(url: String, matches: usize) -> ArchiveTarget {
    ArchiveTarget::Results {
        url,
        matches: Some(matches),
    }
}

/// The locality a classification entry names, after one of the titles the
/// collection's entries start with; `None` for an entry of another title.
fn entry_locality(entry: &Entry, label: Label, title_prefixes: &[String]) -> Option<String> {
    let after = if title_prefixes.is_empty() {
        entry.title.as_str()
    } else {
        title_prefixes
            .iter()
            .find_map(|prefix| strip_words(&entry.title, prefix))?
    };
    let locality = match label {
        Label::Title => after,
        Label::Link => entry.link.as_str(),
    };
    let locality = locality.trim();
    (!locality.is_empty()).then(|| locality.to_owned())
}

/// `text` after its first words, when they are `prefix`'s, case, accents
/// and punctuation aside: `Saint-Exemple` after `Registres paroissiaux et
/// d'état civil :` in `Registres paroissiaux et d’état civil : Saint-Exemple`.
fn strip_words<'t>(text: &'t str, prefix: &str) -> Option<&'t str> {
    let wanted = fold(prefix);
    let mut boundaries = text
        .char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()));
    boundaries
        .find(|at| {
            fold(&text[..*at]) == wanted
                && !text[*at..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
        })
        .map(|at| text[at..].trim_start_matches(|c: char| !c.is_alphanumeric() && c != '('))
}

/// The finding aid of the cited locality among the classification
/// scheme's entries: its identifier, or the number of entries naming it
/// when it is not exactly one.
async fn find_document(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<Result<(String, Option<usize>), usize>, ResolveError> {
    let (prefix, label, title_prefixes) = match &settings.inventory {
        Inventory::Document { document, level } => {
            return Ok(Ok((document.clone(), level.map(usize::from))));
        }
        Inventory::Classification {
            prefix,
            locality,
            title_prefixes,
        } => (prefix, *locality, title_prefixes.as_slice()),
    };
    let page = fetch.get(CLASSIFICATION_PATH).await?;
    let entries = page::entries(&page, prefix)?;
    let named: Vec<&Entry> = entries
        .iter()
        .filter(|entry| entry_locality(entry, label, title_prefixes).is_some())
        .collect();
    if named.is_empty() {
        return Err(unexpected(
            "the classification scheme lists none of the collection's finding aids",
        ));
    }
    let matching: Vec<&Entry> = named
        .into_iter()
        .filter(|entry| {
            entry_locality(entry, label, title_prefixes)
                .is_some_and(|locality| same_place(&locality, &citation.locality))
        })
        .collect();
    Ok(match matching.as_slice() {
        [entry] => Ok((entry.document.clone(), None)),
        entries => Err(entries.len()),
    })
}

/// The cited register among the candidates: by act, parish and period
/// first, the call number only breaking a tie, since a portal may show
/// none (the Vaucluse communes' aids) or one shared by several registers.
fn choose<'c>(
    candidates: &'c [Candidate<Register>],
    citation: &CitationParts,
) -> Selection<'c, Register> {
    let localities = [citation.locality.as_str()];
    let mut without = citation.clone();
    without.call_number = None;
    let selection = match select(candidates, &without, &localities) {
        Selection::Many(count) if count > 1 && citation.call_number.is_some() => {
            match select(candidates, citation, &localities) {
                Selection::One(one) => Selection::One(one),
                Selection::Many(_) => Selection::Many(count),
            }
        }
        other => other,
    };
    // A register showing another call number is not the cited one, which
    // another collection may hold: the aid of decennial tables beside the
    // registers holding tables too.
    match (selection, &citation.call_number) {
        (Selection::One(one), Some(cited))
            if one
                .call_number
                .as_deref()
                .is_some_and(|shown| !cited.matches(shown)) =>
        {
            Selection::Many(1)
        }
        (selection, _) => selection,
    }
}

/// The viewer link opening a register on the image `name`.
fn on_image(link: &str, name: &str) -> String {
    let separator = if link.contains('?') { '&' } else { '?' };
    format!("{link}{separator}img={}", encode(name))
}

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let (document, level) = match find_document(&settings, citation, fetch).await? {
        Ok(found) => found,
        Err(matches) => return Ok(results(settings.landing(None), matches)),
    };
    let landing = settings.landing(Some(&document));

    let page = fetch.get(&Settings::document_path(&document)).await?;
    let aid = page::tree(&page, &document)?;
    let leaves = register::leaves(&aid, level, &citation.locality);
    let candidates = register::candidates(&leaves, citation);
    let chosen = match choose(&candidates, citation) {
        Selection::One(chosen) => chosen,
        Selection::Many(matches) => return Ok(results(landing, matches)),
    };

    let id = &chosen.payload.id;
    let show = fetch
        .get(&format!("/archives/show/{document}_{id}"))
        .await?;
    // A register listed without images, or split over several folders.
    let link = match page::viewer_links(&show, &settings.viewer)?.as_slice() {
        [link] => link.clone(),
        links => return Ok(results(format!("{landing}#{id}"), links.len().max(1))),
    };

    let names = image_names(&settings, &link, fetch).await?;
    if names.is_empty() && settings.views == Views::Api {
        return Ok(results(format!("{landing}#{id}"), 1));
    }
    Ok(target(
        archive,
        citation,
        chosen.call_number.as_deref(),
        link,
        &names,
    ))
}

/// The image names of a register, in view order: the viewer's list, or the
/// range its link names; none when a link names no range, which leaves the
/// view to the viewer.
async fn image_names(
    settings: &Settings,
    link: &str,
    fetch: &dyn PortalFetch,
) -> Result<Vec<String>, ResolveError> {
    match settings.views {
        Views::Api => {
            let list = settings
                .image_list(link)
                .ok_or_else(|| unexpected("a viewer link without a folder"))?;
            page::image_names(&fetch.get(&list).await?)
        }
        Views::Range => Ok(page::range_names(link).unwrap_or_default()),
    }
}

/// The `View` target of a register: its viewer link, opened on the cited
/// images by their names.
fn target(
    archive: &Archive,
    citation: &CitationParts,
    call_number: Option<&str>,
    link: String,
    names: &[String],
) -> ArchiveTarget {
    let count = if names.is_empty() {
        usize::MAX
    } else {
        names.len()
    };
    let views = cited_views(citation, count)
        .iter()
        .filter_map(|cited| {
            let name = names.get(usize::from(cited.view).checked_sub(1)?)?;
            Some(ArchiveView {
                view: cited.view,
                url: on_image(&link, name),
                ark: None,
                image: None,
            })
        })
        .collect();
    view_target(archive, citation, call_number, count, link, views)
}
