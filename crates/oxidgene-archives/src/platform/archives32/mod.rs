//! The Gers archives' portal of digitized archives
//! (`/archives_numerisees/portail/…`), a PHP application whose modules —
//! civil status, parish registers, decennial tables, censuses, tables of
//! successions — each have a search form (`<module>/recherche/`) and a
//! viewer (`<module>/visu/`).
//!
//! A search is a form-encoded `POST` of the form, stateless: a locality
//! chosen from its list (or a former commune from a second list), the kinds
//! of acts as checkboxes, years the adapter leaves out since the portal's
//! year filter is unreliable. The answer repeats the form, with its lists,
//! and lists every register in one table, each with its call number, period,
//! acts, image count and viewer link. The viewer opens a register at a
//! view's identifier (`fichier`); the civil status, parish registers and
//! tables number their views contiguously, the censuses and tables of
//! successions list them in the viewer. Every page sits behind a
//! bot-mitigation redirect only a browser passes. Archive Portals §4.13
//! specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::locality::{forms, matching_labels};
use super::markup;
use super::select::{Candidate, act_code, narrow};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, ActKind, CitationParts};
use crate::transport::{PortalFetch, PortalRequest};
use crate::{ArchiveTarget, ArchiveView, ResolveError};
use page::Row;

/// The Gers portal's adapter.
pub struct Archives32;

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("archives32 settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("archives32: {detail}"))
}

/// A body that is not an anti-bot page, which is reported as such and not
/// as a change of the portal's shape.
fn unchallenged(body: String) -> Result<String, ResolveError> {
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

/// How a module's viewer numbers a register's views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Views {
    /// Contiguous identifiers: view `n` is the first view's plus `n - 1`.
    Contiguous,
    /// Identifiers in no order: the viewer's list gives view `n`'s.
    Listed,
}

/// The names of a form's locality lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    /// The communes (or registration offices): `lieu`, `LIEU`.
    locality: String,
    /// The former communes, where the form has them: `ancienne`.
    #[serde(default)]
    former: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    path: String,
    fields: Fields,
    #[serde(default)]
    acts: BTreeMap<String, Vec<String>>,
    views: Views,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    /// The module's path: `/archives_numerisees/portail/etats_civils/ec`.
    path: String,
    fields: Fields,
    /// The checkboxes of each document kind, by code; none for a module
    /// whose form has no kinds (censuses, tables of successions).
    acts: BTreeMap<String, Vec<String>>,
    views: Views,
}

/// A form field's name: letters, digits and underscores.
fn is_field(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        if !is_https_origin(&raw.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        if !raw.path.starts_with('/')
            || raw.path.ends_with('/')
            || !raw.path[1..].split('/').all(is_field)
        {
            return Err(invalid(
                "path must be an absolute path without a trailing slash",
            ));
        }
        if !is_field(&raw.fields.locality) || !raw.fields.former.as_deref().is_none_or(is_field) {
            return Err(invalid("fields are the form's field names"));
        }
        for (code, boxes) in &raw.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not a document kind's code")));
            }
            if boxes.is_empty() || !boxes.iter().all(|name| is_field(name)) {
                return Err(invalid("acts are lists of the form's checkbox names"));
            }
        }
        let settings = Self {
            origin: raw.origin,
            transport: raw.transport,
            path: raw.path,
            fields: raw.fields,
            acts: raw.acts,
            views: raw.views,
        };
        if let Some(act) = collection
            .acts
            .iter()
            .find(|act| settings.boxes(act).is_none())
        {
            return Err(invalid(&format!("no checkbox for `{act}`")));
        }
        Ok(settings)
    }

    /// The checkboxes searching a document kind: its own code's, or for a
    /// combined act (`BMS`) each kind's, publications of banns as marriages
    /// where the form has no box of their own. None for a module without
    /// kinds.
    fn boxes(&self, act: &Act) -> Option<Vec<&str>> {
        if self.acts.is_empty() {
            return Some(Vec::new());
        }
        if let Some(boxes) = self.acts.get(&act.to_string()) {
            return Some(boxes.iter().map(String::as_str).collect());
        }
        let mut boxes = Vec::new();
        for kind in act.kinds() {
            let own = self.acts.get(&kind.letter().to_string());
            let filed = self
                .acts
                .get(&ActKind::Marriage.letter().to_string())
                .filter(|_| *kind == ActKind::Publication);
            for name in own.or(filed)? {
                if !boxes.contains(&name.as_str()) {
                    boxes.push(name.as_str());
                }
            }
        }
        (!boxes.is_empty()).then_some(boxes)
    }

    fn search_path(&self) -> String {
        format!("{}/recherche/", self.path)
    }

    fn search_page(&self) -> String {
        format!("{}{}", self.origin, self.search_path())
    }

    fn viewer(&self, query: &str) -> String {
        format!("{}/visu/?{query}", self.path)
    }
}

/// Which list of the form names the searched locality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    Commune,
    Former,
}

/// The form's body: the locality (or the former commune, every commune
/// chosen), no years, the kind's checkboxes.
fn body(settings: &Settings, label: &str, listed: Listed, boxes: &[&str]) -> String {
    let mut query = Query::new();
    let fields = &settings.fields;
    match (listed, &fields.former) {
        (Listed::Commune, former) => {
            query.push(fields.locality.as_str(), label);
            if let Some(former) = former {
                query.push(former.as_str(), "all");
            }
        }
        (Listed::Former, Some(former)) => {
            query
                .push(fields.locality.as_str(), "all")
                .push(former.as_str(), label);
        }
        (Listed::Former, None) => {
            query.push(fields.locality.as_str(), label);
        }
    }
    query.push("annee_d", "").push("annee_f", "");
    for name in boxes {
        query.push(*name, "on");
    }
    query.push("valider", "valider");
    query.to_string()
}

