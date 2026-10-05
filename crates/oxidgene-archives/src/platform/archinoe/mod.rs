//! Archinoë (EidoPolis), the publishing software of several departmental
//! archives, one viewer behind three search modules.
//!
//! Every portal opens a register in the same viewer,
//! `visualiseur/<page>.html?id=<id>&vue=<n>` with `n` one-based, so one
//! adapter serves them; its `search` setting names the module in front of
//! the viewer:
//!
//! - `registre` ([`registre`]): a form that answers a plain `GET` with the
//!   matching registers' rows, once the locality's identifier is read from
//!   the form's locality list;
//! - `seriel` ([`seriel`]): a form that answers a form-encoded `POST`, the
//!   locality written as the portal's own label;
//! - `ead` ([`ead`]): a finding aid browsed commune, act node, collection,
//!   whose register blocks give the call number, period and image count.
//!
//! No module has a persistent view address or an IIIF service, so every
//! target opens the portal's viewer. Archive Portals §4.6 specifies the
//! requests.

mod ead;
mod registre;
mod seriel;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::forms;
use super::markup;
use super::select::{Candidate, narrow};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ArchiveView, ResolveError};

/// The Archinoë adapter.
pub struct Archinoe;

/// How many registers' viewer pages are read to tell registers apart by
/// their image count, when the results lack it.
const MAX_COUNTED: usize = 3;

/// The marker of every view in a viewer page: the page lists one
/// `div_image_<n>` per view.
const VIEW_MARKER: &str = "id=\"div_image_";

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("archinoe settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("archinoe: {detail}"))
}

/// A body that is not an anti-bot challenge, which is reported as such and
/// not as a change of the portal's shape.
fn unchallenged(body: String) -> Result<String, ResolveError> {
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

/// A `GET` of a path on the portal's origin.
async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    unchallenged(fetch.get(path).await?)
}

/// The search module in front of the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Registre,
    Seriel,
    Ead,
}

/// The setting `licence`: how the portal gates its search page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Licence {
    /// `registre.html` leads to a page whose link the reader clicks to
    /// accept the re-use terms.
    Click,
}

/// A collection's `portal` settings as the catalogue writes them: one flat
/// object whose mode-specific members [`Settings::from_raw`] checks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    search: Mode,
    base: String,
    viewer: String,
    acts: BTreeMap<String, String>,
    #[serde(default)]
    fields: Option<RawFields>,
    #[serde(default)]
    licence: Option<Licence>,
    #[serde(default)]
    locality_label: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    form: Option<String>,
    #[serde(default)]
    ir: Option<String>,
    #[serde(default)]
    eadid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFields {
    locality: Option<String>,
    act: Option<String>,
    year: Option<String>,
    year_from: Option<String>,
    year_to: Option<String>,
    cote: Option<String>,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    base: String,
    viewer: String,
    /// The portal's value of each act code: an act identifier (`registre`),
    /// a checkbox name (`seriel`), or the title of the commune's act node
    /// (`ead`).
    acts: BTreeMap<String, String>,
    search: Search,
}

#[derive(Debug, Clone)]
enum Search {
    Registre {
        locality: String,
        act: String,
        year: String,
        /// The portal gates its search page behind a licence the reader
        /// accepts.
        licence: bool,
    },
    Seriel {
        id: String,
        form: String,
        locality: String,
        year_from: String,
        year_to: String,
        cote: String,
        /// How the portal writes a locality: `{locality} (Pas-de-Calais,
        /// France)`.
        locality_label: String,
    },
    Ead {
        ir: String,
        eadid: String,
    },
}

