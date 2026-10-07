//! Prismia Vision (EidoPolis), a single-page application over a JSON API on
//! another origin.
//!
//! The portal's pages call `POST <api>/presentation/v1/Query`, whose results
//! are IIIF Presentation 3 manifest stubs (one per register: call number,
//! image count, dates, parish), after a facet request that lists the
//! localities. Both carry the portal's public key in an `ApiKey` header; the
//! key is the one `/runtimeConfig.js` publishes, read at resolution time so
//! that a rotation does not break the adapter. The portal's viewer opens a
//! register at `/viewer/<manifest address>/<canvas>` with the canvas number
//! as the view number, so the target needs no further request. Archive
//! Portals §4.7 specifies the requests.

#[cfg(any(test, feature = "live"))]
mod live;
mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::locality::{forms, name_start};
use super::query::encode;
use super::select::{Selection, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection};
use crate::citation::{Act, CitationParts};
use crate::transport::{PortalFetch, PortalRequest, origin_of};
use crate::{ArchiveTarget, ArchiveView, ResolveError};

/// The Prismia Vision adapter.
pub struct Prismia;

/// Where the portal publishes its configuration, key included.
const CONFIG_PATH: &str = "/runtimeConfig.js";

/// The registers one search asks for.
const RESULT_SIZE: usize = 50;

/// The localities one lookup asks for.
const LOCALITY_SIZE: usize = 100;

/// The longest locality prefix a lookup sends.
const PREFIX_LENGTH: usize = 30;

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    /// The API's address, `https://<host>/api`.
    api: String,
    /// The portal's search page, as a path.
    search_path: String,
    /// The instrument's `searchBarPathOrId`: the collections searched.
    paths: Vec<String>,
    filters: Filters,
    /// The act filter value of each act code, as the portal writes it.
    acts: BTreeMap<String, String>,
}

/// The keys of the two filters of the instrument.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Filters {
    locality: String,
    act: String,
}

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    /// The register's manifest address, which the portal's viewer takes.
    manifest: String,
}

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("prismia settings: {message}"))
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("prismia: {detail}"))
}

fn is_text(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_control)
}

impl Settings {
    fn read(collection: &Collection) -> Result<Self, CatalogError> {
        let settings =
            Self::deserialize(&collection.portal).map_err(|error| invalid(&error.to_string()))?;
        settings.check()?;
        settings.check_acts(collection)?;
        Ok(settings)
    }

    fn check(&self) -> Result<(), CatalogError> {
        if !is_https_origin(&self.origin) {
            return Err(invalid("origin must be an https origin"));
        }
        let api_origin = origin_of(&self.api).filter(|origin| is_https_origin(origin));
        let path_ok = api_origin
            .and_then(|origin| self.api.strip_prefix(origin))
            .is_some_and(|path| {
                path.starts_with('/')
                    && !path.ends_with('/')
                    && !path.contains(['?', '#', ' ', '\\'])
            });
        if !path_ok {
            return Err(invalid("api must be an https address with a path"));
        }
        if !self.search_path.starts_with('/') || self.search_path.contains(['#', ' ']) {
            return Err(invalid("search_path must be an absolute path"));
        }
        if self.paths.is_empty() || !self.paths.iter().all(|path| is_text(path)) {
            return Err(invalid("paths must be a list of collection paths"));
        }
        if !is_text(&self.filters.locality) || !is_text(&self.filters.act) {
            return Err(invalid("filters must name the locality and act keys"));
        }
        Ok(())
    }

    /// Every act code is valid and has a value, and every act the collection
    /// holds has one.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, value) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if !is_text(value) {
                return Err(invalid(&format!("the value of `{code}` is empty")));
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

