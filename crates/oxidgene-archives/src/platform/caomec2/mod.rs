//! CAOMEC2, the civil-status search of the Archives nationales d'outre-mer
//! (`/caomec2/`): the registers of the overseas territories, one commune and
//! one year a row, each opening an OpenSeadragon viewer of Deep Zoom images
//! (`osd.php`).
//!
//! A territory's search form (`recherche.php?territoire=<T>`) lists its
//! communes; a search (`resultats.php`, a `GET`) takes the territory, the
//! commune as listed, and the year, and lists a row per register: a kind of
//! act (`Naissance`, `Mariage`, `Décès`) or all of them (`Tous actes`). The
//! viewer has no address per view — it ignores every parameter naming one —,
//! so a register opens on its first view, and the reader goes to the cited
//! one. The portal answers over plain `http` only, the catalogue's named
//! exception (Archive Portals §3.1). Archive Portals §4.15 specifies the
//! requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use oxidgene_core::search::{Separator, fold_text};
use serde::Deserialize;

use super::locality::{forms, matching_labels};
use super::markup::{self, letters};
use super::select::{Candidate, holds_act, narrow};
use super::view::view_target;
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_portal_origin, refuse_series};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveTarget, ResolveError};
use page::Row;

/// The CAOMEC2 adapter.
pub struct Caomec2;

/// The application's path on the portal.
const BASE: &str = "/caomec2";

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("caomec2 settings: {message}"))
}

async fn get(fetch: &dyn PortalFetch, path: &str) -> Result<String, ResolveError> {
    let body = fetch.get(path).await?;
    if markup::is_challenge(&body) {
        Err(ResolveError::Challenged)
    } else {
        Ok(body)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    origin: String,
    #[serde(default)]
    transport: Access,
    territory: String,
    #[serde(default)]
    code: Option<String>,
}

/// The settings of one collection, checked.
#[derive(Debug, Clone)]
struct Settings {
    origin: String,
    transport: Access,
    insecure_http: bool,
    /// The territory as the search names it: `GUYANE`, `REUNION`.
    territory: String,
    /// The archive's citation code naming this territory alone
    /// (`ANOM973`), if it has one.
    code: Option<String>,
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let raw =
            Raw::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        if !is_portal_origin(collection, &raw.origin) {
            return Err(invalid(
                "origin must be an https origin, or http for a collection marked insecure_http",
            ));
        }
        let territory_ok = !raw.territory.trim().is_empty()
            && raw
                .territory
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b' ');
        if !territory_ok {
            return Err(invalid("territory is the search's value, in capitals"));
        }
        refuse_series("caomec2", collection)?;
        if let Some(act) = collection
            .acts
            .iter()
            .find(|act| !matches!(act, Act::Register(_)))
        {
            return Err(invalid(&format!(
                "the search's rows are registers of acts, not `{act}`"
            )));
        }
        Ok(Self {
            origin: raw.origin,
            transport: raw.transport,
            insecure_http: collection.insecure_http,
            territory: raw.territory,
            code: raw.code,
        })
    }

    /// The territory's search form, which lists its communes.
    fn form_path(&self) -> String {
        let mut query = Query::new();
        query.push("territoire", self.territory.as_str());
        format!("{BASE}/recherche.php?{query}")
    }

    /// The search of a commune, as listed, and a year, every kind of act.
    fn results_path(&self, commune: &str, year: Option<u16>) -> String {
        let mut query = Query::new();
        query
            .push("territoire", self.territory.as_str())
            .push("commune", commune)
            .push("typeacte", "")
            .push("theme", "")
            .push(
                "annee",
                year.map(|year| year.to_string()).unwrap_or_default(),
            )
            .push("debut", "")
            .push("fin", "")
            .push("vue", "");
        format!("{BASE}/resultats.php?{query}")
    }

    /// The viewer of a row's register.
    fn viewer_path(&self, row: &Row) -> String {
        let mut query = Query::new();
        query
            .push("territoire", self.territory.as_str())
            .push("commune", row.commune.as_str())
            .push("annee", row.year.as_str());
        if let Some(typeacte) = &row.typeacte {
            query.push("typeacte", typeacte.as_str());
        }
        format!("{BASE}/osd.php?{query}")
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }
}

/// The citation's locality as the portal writes its communes: capitals,
/// without accents, punctuation kept (`L'ETANG-EXEMPLE`).
fn portal_form(locality: &str) -> String {
    fold_text(locality, |_| Separator::Keep).to_uppercase()
}

