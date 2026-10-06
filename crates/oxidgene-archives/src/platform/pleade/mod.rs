//! Pleade (AJLSM), the EAD publishing software behind the Mayenne and
//! Pyrénées-Atlantiques portals.
//!
//! Registers are components of EAD finding aids, each opening in a Mirador
//! viewer at its ARK, view `n` at `…/ark:/<naan>/<name>/f<n>`. A collection
//! finds its register in one of two ways, its `mode`:
//!
//! - `form` ([`form`]): a search form whose results list the registers, the
//!   locality written as the form's list writes it;
//! - `tree` ([`tree`]): a walk down a finding aid's table of contents, from
//!   the locality's node through the nodes naming the cited kind and years
//!   to the registers.
//!
//! Neither publishes an image count with a register: the register's IIIF
//! manifest gives it, read only to tell registers apart by the cited count.
//! Archive Portals §4.10 specifies the requests.

mod form;
#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;
mod tree;

use serde::Deserialize;

use super::locality::forms;
use super::markup;
use super::select::{Candidate, narrow};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, is_https_origin, refuse_series};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::CitationParts;
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ArchiveView, ResolveError};
use page::Ark;

/// The Pleade adapter.
pub struct Pleade;

/// How many registers' manifests are read to tell them apart by the cited
/// image count, when nothing else does.
const MAX_COUNTED: usize = 3;

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("pleade settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("pleade: {detail}"))
}

async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    let body = fetch.get(path).await?;
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

/// How a collection finds its register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Form,
    Tree,
}

/// The criteria of a `form` collection, by the number of their inputs:
/// `query<n>` for the locality and the kind of document, `du<n>`, `db<n>`
/// and `de<n>` for the year.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Criteria {
    locality: u8,
    kind: u8,
    year: u8,
}

/// The values of the kind-of-document criterion.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Kinds {
    registers: String,
    tables: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    path: String,
    mode: Mode,
    #[serde(default)]
    form: Option<String>,
    #[serde(default)]
    results: Option<String>,
    #[serde(default)]
    criteria: Option<Criteria>,
    #[serde(default)]
    kinds: Option<Kinds>,
    #[serde(default)]
    aid: Option<String>,
    #[serde(default)]
    locality_depth: Option<u8>,
    #[serde(default)]
    locality_label: Option<String>,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    /// The Pleade mount: `/archives-en-ligne`.
    path: String,
    search: Search,
}

#[derive(Debug, Clone)]
enum Search {
    Form {
        /// The form page and the results fragment, relative to `path`.
        page: String,
        results: String,
        criteria: Criteria,
        kinds: Kinds,
    },
    Tree {
        /// The finding aid's identifier.
        aid: String,
        /// The depth of the table of contents at which the localities'
        /// nodes stand, the aid's first level being 1.
        depth: u8,
        /// How a locality's node is titled, with one `{locality}`.
        label: String,
    },
}

/// A path relative to the Pleade mount: no leading slash, no query.
fn is_relative(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('/')
        && !text.contains(['?', '#', ' ', '\\', ':'])
        && !text.contains("..")
}