/// A name the portal's forms use: letters, digits and `_`.
fn is_reference(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A required member of the mode, checked with `valid`.
fn required(
    value: Option<String>,
    name: &str,
    valid: impl Fn(&str) -> bool,
) -> Result<String, CatalogError> {
    value
        .filter(|text| valid(text))
        .ok_or_else(|| invalid(&format!("`{name}` is missing or malformed")))
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        let settings = Self::from_raw(raw)?;
        settings.check_acts(collection)?;
        Ok(settings)
    }

    fn from_raw(raw: Raw) -> Result<Self, CatalogError> {
        if !is_https_origin(&raw.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !raw.base.starts_with('/')
            || raw.base.ends_with('/')
            || raw.base.contains(['?', '#', ' '])
        {
            return Err(invalid(
                "base must be an absolute path without a trailing slash",
            ));
        }
        if !raw.viewer.starts_with('/')
            || raw.viewer.matches("{id}").count() != 1
            || !raw.viewer.contains('?')
            || raw.viewer.contains(['#', ' '])
        {
            return Err(invalid("viewer must be a path with a query and one `{id}`"));
        }
        let has_fields = raw.fields.is_some();
        let fields = raw.fields.unwrap_or_default();
        let search = match raw.search {
            Mode::Registre => {
                if raw.id.is_some()
                    || raw.form.is_some()
                    || raw.ir.is_some()
                    || raw.eadid.is_some()
                    || raw.locality_label.is_some()
                    || fields.year_from.is_some()
                    || fields.year_to.is_some()
                    || fields.cote.is_some()
                {
                    return Err(invalid(
                        "a `registre` search takes locality, act and year fields",
                    ));
                }
                if !raw.acts.values().all(|value| is_number(value)) {
                    return Err(invalid("`registre` acts are numeric act identifiers"));
                }
                Search::Registre {
                    locality: required(fields.locality, "fields.locality", is_reference)?,
                    act: required(fields.act, "fields.act", is_reference)?,
                    year: required(fields.year, "fields.year", is_reference)?,
                    licence: raw.licence.is_some(),
                }
            }
            Mode::Seriel => {
                if raw.ir.is_some()
                    || raw.eadid.is_some()
                    || raw.licence.is_some()
                    || fields.act.is_some()
                    || fields.year.is_some()
                {
                    return Err(invalid(
                        "a `seriel` search takes locality, year and cote fields",
                    ));
                }
                if !raw.acts.values().all(|value| is_reference(value)) {
                    return Err(invalid("`seriel` acts are checkbox names"));
                }
                Search::Seriel {
                    id: required(raw.id, "id", is_number)?,
                    form: required(raw.form, "form", is_reference)?,
                    locality: required(fields.locality, "fields.locality", is_reference)?,
                    year_from: required(fields.year_from, "fields.year_from", is_reference)?,
                    year_to: required(fields.year_to, "fields.year_to", is_reference)?,
                    cote: required(fields.cote, "fields.cote", is_reference)?,
                    locality_label: required(raw.locality_label, "locality_label", |text| {
                        text.matches("{locality}").count() == 1
                    })?,
                }
            }
            Mode::Ead => {
                if raw.id.is_some()
                    || raw.form.is_some()
                    || raw.licence.is_some()
                    || raw.locality_label.is_some()
                    || has_fields
                {
                    return Err(invalid("an `ead` search takes `ir` and `eadid` only"));
                }
                if !raw
                    .acts
                    .values()
                    .all(|title| !title.is_empty() && !title.chars().any(char::is_control))
                {
                    return Err(invalid("`ead` acts are node titles"));
                }
                Search::Ead {
                    ir: required(raw.ir, "ir", is_number)?,
                    eadid: required(raw.eadid, "eadid", is_reference)?,
                }
            }
        };
        Ok(Self {
            origin: raw.origin,
            transport: raw.transport,
            base: raw.base,
            viewer: raw.viewer,
            acts: raw.acts,
            search,
        })
    }

    /// Every act code is valid, and every act the collection holds has a
    /// value.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for code in self.acts.keys() {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_value(act).is_none())
        {
            Some(act) => Err(invalid(&format!("no value for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The portal's value of an act: its own code's, or for a combined act
    /// (`BMS`) its first kind's, whose category holds the mixed registers.
    fn act_value(&self, act: &Act) -> Option<&str> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.kinds().first()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(String::as_str)
    }

    /// The page a reader opens to search by hand, and a browser transport
    /// loads first.
    fn search_page(&self) -> String {
        match &self.search {
            Search::Registre { .. } => format!("{}{}/registre.html", self.origin, self.base),
            Search::Seriel { id, form, .. } => {
                format!(
                    "{}{}/ir_seriel.php?id={id}&p={form}",
                    self.origin, self.base
                )
            }
            Search::Ead { ir, eadid } => format!(
                "{}{}/ir_ead_visu.php?eadid={eadid}&ir={ir}",
                self.origin, self.base
            ),
        }
    }

    /// The viewer's path and query for a register.
    fn viewer_path(&self, id: &str) -> String {
        self.viewer.replace("{id}", id)
    }

    /// The viewer's address, opened on `view` (one-based) when given.
    fn view_url(&self, id: &str, view: Option<u16>) -> String {
        let mut url = format!("{}{}", self.origin, self.viewer_path(id));
        if let Some(view) = view {
            url.push_str(&format!("&vue={view}"));
        }
        url
    }
}

/// A register a search listed, as the viewer opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    /// The viewer's `id`: digits.
    id: String,
}

/// The register a selection chose.
struct Chosen {
    /// The viewer's `id`.
    id: String,
    call_number: Option<String>,
    /// The image count, when the row or the viewer gave it.
    images: Option<usize>,
}

impl Chosen {
    fn new(candidate: &Candidate<Register>, counted: Option<usize>) -> Self {
        Self {
            id: candidate.payload.id.clone(),
            call_number: candidate.call_number.clone(),
            images: counted.or_else(|| candidate.images.map(usize::from)),
        }
    }
}

/// Chooses the cited register among a search's rows, reading the image count
/// from the viewer of each remaining register when the rows lack it and the
/// citation gives one. `Err` is the number of registers left apart.
async fn choose(
    settings: &Settings,
    candidates: &[Candidate<Register>],
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Result<Chosen, usize>, ResolveError> {
    let kept = narrow(candidates, citation, localities);
    let counted = (2..=MAX_COUNTED).contains(&kept.len())
        && kept.iter().all(|candidate| candidate.images.is_none());
    let (Some(cited), true) = (citation.view_count, counted) else {
        return Ok(match kept.as_slice() {
            [only] => Ok(Chosen::new(only, None)),
            _ => Err(kept.len()),
        });
    };
    let mut matching = Vec::new();
    for candidate in &kept {
        let page = get(fetch, &settings.viewer_path(&candidate.payload.id)).await?;
        let views = page.matches(VIEW_MARKER).count();
        if views == 0 {
            return Err(unexpected("the viewer page lists no view"));
        }
        if views == usize::from(cited) {
            matching.push((*candidate, views));
        }
    }
    Ok(match matching.as_slice() {
        [(only, views)] => Ok(Chosen::new(only, Some(*views))),
        _ => Err(kept.len()),
    })
}

/// The `View` target of a chosen register.
fn view(
    settings: &Settings,
    archive: &Archive,
    citation: &CitationParts,
    chosen: &Chosen,
) -> ArchiveTarget {
    // A register's views are counted only where its row or its viewer gave
    // the count: a cited view is otherwise left to the viewer.
    let count = chosen.images.unwrap_or(usize::MAX);
    let views = cited_views(citation, count)
        .iter()
        .map(|cited| ArchiveView {
            view: cited.view,
            url: settings.view_url(&chosen.id, Some(cited.view)),
            ark: None,
            image: None,
        })
        .collect();
    view_target(
        archive,
        citation,
        chosen.call_number.as_deref(),
        count,
        settings.view_url(&chosen.id, None),
        views,
    )
}

/// What a portal's search resolved to, before it is a target.
enum Found {
    /// One register.
    One(Chosen),
    /// The count of registers the citation cannot tell apart.
    Many(usize),
    /// The reader must act on the portal first, such as accept a licence:
    /// the search page, with no match count.
    ReaderStep,
}

impl Platform for Archinoe {
    fn id(&self) -> &'static str {
        "archinoe"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: settings.search_page(),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The portals' forms are filled by identifiers or scripts, not by an
        // address: the search page is the closest page a reader can use.
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
        Search::Registre { .. } => registre::find(&settings, citation, &localities, fetch).await?,
        Search::Seriel { .. } => seriel::find(&settings, citation, &localities, fetch).await?,
        Search::Ead { .. } => ead::find(&settings, citation, &localities, fetch).await?,
    };
    Ok(match found {
        Found::One(chosen) => view(&settings, archive, citation, &chosen),
        Found::Many(matches) => ArchiveTarget::Results {
            url: settings.search_page(),
            matches: Some(matches),
        },
        Found::ReaderStep => ArchiveTarget::Results {
            url: settings.search_page(),
            matches: None,
        },
    })
}

/// A query string from `(name, value)` pairs, percent-encoded.
fn query(pairs: &[(&str, &str)]) -> String {
    let mut query = Query::new();
    for (name, value) in pairs {
        query.push(*name, *value);
    }
    query.to_string()
}

/// The digits that follow `marker` in `text`: a register's identifier.
fn number_after(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let digits: String = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then_some(digits)
}