/// The act codes a row's label holds, `None` for a register of every act
/// (`Tous actes`), and nothing for a kind no citation of a register names
/// (manumissions, recognitions, judgements, stillbirths, tables). A
/// baptism is filed as a birth and a burial as a death. The labels' accents
/// may have been lost to the portal's encoding (`D\u{fffd}c\u{fffd}s`).
fn held(label: &str) -> Option<Option<&'static str>> {
    let named = |word: &str| markup::is_named(label, &[letters(word)]);
    if named("Tous actes") {
        Some(None)
    } else if named("Naissance") {
        Some(Some("BN"))
    } else if named("Mariage") {
        Some(Some("M"))
    } else if named("Décès") {
        Some(Some("DS"))
    } else {
        None
    }
}

/// Whether a territorial citation code names another territory than the
/// collection's: then the collection is not searched.
fn names_another_territory(archive: &Archive, settings: &Settings, code: &str) -> bool {
    settings.code.as_deref() != Some(code)
        && archive
            .collections
            .iter()
            .filter(|collection| collection.platform == "caomec2")
            .filter_map(|collection| Settings::read(collection).ok()?.code)
            .any(|territorial| territorial == code)
}

/// What a search found: the commune as the form lists it, the registers of
/// its rows that may hold a cited act, and the search's page.
struct Found {
    commune: String,
    candidates: Vec<Candidate<String>>,
    results: String,
}

/// The territory's form, then the search of the commune the citation names
/// as the form lists it. `None` when the form lists no such commune.
async fn search(
    settings: &Settings,
    citation: &CitationParts,
    fetch: &dyn PortalFetch,
) -> Result<Option<Found>, ResolveError> {
    let form = get(fetch, &settings.form_path()).await?;
    let communes = page::communes(&form)?;
    let wanted = forms(&citation.locality);
    let localities: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let Some(commune) = matching_labels(&communes, &localities).first().copied() else {
        return Ok(None);
    };
    let results = settings.results_path(commune, citation.year);
    let answer = get(fetch, &results).await?;
    let candidates = page::results(&answer)?
        .iter()
        .filter_map(|row| {
            let act = held(&row.act)?;
            Some(Candidate {
                locality: Some(row.commune.clone()),
                call_number: None,
                act: act.map(str::to_owned),
                parish: None,
                period: Some(row.year.clone()),
                images: None,
                numbers: None,
                payload: settings.viewer_path(row),
            })
        })
        .collect();
    Ok(Some(Found {
        commune: commune.to_owned(),
        candidates,
        results,
    }))
}

impl Platform for Caomec2 {
    fn id(&self) -> &'static str {
        "caomec2"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        // The territory's search form: a light page, and the first request.
        Some(PortalEndpoint {
            start: settings.url(&settings.form_path()),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
            insecure_http: settings.insecure_http,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        if citation.locality.trim().is_empty() {
            return Some(settings.url(&settings.form_path()));
        }
        // The commune as the portal writes it; its search ignores case.
        let commune = portal_form(&citation.locality);
        Some(settings.url(&settings.results_path(&commune, citation.year)))
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
    let form = settings.url(&settings.form_path());
    // A code naming another territory: not this collection's register, and
    // no request sent; the offline target, which another collection's
    // search outranks (§5.2).
    if names_another_territory(archive, &settings, &citation.code) {
        return Ok(ArchiveTarget::Results {
            url: form,
            matches: None,
        });
    }
    let Some(found) = search(&settings, citation, fetch).await? else {
        return Ok(ArchiveTarget::Results {
            url: form,
            matches: Some(0),
        });
    };
    // Every row is of the commune searched, as the form lists it.
    let mut chosen = narrow(&found.candidates, citation, &[found.commune.as_str()]);
    // Narrowing skips a criterion that would leave no register; a register
    // of another kind than the cited one is never it.
    chosen.retain(|candidate| holds_act(candidate.act.as_deref(), &citation.act));
    let results = settings.url(&found.results);
    match chosen.as_slice() {
        // The register: its viewer opens on the first view, which has no
        // address of its own; its size is the viewer's to show.
        [only] => Ok(view_target(
            archive,
            citation,
            None,
            usize::MAX,
            settings.url(&only.payload),
            Vec::new(),
        )),
        [] => Ok(ArchiveTarget::Results {
            url: results,
            matches: Some(0),
        }),
        many => Ok(ArchiveTarget::Results {
            url: results,
            matches: Some(many.len()),
        }),
    }
}