/// An EAD identifier: letters, digits, `_` and `-`.
fn is_identifier(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        let settings = Self::from_raw(raw)?;
        if matches!(settings.search, Search::Form { .. }) {
            // The form searches acts and tables.
            refuse_series("pleade", collection)?;
        }
        Ok(settings)
    }

    fn from_raw(raw: Raw) -> Result<Self, CatalogError> {
        if !is_https_origin(&raw.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !raw.path.starts_with('/')
            || raw.path.ends_with('/')
            || raw.path.contains(['?', '#', ' '])
        {
            return Err(invalid(
                "path must be an absolute path without a trailing slash",
            ));
        }
        let search = match raw.mode {
            Mode::Form => {
                if raw.aid.is_some() || raw.locality_depth.is_some() || raw.locality_label.is_some()
                {
                    return Err(invalid(
                        "a `form` collection takes form, results, criteria and kinds",
                    ));
                }
                let (Some(page), Some(results), Some(criteria), Some(kinds)) =
                    (raw.form, raw.results, raw.criteria, raw.kinds)
                else {
                    return Err(invalid(
                        "a `form` collection needs form, results, criteria and kinds",
                    ));
                };
                if !is_relative(&page) || !is_relative(&results) {
                    return Err(invalid("form and results are paths under the Pleade mount"));
                }
                let numbers = [criteria.locality, criteria.kind, criteria.year];
                if numbers.iter().any(|n| !(1..=9).contains(n))
                    || numbers[0] == numbers[1]
                    || numbers[1] == numbers[2]
                    || numbers[0] == numbers[2]
                {
                    return Err(invalid("criteria are distinct input numbers, 1 to 9"));
                }
                if kinds.registers.trim().is_empty() || kinds.tables.trim().is_empty() {
                    return Err(invalid("kinds name the registers' and the tables' values"));
                }
                Search::Form {
                    page,
                    results,
                    criteria,
                    kinds,
                }
            }
            Mode::Tree => {
                if raw.form.is_some()
                    || raw.results.is_some()
                    || raw.criteria.is_some()
                    || raw.kinds.is_some()
                {
                    return Err(invalid(
                        "a `tree` collection takes aid, locality_depth and locality_label",
                    ));
                }
                let aid = raw
                    .aid
                    .filter(|aid| is_identifier(aid))
                    .ok_or_else(|| invalid("`aid` is missing or malformed"))?;
                let depth = raw
                    .locality_depth
                    .filter(|depth| (1..=10).contains(depth))
                    .ok_or_else(|| invalid("`locality_depth` is missing or out of 1 to 10"))?;
                let label = raw
                    .locality_label
                    .filter(|label| label.matches("{locality}").count() == 1)
                    .ok_or_else(|| invalid("`locality_label` needs one `{locality}`"))?;
                Search::Tree { aid, depth, label }
            }
        };
        Ok(Self {
            origin: raw.origin,
            transport: raw.transport,
            path: raw.path,
            search,
        })
    }

    /// The portal's origin and mount, which viewer links start with.
    fn prefix(&self) -> String {
        format!("{}{}", self.origin, self.path)
    }

    /// The page a reader opens to search by hand: the form, or the finding
    /// aid.
    fn search_page(&self) -> String {
        match &self.search {
            Search::Form { page, .. } => format!("{}/{page}", self.prefix()),
            Search::Tree { aid, .. } => format!("{}/ead.html?id={aid}", self.prefix()),
        }
    }
}

/// A register a search found, as the viewer opens it: its ARK when the
/// search gave it, or the finding aid's component that gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Register {
    Ark(Ark),
    Component(String),
}

/// What a search found, before the registers it cannot tell apart are
/// counted.
struct Found {
    candidates: Vec<Candidate<Register>>,
    /// Where a reader sees the same results, when the search has a page.
    results_url: Option<String>,
    /// The registers found beyond those read, all of them counted.
    total: usize,
}

/// The register a selection chose, with its ARK and, where it was read,
/// its image count.
struct Chosen {
    ark: Ark,
    call_number: Option<String>,
    period: Option<String>,
    images: Option<usize>,
}

/// The ARK of a register: its own, or its component fragment's, which also
/// gives the call number and dates the table of contents lacks.
async fn detailed(
    settings: &Settings,
    candidate: &Candidate<Register>,
    fetch: &dyn PortalFetch,
) -> Result<Candidate<Ark>, ResolveError> {
    let mut detailed = Candidate {
        locality: candidate.locality.clone(),
        call_number: candidate.call_number.clone(),
        act: candidate.act.clone(),
        parish: candidate.parish.clone(),
        period: candidate.period.clone(),
        images: candidate.images,
        numbers: candidate.numbers,
        payload: Ark {
            naan: String::new(),
            name: String::new(),
        },
    };
    match &candidate.payload {
        Register::Ark(ark) => detailed.payload = ark.clone(),
        Register::Component(id) => {
            let fragment =
                get(fetch, &format!("{}/ead-fragment.xsp?c={id}", settings.path)).await?;
            let component = page::component(&fragment, &settings.prefix())?;
            detailed.payload = component
                .ark
                .ok_or_else(|| unexpected("the register's component has no viewer link"))?;
            if detailed.call_number.is_none() {
                detailed.call_number = component.call_number;
            }
            if detailed.period.is_none() {
                detailed.period = component.period;
            }
        }
    }
    Ok(detailed)
}

