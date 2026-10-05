//! Ligeo Diffusion (Boscop), the publishing software of many French
//! departmental archives.
//!
//! Each collection is one search of the portal ("recherche"), named in the
//! path and placed in the menu by a node number. Its form is a plain `GET`
//! whose results are server-rendered HTML, a table whose columns differ from
//! portal to portal and are read by their header text. The cited register is
//! selected among the rows (`select`); its viewer address is
//! `/ark:/<naan>/<id>/<tag>/<group>/<view>`, and for a `display: "iiif"`
//! archive its IIIF Presentation 2 manifest gives the image count and sizes.
//! Archive Portals §4.5 specifies the requests.

mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::iiif::{PICTURE_BOUND, THUMBNAIL_MIN_WIDTH};
use super::select::{Selection, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::{Act, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};

/// The Ligeo adapter.
pub struct Ligeo;

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    /// `/archive`, or `/archives` on some portals.
    #[serde(default = "default_prefix")]
    prefix: String,
    /// The search's name in the path, also sent as `type`: `etatcivil`,
    /// `paroissiaux`, `etatcivil2`.
    search: String,
    /// The menu node, `n:<node>` in the path.
    node: u32,
    fields: Fields,
    /// The act filter of each act code; none when the form has no act filter.
    #[serde(default)]
    acts: BTreeMap<String, ActFilter>,
    columns: Columns,
}

fn default_prefix() -> String {
    "/archive".to_owned()
}

/// The names of the form inputs the adapter fills.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    locality: String,
    /// The act input, for acts written as one value (`RECH_acte[]=N`).
    #[serde(default)]
    act: Option<String>,
    /// The years' inputs, both or neither.
    #[serde(default)]
    year_from: Option<String>,
    #[serde(default)]
    year_to: Option<String>,
}

/// How the form expresses an act.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum ActFilter {
    /// The value of the `fields.act` input. An input named `…[]` is a
    /// checkbox list: a combined act repeats it once per kind.
    Value(String),
    /// Inputs of their own, with their values: a document type and an act
    /// glob (`RECH_doc=EC&RECH_acte2=*aissanc*`).
    Params(BTreeMap<String, String>),
}

/// The header text of the result columns, read as written by the portal,
/// case, accents and punctuation ignored. A table either has a `locality`
/// column, or only a `title` column, from which the locality, parish, acts
/// and call number are read.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Columns {
    #[serde(default)]
    locality: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    acts: Option<String>,
    #[serde(default)]
    parish: Option<String>,
    #[serde(default)]
    period: Option<String>,
    #[serde(default)]
    call_number: Option<String>,
}

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("ligeo settings: {message}"))
}

/// An input or search name: letters, digits and `_ - [ ]`.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-[]".contains(&byte))
}