    /// The act's own value, or for a combined act (`BMS`) its first kind's.
    fn act_value(&self, act: &Act) -> Option<&str> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.primary_kind()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(String::as_str)
    }

    /// The API's origin, which the endpoint declares.
    fn api_origin(&self) -> Option<&str> {
        origin_of(&self.api)
    }

    fn search_page(&self) -> String {
        format!("{}{}", self.origin, self.search_path)
    }

    /// The portal's viewer on a manifest, opened on canvas `view`
    /// (one-based) when given.
    fn view_url(&self, manifest: &str, view: Option<u16>) -> String {
        let mut url = format!("{}/viewer/{}", self.origin, encode(manifest));
        if let Some(view) = view {
            url.push_str(&format!("/{view}"));
        }
        url
    }

    /// A `POST` of a JSON body to the API, with the portal's key.
    fn api_request(&self, path: &str, key: &str, body: &serde_json::Value) -> PortalRequest {
        PortalRequest::post(
            format!("{}{path}", self.api),
            "application/json",
            body.to_string(),
        )
        .header("ApiKey", key)
    }

    /// The locality lookup: the facet values of the commune key that start
    /// like the cited name.
    fn locality_request(&self, key: &str, citation: &CitationParts) -> PortalRequest {
        self.api_request(
            "/presentation/v1/facet/getFacetValues",
            key,
            &json!({
                "prismPathOrId": self.paths,
                "aggregateTag": "Lieux",
                "aggregateValue": [self.filters.locality],
                "text": name_start(&citation.locality, PREFIX_LENGTH),
                "size": LOCALITY_SIZE,
            }),
        )
    }

    /// The search of one locality value, filtered by the act and, when the
    /// citation has one, the year.
    fn search_request(&self, key: &str, locality: &str, citation: &CitationParts) -> PortalRequest {
        let mut filters = vec![json!({
            "keys": [self.filters.locality],
            "values": [locality],
        })];
        if let Some(act) = self.act_value(&citation.act) {
            filters.push(json!({ "keys": [self.filters.act], "values": [act] }));
        }
        let mut body = json!({
            "searchBarPathOrId": self.paths,
            "tagSelectedFilters": filters,
            "target": ["document"],
            "images": true,
            "from": 0,
            "size": RESULT_SIZE,
            "responseBy": "Date",
        });
        if let (Some(year), Some(body)) = (citation.year, body.as_object_mut()) {
            body.insert(
                "periodeDeb".to_owned(),
                json!(format!("{year}-01-01T00:00:00.000Z")),
            );
            body.insert(
                "periodeFin".to_owned(),
                json!(format!("{year}-12-31T00:00:00.000Z")),
            );
            body.insert("periodeRange".to_owned(), json!("between"));
            body.insert("periodeFieldType".to_owned(), json!("year"));
        }
        self.api_request("/presentation/v1/Query", key, &body)
    }
}

impl Platform for Prismia {
    fn id(&self) -> &'static str {
        "prismia"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        // The observed instrument is the civil status; a series' filters
        // are unknown.
        super::refuse_series(self.id(), collection)?;
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: settings.search_page(),
            other_origins: vec![settings.api_origin()?.to_owned()],
            origin: settings.origin,
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, _citation: &CitationParts) -> Option<String> {
        // The portal's filters live in the application's state, not in its
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
    let results = |matches| ArchiveTarget::Results {
        url: settings.search_page(),
        matches: Some(matches),
    };

    let key = page::api_key(&fetch.get(CONFIG_PATH).await?)?;
    let facets = fetch
        .request(&settings.locality_request(&key, citation))
        .await?;
    let wanted = forms(&citation.locality);
    let Some(locality) = page::locality(&facets, &wanted)? else {
        return Ok(results(0));
    };

    let answer = fetch
        .request(&settings.search_request(&key, &locality, citation))
        .await?;
    let (rows, total) = page::registers(&answer, &settings.api, &locality)?;
    let row = match select(&rows, citation, &[&locality]) {
        Selection::One(row) => row,
        // A search that matched more registers than it returned is wider
        // than the rows selection saw.
        Selection::Many(matches) => {
            return Ok(results(if total > rows.len() { total } else { matches }));
        }
    };

    let manifest = &row.payload.manifest;
    let count = row.images.map_or(usize::MAX, usize::from);
    let views = cited_views(citation, count, row.period.as_deref())
        .iter()
        .map(|cited| ArchiveView {
            view: cited.view,
            url: settings.view_url(manifest, Some(cited.view)),
            ark: None,
            image: None,
        })
        .collect();
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        count,
        settings.view_url(manifest, None),
        views,
    ))
}