/// One search's answer: the form's lists, and the rows found.
async fn post(
    settings: &Settings,
    label: &str,
    listed: Listed,
    boxes: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<(String, Vec<Row>), ResolveError> {
    let request = PortalRequest::post(
        settings.search_path(),
        "application/x-www-form-urlencoded",
        body(settings, label, listed, boxes),
    );
    let answer = unchallenged(fetch.request(&request).await?)?;
    if page::options(&answer, &settings.fields.locality).is_empty() {
        return Err(markup::unreadable(
            &answer,
            "archives32: the answer has no locality list".to_owned(),
        ));
    }
    let rows = page::rows(&answer)?.ok_or_else(|| unexpected("the answer has no results"))?;
    Ok((answer, rows))
}

/// What a search found.
struct Found {
    rows: Vec<Row>,
    listed: Listed,
}

/// Searches the cited locality: first as cited, then, when the form's list
/// writes it otherwise, as the list writes it, a commune before a former
/// commune of the same name. `None` when no list names it.
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Option<Found>, ResolveError> {
    let boxes = settings
        .boxes(&citation.act)
        .ok_or(ResolveError::NoAdapter)?;
    let (answer, rows) = post(settings, &citation.locality, Listed::Commune, &boxes, fetch).await?;
    let communes = page::options(&answer, &settings.fields.locality);
    if communes.contains(&citation.locality) {
        return Ok(Some(Found {
            rows,
            listed: Listed::Commune,
        }));
    }
    let formers = settings
        .fields
        .former
        .as_deref()
        .map(|former| page::options(&answer, former))
        .unwrap_or_default();
    let label = matching_labels(&communes, localities)
        .first()
        .map(|label| ((*label).to_owned(), Listed::Commune))
        .or_else(|| {
            matching_labels(&formers, localities)
                .first()
                .map(|label| ((*label).to_owned(), Listed::Former))
        });
    let Some((label, listed)) = label else {
        return Ok(None);
    };
    let (_, rows) = post(settings, &label, listed, &boxes, fetch).await?;
    Ok(Some(Found { rows, listed }))
}

/// A row as selection reads it: the locality the search named, the
/// parish, and the acts of a register (a table or a series module lists
/// only its own kind).
fn candidate(row: &Row, listed: Listed, act: &Act) -> Option<Candidate<String>> {
    let locality = match listed {
        Listed::Commune => row.locality.clone(),
        Listed::Former => row.former.clone(),
    };
    Some(Candidate {
        locality,
        call_number: row.call_number.clone(),
        act: matches!(act, Act::Register(_))
            .then(|| row.acts.as_deref().and_then(|acts| act_code(acts, false)))
            .flatten(),
        parish: row.parish.clone(),
        period: row.period.clone(),
        images: row.images,
        numbers: None,
        payload: row.viewer.clone()?,
    })
}

/// The `View` target of a chosen register: its viewer at each cited view's
/// identifier, computed for contiguous views, read from the viewer's list
/// otherwise — one request, and only when the citation names a view.
async fn view(
    settings: &Settings,
    archive: &Archive,
    citation: &CitationParts,
    chosen: &Candidate<String>,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveTarget, ResolveError> {
    let query = &chosen.payload;
    let first = page::viewer_first(query).ok_or_else(|| unexpected("a viewer link"))?;
    let register = format!("{}{}", settings.origin, settings.viewer(query));
    let mut count = chosen.images.map_or(0, usize::from);
    let ids: Vec<u64> = match settings.views {
        _ if citation.views.is_empty() => Vec::new(),
        Views::Contiguous => (first..).take(count).collect(),
        Views::Listed => {
            let viewer = unchallenged(fetch.get(&settings.viewer(query)).await?)?;
            let ids = page::views(&viewer)?;
            count = ids.len();
            ids
        }
    };
    let views = cited_views(citation, count, chosen.period.as_deref())
        .iter()
        .filter_map(|cited| {
            let id = ids.get(usize::from(cited.view).checked_sub(1)?)?;
            Some(ArchiveView {
                view: cited.view,
                url: format!(
                    "{}{}",
                    settings.origin,
                    settings.viewer(&page::viewer_at(query, *id))
                ),
                ark: None,
                image: None,
            })
        })
        .collect();
    Ok(view_target(
        archive,
        citation,
        chosen.call_number.as_deref(),
        count,
        register,
        views,
    ))
}

impl Platform for Archives32 {
    fn id(&self) -> &'static str {
        "archives32"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        // The site's robots.txt: a page of the origin behind the same
        // bot-mitigation redirect, the lightest there is.
        Some(PortalEndpoint {
            start: format!("{}/robots.txt", settings.origin),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The search is a `POST`: its form is where a reader starts it.
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
    let candidates: Vec<Candidate<String>> = search(&settings, citation, &localities, fetch)
        .await?
        .map(|found| {
            found
                .rows
                .iter()
                .filter_map(|row| candidate(row, found.listed, &citation.act))
                .collect()
        })
        .unwrap_or_default();
    let matches = match narrow(&candidates, citation, &localities).as_slice() {
        [only] => return view(&settings, archive, citation, only, fetch).await,
        many => many.len(),
    };
    Ok(ArchiveTarget::Results {
        url: settings.search_page(),
        matches: Some(matches),
    })
}