/// A form value: no separator or control character a query could not carry
/// once encoded as a single value.
fn is_value(text: &str) -> bool {
    !text.trim().is_empty() && !text.chars().any(char::is_control)
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
        if !self.prefix.starts_with('/')
            || self.prefix.len() < 2
            || !self.prefix[1..]
                .bytes()
                .all(|byte| byte.is_ascii_lowercase())
        {
            return Err(invalid("prefix must be a path such as /archive"));
        }
        if !is_name(&self.search) || self.search.contains(['[', ']', '-']) || self.node == 0 {
            return Err(invalid("search and node"));
        }
        let fields = &self.fields;
        let names = [
            Some(&fields.locality),
            fields.act.as_ref(),
            fields.year_from.as_ref(),
            fields.year_to.as_ref(),
        ];
        if !names.into_iter().flatten().all(|name| is_name(name)) {
            return Err(invalid("input names"));
        }
        if fields.year_from.is_some() != fields.year_to.is_some() {
            return Err(invalid("year_from and year_to go together"));
        }
        self.columns.check()
    }

    /// Every act code is valid and has a usable filter, and every act the
    /// collection holds has one, unless the form has no act filter.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, filter) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            let usable = match filter {
                ActFilter::Value(value) => self.fields.act.is_some() && is_value(value),
                ActFilter::Params(params) => {
                    !params.is_empty()
                        && params
                            .iter()
                            .all(|(name, value)| is_name(name) && is_value(value))
                }
            };
            if !usable {
                return Err(invalid(&format!(
                    "the filter of `{code}` needs `fields.act` and a value, or inputs of its own"
                )));
            }
        }
        if self.acts.is_empty() {
            return match self.fields.act {
                Some(_) => Err(invalid("fields.act without acts")),
                None => Ok(()),
            };
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_filters(act).is_empty())
        {
            Some(act) => Err(invalid(&format!("no filter for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The filters searching `act`: its own entry, or one per kind of a
    /// combined act.
    fn act_filters(&self, act: &Act) -> Vec<&ActFilter> {
        if let Some(filter) = self.acts.get(&act.to_string()) {
            return vec![filter];
        }
        act.kinds()
            .iter()
            .filter_map(|kind| self.acts.get(&kind.letter().to_string()))
            .collect()
    }

    /// The filters of a search, shared by the request and the results page:
    /// the locality, the act and the year. Nothing else of the citation
    /// leaves the application. The locality is the portal's text match, and
    /// the year is the portal's interval test: both are re-checked on the
    /// rows.
    fn filters(&self, citation: &CitationParts) -> Query {
        let mut query = Query::new();
        query.push(&self.fields.locality, citation.locality.as_str());
        for filter in self.act_filters(&citation.act) {
            match filter {
                ActFilter::Value(value) => {
                    if let Some(name) = &self.fields.act {
                        query.push(name, value.as_str());
                        // A single-valued input holds one kind.
                        if !name.ends_with("[]") {
                            break;
                        }
                    }
                }
                ActFilter::Params(params) => {
                    for (name, value) in params {
                        query.push(name, value.as_str());
                    }
                    break;
                }
            }
        }
        if let (Some(from), Some(to), Some(year)) =
            (&self.fields.year_from, &self.fields.year_to, citation.year)
        {
            query
                .push(from, year.to_string())
                .push(to, year.to_string());
        }
        query.push("type", self.search.as_str());
        query
    }

    fn results_path(&self, filters: &Query) -> String {
        format!(
            "{}/resultats/{}/n:{}?{filters}",
            self.prefix, self.search, self.node
        )
    }

    fn search_page(&self) -> String {
        format!(
            "{}{}/recherche/{}/n:{}",
            self.origin, self.prefix, self.search, self.node
        )
    }

    /// `<ark>/<tag>/<group>/<view>`, view one-based, on the portal's origin.
    fn view_url(&self, register: &page::Register, view: u16) -> String {
        format!("{}{}/{view}", self.origin, register.viewer())
    }

    /// An image on the portal's own origin rather than on the host the
    /// manifest declares.
    fn on_origin(&self, address: &str, prefix: &str) -> Option<String> {
        let path = page::path_of(address)?;
        path.starts_with(prefix)
            .then(|| format!("{}{path}", self.origin))
    }
}

impl Columns {
    fn check(&self) -> Result<(), CatalogError> {
        if self.locality.is_some() == self.title.is_some() {
            return Err(invalid("columns need a locality or a title column"));
        }
        let headers = [
            &self.locality,
            &self.title,
            &self.acts,
            &self.parish,
            &self.period,
            &self.call_number,
        ];
        if headers
            .into_iter()
            .flatten()
            .any(|header| crate::platform::markup::fold(header).is_empty())
        {
            return Err(invalid("column headers must not be blank"));
        }
        Ok(())
    }
}

impl Platform for Ligeo {
    fn id(&self) -> &'static str {
        "ligeo"
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

    fn results_url(&self, collection: &Collection, citation: &CitationParts) -> Option<String> {
        let settings = Settings::read(collection).ok()?;
        let path = settings.results_path(&settings.filters(citation));
        Some(format!("{}{path}", settings.origin))
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
    let path = settings.results_path(&settings.filters(citation));
    let results = |matches| ArchiveTarget::Results {
        url: format!("{}{path}", settings.origin),
        matches: Some(matches),
    };

    let found = page::results(&fetch.get(&path).await?, &settings.columns)?;
    // More answers than rows: the portal paginates, and the cited register
    // may be on a page not read.
    if found.total.is_some_and(|total| total > found.rows.len()) {
        return Ok(results(found.total.unwrap_or_default()));
    }
    let row = match choose(&found.rows, citation) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    // A register listed without a viewer link cannot be opened on a view.
    let Some(register) = &row.payload else {
        return Ok(results(1));
    };

    let mut canvases = Vec::new();
    let image_count = match archive.display {
        Display::Iiif => {
            canvases = page::manifest(&fetch.get(&register.manifest()).await?)?;
            canvases.len()
        }
        // Without a manifest the count is the row's; a row that shows none
        // leaves the portal's viewer to bound the view.
        Display::Portal => row.images.map_or(usize::MAX, usize::from),
    };

    let cited = cited_views(citation, image_count);
    let mut views = Vec::with_capacity(cited.len());
    for view in cited {
        let canvas = canvases.get(usize::from(view.view) - 1);
        views.push(ArchiveView {
            view: view.view,
            url: settings.view_url(register, view.view),
            ark: canvas
                .and_then(|canvas| canvas.ark.as_deref())
                .and_then(|ark| settings.on_origin(ark, "/ark:/")),
            image: canvas.map(|canvas| image(&settings, canvas)).transpose()?,
        });
    }
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        image_count,
        settings.view_url(register, 1),
        views,
    ))
}

/// The row of the cited register. The call number only breaks a tie: the
/// portals show none (Ain shows an internal reference), or one shared by the
/// registers of every locality or of several acts (Ardèche), so a cited call
/// number a row does not carry must not discard it.
fn choose<'r>(
    rows: &'r [crate::platform::select::Candidate<Option<page::Register>>],
    citation: &CitationParts,
) -> Selection<'r, Option<page::Register>> {
    let localities = [citation.locality.as_str()];
    let mut without = citation.clone();
    without.call_number = None;
    match select(rows, &without, &localities) {
        Selection::Many(count) if count > 1 && citation.call_number.is_some() => {
            match select(rows, citation, &localities) {
                one @ Selection::One(_) => one,
                Selection::Many(_) => Selection::Many(count),
            }
        }
        other => other,
    }
}

/// The image of one view, on the portal's own origin: the level 1 service's
/// whole image bounded to the screen, and a tile. The size is the canvas's:
/// the service's own `info.json` is unreliable on these portals.
fn image(settings: &Settings, canvas: &page::Canvas) -> Result<ArchiveImage, ResolveError> {
    let base = canvas
        .service
        .as_deref()
        .and_then(|service| settings.on_origin(service, "/iiif/"))
        .ok_or_else(|| {
            ResolveError::UnexpectedResponse("ligeo: a canvas lacks its image service".to_owned())
        })?;
    let picture = if canvas.width.max(canvas.height) > PICTURE_BOUND {
        // Level 1 sizes by width or height alone, not by a bounding box.
        if canvas.width >= canvas.height {
            format!("{PICTURE_BOUND},")
        } else {
            format!(",{PICTURE_BOUND}")
        }
    } else {
        "full".to_owned()
    };
    Ok(ArchiveImage {
        picture: format!("{base}/full/{picture}/0/default.jpg"),
        thumbnail: format!("{base}/full/{THUMBNAIL_MIN_WIDTH},/0/default.jpg"),
        width: canvas.width,
        height: canvas.height,
    })
}
