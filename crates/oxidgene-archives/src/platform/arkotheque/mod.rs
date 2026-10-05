//! Arkothèque (1 égal 2), the publishing software of many French
//! departmental archives.
//!
//! Each collection is searched by one engine of the portal's request
//! interface, `/_recherche-api/moteur`, filtered by the locality, the act
//! category and, where the engine has the filter, the year. The answer's
//! result rows are selected down to one register (`select`), whose viewer
//! endpoint lists its images; the target is the portal's record page opened
//! on the cited image, `<search_path>?detail=<record>#<viewer address>/<i>`
//! with `i` zero-based. Archive Portals §4.3 specifies the requests.

mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::iiif::image_info;
use super::select::{Selection, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::{Act, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};

/// The rows one search asks for: the largest page the engines accept (25,
/// 50 or 100).
const RESULT_SIZE: &str = "100";

/// The search interface every Arkothèque portal exposes.
const SEARCH_PATH: &str = "/_recherche-api/moteur";

/// The Arkothèque adapter.
pub struct Arkotheque;

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    /// The collection's search page, where records and views open.
    search_path: String,
    /// The engine's unique reference, `arko_default_…`.
    engine: String,
    /// The search component's content identifiers.
    content_ids: Vec<String>,
    /// The list display mode, whose rows carry the cells read below.
    display_mode: String,
    fields: Fields,
    /// The act filter value of each act code, with its record key:
    /// `Baptêmes[[arko_fiche_…]]`. The engines match nothing without the key.
    acts: BTreeMap<String, String>,
    #[serde(default)]
    locality_style: LocalityStyle,
    cells: Cells,
}

/// The engine's filter references.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    locality: String,
    act: String,
    /// Absent from engines without a period filter.
    #[serde(default)]
    period: Option<String>,
}

/// The `data-champ` names of the result row cells selection reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cells {
    locality: String,
    #[serde(default)]
    parish: Option<String>,
    #[serde(default)]
    act: Option<String>,
    #[serde(default)]
    period: Option<String>,
}

/// How the portal writes a locality's leading article.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LocalityStyle {
    /// `Le Mans`.
    #[default]
    Plain,
    /// `Mans (Le)`.
    ArticleSuffix,
}

/// The articles `article_suffix` moves behind the name.
const ARTICLES: [&str; 5] = ["Les ", "Le ", "La ", "L'", "L’"];

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("arkotheque settings: {message}"))
}