/// Chooses the cited register: the one the citation's parts single out, its
/// component read for its ARK; or, of at most [`MAX_COUNTED`] left, the
/// ones whose components complete them, then the one whose manifest counts
/// the cited images. `Err` is the number of registers left apart.
async fn choose(
    settings: &Settings,
    candidates: &[Candidate<Register>],
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Result<Chosen, usize>, ResolveError> {
    let kept = narrow(candidates, citation, localities);
    if kept.is_empty() || kept.len() > MAX_COUNTED {
        return Ok(Err(kept.len()));
    }
    let mut completed = Vec::new();
    for candidate in &kept {
        completed.push(detailed(settings, candidate, fetch).await?);
    }
    let left = narrow(&completed, citation, localities);
    if let [only] = left.as_slice() {
        return Ok(Ok(Chosen {
            ark: only.payload.clone(),
            call_number: only.call_number.clone(),
            period: only.period.clone(),
            images: None,
        }));
    }
    let Some(cited) = citation.view_count else {
        return Ok(Err(left.len()));
    };
    let mut matching = Vec::new();
    for candidate in &left {
        let count =
            page::view_count(&get(fetch, &candidate.payload.manifest_path(&settings.path)).await?)?;
        if count == usize::from(cited) {
            matching.push((*candidate, count));
        }
    }
    Ok(match matching.as_slice() {
        [(only, count)] => Ok(Chosen {
            ark: only.payload.clone(),
            call_number: only.call_number.clone(),
            period: only.period.clone(),
            images: Some(*count),
        }),
        _ => Err(left.len()),
    })
}

/// The `View` target of a chosen register: Mirador at the cited view.
fn view(
    settings: &Settings,
    archive: &Archive,
    citation: &CitationParts,
    chosen: &Chosen,
) -> ArchiveTarget {
    // A register is counted only where its manifest was read: a cited view
    // is otherwise left to the viewer.
    let count = chosen.images.unwrap_or(usize::MAX);
    let views = cited_views(citation, count, chosen.period.as_deref())
        .iter()
        .map(|cited| {
            let url = format!(
                "{}{}",
                settings.origin,
                chosen.ark.view_path(&settings.path, cited.view)
            );
            ArchiveView {
                view: cited.view,
                ark: Some(url.clone()),
                url,
                image: None,
            }
        })
        .collect();
    view_target(
        archive,
        citation,
        chosen.call_number.as_deref(),
        count,
        format!(
            "{}{}",
            settings.origin,
            chosen.ark.view_path(&settings.path, 1)
        ),
        views,
    )
}

impl Platform for Pleade {
    fn id(&self) -> &'static str {
        "pleade"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}/robots.txt", settings.origin),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
            insecure_http: false,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The form's locality is a label of its list and the aid's a node
        // of its tree: neither is known without a request.
        Some(Settings::read(collection).ok()?.search_page())
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

async fn resolve(
    archive: &Archive,
    collection: &Collection,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let settings = Settings::read(collection).map_err(|_| ResolveError::NoAdapter)?;
    let wanted = forms(&citation.locality);
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let found = match &settings.search {
        Search::Form { .. } => form::find(&settings, citation, &localities, fetch).await?,
        Search::Tree { .. } => tree::find(&settings, citation, &localities, fetch).await?,
    };
    let left = match choose(&settings, &found.candidates, citation, &localities, fetch).await? {
        Ok(chosen) => return Ok(view(&settings, archive, citation, &chosen)),
        Err(left) => left,
    };
    Ok(ArchiveTarget::Results {
        url: found.results_url.unwrap_or_else(|| settings.search_page()),
        matches: Some(if found.candidates.is_empty() {
            found.total
        } else {
            left
        }),
    })
}