/// A reference of the request interface: letters, digits and `_`.
fn is_reference(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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
        if !self.search_path.starts_with('/') || self.search_path.contains(['?', '#', ' ']) {
            return Err(invalid("search_path must be an absolute path"));
        }
        let references = [
            &self.engine,
            &self.display_mode,
            &self.fields.locality,
            &self.fields.act,
        ];
        if !references
            .into_iter()
            .all(|reference| is_reference(reference))
            || !self.fields.period.as_deref().is_none_or(is_reference)
        {
            return Err(invalid("engine, display mode and filter references"));
        }
        if self.content_ids.is_empty()
            || !self
                .content_ids
                .iter()
                .all(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(invalid("content_ids must be numeric identifiers"));
        }
        let cells = [&self.cells.parish, &self.cells.act, &self.cells.period];
        if !is_reference(&self.cells.locality)
            || !cells.into_iter().flatten().all(|cell| is_reference(cell))
        {
            return Err(invalid("cell names"));
        }
        Ok(())
    }

    /// Every act code is valid and has a filter value with its record key,
    /// and every act the collection holds has one.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, value) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if !(value.contains("[[arko_fiche_") && value.ends_with("]]")) {
                return Err(invalid(&format!(
                    "the filter value of `{code}` lacks its record key"
                )));
            }
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_value(act).is_none())
        {
            Some(act) => Err(invalid(&format!("no filter value for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The act filter value: the act's own code, or for a combined act
    /// (`BMS`) its first kind, whose category holds the mixed registers.
    fn act_value(&self, act: &Act) -> Option<&str> {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.kinds().first()?;
                self.acts.get(&first.letter().to_string())
            })
            .map(String::as_str)
    }

    /// The locality as the portal writes it.
    fn locality(&self, citation: &CitationParts) -> String {
        let locality = citation.locality.as_str();
        if self.locality_style == LocalityStyle::ArticleSuffix {
            for article in ARTICLES {
                if let Some(name) = locality.strip_prefix(article)
                    && !name.is_empty()
                {
                    return format!("{name} ({})", article.trim_end());
                }
            }
        }
        locality.to_owned()
    }

    /// The filters of a search, shared by the request and the search page:
    /// the locality, the act category and, where the engine has the filter,
    /// the year. Nothing else of the citation leaves the application.
    fn filters(&self, citation: &CitationParts) -> Query {
        let engine = &self.engine;
        let mut query = Query::new();
        query
            .push(format!("{engine}--ficheFocus"), "")
            .push(format!("{engine}--filtreGroupes[mode]"), "simple")
            .push(format!("{engine}--filtreGroupes[op]"), "AND");
        let mut filter = |field: &str, value: String, mode: &str| {
            let prefix = format!("{engine}--filtreGroupes[groupes][0][{field}]");
            query
                .push(format!("{prefix}[op]"), "AND")
                .push(format!("{prefix}[q][]"), value)
                .push(format!("{prefix}[extras][mode]"), mode);
        };
        filter(&self.fields.locality, self.locality(citation), "popup");
        if let Some(act) = self.act_value(&citation.act) {
            filter(&self.fields.act, act.to_owned(), "select");
        }
        if let (Some(field), Some(year)) = (&self.fields.period, citation.year) {
            filter(field, format!("{year}|{year}"), "slider");
        }
        query
            .push(format!("{engine}--from"), "0")
            .push(format!("{engine}--resultSize"), RESULT_SIZE);
        for id in &self.content_ids {
            query.push(format!("{engine}--contenuIds[]"), id.as_str());
        }
        query.push(format!("{engine}--modeRestit"), self.display_mode.as_str());
        query
    }

    fn search_request(&self, filters: &Query) -> String {
        let mut query = Query::new();
        query.push("refUnique", self.engine.as_str());
        format!("{SEARCH_PATH}?{query}&{filters}")
    }

    fn search_page(&self, filters: &Query) -> String {
        format!("{}{}?{filters}", self.origin, self.search_path)
    }

    /// The record page opened on image `index`, zero-based.
    fn view_url(&self, record: &str, viewer: &str, index: u16) -> String {
        format!(
            "{}{}?detail={}#{viewer}/{index}",
            self.origin,
            self.search_path,
            super::query::encode(record)
        )
    }
}

impl Platform for Arkotheque {
    fn id(&self) -> &'static str {
        "arkotheque"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}{}", settings.origin, settings.search_path),
            origin: settings.origin,
            other_origins: Vec::new(),
            access: settings.transport,
        })
    }

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        Some(settings.search_page(&settings.filters(citation)))
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
    let filters = settings.filters(citation);
    let results = |matches| ArchiveTarget::Results {
        url: settings.search_page(&filters),
        matches: Some(matches),
    };

    let answer = fetch.get(&settings.search_request(&filters)).await?;
    let rows = page::search_rows(&answer, &settings.cells)?;
    let styled = settings.locality(citation);
    let row = match select(&rows, citation, &[&citation.locality, &styled]) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    let record = &row.payload.record;
    // A register listed without images cannot be opened on a view.
    let Some(viewer) = row.payload.viewer.as_deref() else {
        return Ok(results(1));
    };
    let sources = page::viewer_sources(&fetch.get(viewer).await?)?;

    let cited = cited_views(citation, sources.len());
    let mut views = Vec::with_capacity(cited.len());
    for view in cited {
        let source = &sources[usize::from(view.view) - 1];
        let image = match archive.display {
            Display::Iiif => Some(image(&settings, source, fetch).await?),
            Display::Portal => None,
        };
        views.push(ArchiveView {
            view: view.view,
            url: settings.view_url(record, viewer, view.view - 1),
            ark: source
                .ark
                .as_ref()
                .map(|ark| format!("{}{ark}", settings.origin)),
            image,
        });
    }
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        sources.len(),
        settings.view_url(record, viewer, 0),
        views,
    ))
}

/// The image of one view, for a `display: "iiif"` archive: its size from the
/// service's `info.json`, and addresses built on the portal's own image path
/// rather than on the service's `@id`, which names an internal host.
async fn image(
    settings: &Settings,
    source: &page::Source,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveImage, ResolveError> {
    let info = image_info(&fetch.get(&format!("{}/info.json", source.src)).await?)?;
    Ok(info.image(&format!("{}{}", settings.origin, source.src)))
}
