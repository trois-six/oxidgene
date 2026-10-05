//! Mnesys Expo (Naoned), the publishing software of the departmental
//! archives of Indre-et-Loire, Calvados and Marne among others.
//!
//! One search form of the portal serves every register of the archive —
//! parish registers, civil status, decennial tables — so each archive has
//! one collection. The form is a plain `GET` of `/search/results` filtered by
//! the locality's exact label, the act's label and the year; the answer's
//! rows are selected down to one register (`select`), whose image is
//! addressed by its own ARK, `/ark:/<naan>/<name>/<image id>`: the portal's
//! viewer opens on it, and it is also the view's persistent address. Archive
//! Portals §4.4 specifies the requests.

mod page;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::Deserialize;

use super::iiif::image_info;
use super::markup::fold;
use super::select::{Candidate, Selection, select};
use super::view::{cited_views, view_target};
use super::{Access, BoxFuture, Platform, PortalEndpoint, Query, is_https_origin};
use crate::catalog::{Archive, CatalogError, Collection, Display};
use crate::citation::{Act, ActKind, CitationParts};
use crate::transport::PortalFetch;
use crate::{ArchiveImage, ArchiveTarget, ArchiveView, ResolveError};

/// The rows one results page asks for: the portal serves 20, 40 or 80.
const RESULTS_PER_PAGE: usize = 80;

/// The widest run of images one viewer request asks for.
const MAX_WINDOW: u16 = 10;

/// The Mnesys adapter.
pub struct Mnesys;

/// A collection's `portal` settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    origin: String,
    #[serde(default)]
    transport: Access,
    /// The search form's UUID.
    form: String,
    fields: Fields,
    /// The patterns of a locality's value, with `{locality}`. The portal
    /// needs the exact label, and its labels differ for communes that still
    /// exist and communes since merged: every pattern is sent, and a label
    /// the portal does not know is ignored.
    locality_label: Vec<String>,
    #[serde(default)]
    locality_style: LocalityStyle,
    /// The portal's labels of each act code. Several codes may share a
    /// label (`baptêmes - naissances`).
    acts: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    call_number: CallNumber,
    image_source: ImageSource,
}

/// The names of the form's inputs, which carry a prefix per portal
/// (`0-controlledAccessGeographicName[]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    locality: String,
    act: String,
    year: String,
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

/// Whether the rows show the register's call number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CallNumber {
    #[default]
    Row,
    /// The portal shows none: the cited call number cannot select a row.
    None,
}

/// Where the images of a register are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ImageSource {
    /// The register's IIIF manifest: every image with its pixel size, in one
    /// request that grows with the register (hundreds of kilobytes).
    Manifest,
    /// The viewer's own endpoint, asked for the cited images only; the pixel
    /// size of an image comes from its `info.json`.
    Visualizer,
}

/// The articles `article_suffix` moves behind the name.
const ARTICLES: [&str; 5] = ["Les ", "Le ", "La ", "L'", "L’"];

fn invalid(message: &str) -> CatalogError {
    CatalogError::new(format!("mnesys settings: {message}"))
}

/// An input name: letters, digits, `-`, `_` and the `[]` of a repeatable one.
fn is_field_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_[]".contains(&byte))
}

/// A form UUID: five hexadecimal groups of 8, 4, 4, 4 and 12 digits.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The words of a text that identify an act: folded, at least four letters,
/// plural marks removed.
fn words(text: &str) -> Vec<String> {
    fold(text)
        .split(' ')
        .filter(|word| word.len() >= 4)
        .map(|word| word.strip_suffix('s').unwrap_or(word).to_owned())
        .collect()
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
        if !is_uuid(&self.form) {
            return Err(invalid("form must be a UUID"));
        }
        let names = [&self.fields.locality, &self.fields.act, &self.fields.year];
        if !names.into_iter().all(|name| is_field_name(name)) {
            return Err(invalid("input names"));
        }
        if self.locality_label.is_empty()
            || !self
                .locality_label
                .iter()
                .all(|label| label.matches("{locality}").count() == 1)
        {
            return Err(invalid("each locality label needs one {locality}"));
        }
        Ok(())
    }

    /// Every act code is valid and has labels, and every act the collection
    /// holds has some.
    fn check_acts(&self, collection: &Collection) -> Result<(), CatalogError> {
        for (code, labels) in &self.acts {
            if Act::from_code(code).is_none() {
                return Err(invalid(&format!("`{code}` is not an act code")));
            }
            if labels.is_empty() || labels.iter().any(|label| label.trim().is_empty()) {
                return Err(invalid(&format!("`{code}` has an empty label")));
            }
        }
        match collection
            .acts
            .iter()
            .find(|act| self.act_labels(act).is_empty())
        {
            Some(act) => Err(invalid(&format!("no label for `{act}`"))),
            None => Ok(()),
        }
    }

    /// The labels of an act: its own, or for a combined act (`BMS`) those of
    /// its first kind, whose category holds the mixed registers.
    fn act_labels(&self, act: &Act) -> &[String] {
        self.acts
            .get(&act.to_string())
            .or_else(|| {
                let first = act.kinds().first()?;
                self.acts.get(&first.letter().to_string())
            })
            .map_or(&[], Vec::as_slice)
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
    /// the locality's labels, the act's labels and the year. Nothing else of
    /// the citation leaves the application.
    fn filters(&self, citation: &CitationParts) -> Query {
        let mut query = Query::new();
        query
            .push("formUuid", self.form.as_str())
            .push("mode", "list")
            .push("sort", "date_asc");
        let locality = self.locality(citation);
        for pattern in &self.locality_label {
            query.push(
                self.fields.locality.as_str(),
                pattern.replace("{locality}", &locality),
            );
        }
        for label in self.act_labels(&citation.act) {
            query.push(self.fields.act.as_str(), label.as_str());
        }
        if let Some(year) = citation.year {
            query.push(self.fields.year.as_str(), year.to_string());
        }
        query
    }

    fn search_request(&self, filters: &Query) -> String {
        format!("/search/results?{filters}&resultsPerPage={RESULTS_PER_PAGE}")
    }

    fn search_page(&self, filters: &Query) -> String {
        format!("{}/search/results?{filters}", self.origin)
    }

    /// The words that mark a table among the portal's context entries: those
    /// of the table acts' labels.
    fn table_words(&self) -> Vec<String> {
        self.acts
            .iter()
            .filter(|(code, _)| matches!(Act::from_code(code), Some(Act::Table(_))))
            .flat_map(|(_, labels)| labels.iter().flat_map(|label| words(label)))
            .collect()
    }

    /// The act kinds a row's text mentions, written as an act code: the
    /// labels' words of each single-kind act found among the row's words.
    fn row_act(&self, row_words: &[String]) -> Option<String> {
        let kinds: String = self
            .acts
            .iter()
            .filter_map(|(code, labels)| match Act::from_code(code)? {
                Act::Register(kinds) if kinds.len() == 1 => Some((kinds[0], labels)),
                _ => None,
            })
            .filter(|(_, labels)| {
                labels
                    .iter()
                    .flat_map(|label| words(label))
                    .any(|word| row_words.contains(&word))
            })
            .map(|(kind, _): (ActKind, _)| kind.letter())
            .collect();
        (!kinds.is_empty()).then_some(kinds)
    }

    /// The registers of a results page as selection candidates, without the
    /// rows the cited act excludes. The act filter is not exclusive — a
    /// search for births also returns the births' decennial tables — so a row
    /// is read for what it is: a table, or a register of the acts its text
    /// mentions. `forms` are the folded forms of the cited locality a row may
    /// show.
    fn candidates(
        &self,
        rows: Vec<page::Row>,
        citation: &CitationParts,
        forms: &[String],
    ) -> Vec<Candidate<Register>> {
        let table_words = self.table_words();
        let wants_table = matches!(citation.act, Act::Table(_));
        let parish = citation.parish.as_deref().map(fold);
        rows.into_iter()
            .filter_map(|row| {
                let text = std::iter::once(row.title.as_str())
                    .chain(row.collection.as_deref())
                    .chain(row.context.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" ");
                let row_words = words(&text);
                let is_table = row_words.iter().any(|word| table_words.contains(word));
                (is_table == wants_table).then_some((row, row_words))
            })
            .map(|(row, row_words)| Candidate {
                locality: row
                    .context
                    .iter()
                    .find(|entry| forms.contains(&fold(entry)))
                    .or_else(|| row.context.first())
                    .cloned(),
                call_number: row.call_number,
                act: self.row_act(&row_words),
                parish: parish.as_ref().and_then(|parish| {
                    row.context
                        .iter()
                        .find(|entry| {
                            let entry = fold(entry);
                            entry == *parish || entry.ends_with(&format!(" {parish}"))
                        })
                        .cloned()
                }),
                period: row.period,
                images: row.images,
                payload: Register { ark: row.ark },
            })
            .collect()
    }
}

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Register {
    ark: page::Ark,
}

impl Platform for Mnesys {
    fn id(&self) -> &'static str {
        "mnesys"
    }

    fn validate(&self, collection: &Collection) -> Result<(), CatalogError> {
        Settings::read(collection).map(drop)
    }

    fn endpoint(&self, collection: &Collection) -> Option<PortalEndpoint> {
        let settings = Settings::read(collection).ok()?;
        Some(PortalEndpoint {
            start: format!("{}/search/form/{}", settings.origin, settings.form),
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
    let page = page::results(&answer, RESULTS_PER_PAGE)?;
    // Rows beyond the page are unseen: a twin of the selected one may be
    // among them, so the search page is the answer.
    if page.total > page.rows.len() {
        return Ok(results(page.total));
    }

    let styled = settings.locality(citation);
    let forms = [fold(&citation.locality), fold(&styled)];
    let candidates = settings.candidates(page.rows, citation, &forms);
    // Rows showing no call number cannot be told apart by the cited one.
    let without_call_number;
    let cited = if settings.call_number == CallNumber::None && citation.call_number.is_some() {
        without_call_number = CitationParts {
            call_number: None,
            ..citation.clone()
        };
        &without_call_number
    } else {
        citation
    };
    let row = match select(&candidates, cited, &[&citation.locality, &styled]) {
        Selection::One(row) => row,
        Selection::Many(matches) => return Ok(results(matches)),
    };
    let ark = &row.payload.ark;
    // A register listed without images cannot be opened on a view.
    let Some(image_count) = row.images.filter(|count| *count > 0) else {
        return Ok(results(1));
    };

    let cited_views = cited_views(citation, usize::from(image_count));
    let mut numbers: Vec<u16> = cited_views.iter().map(|view| view.view).collect();
    numbers.sort_unstable();
    numbers.dedup();
    let images = images(&settings, ark, &numbers, fetch).await?;

    let mut views = Vec::with_capacity(numbers.len());
    for (view, image) in numbers.into_iter().zip(images) {
        let address = ark.image(&settings.origin, &image.id);
        views.push(ArchiveView {
            view,
            url: address.clone(),
            ark: Some(address),
            image: match archive.display {
                Display::Iiif => Some(iiif_image(&settings, ark, &image, fetch).await?),
                Display::Portal => None,
            },
        });
    }
    Ok(view_target(
        archive,
        citation,
        row.call_number.as_deref(),
        usize::from(image_count),
        ark.image(&settings.origin, &ark.first_image),
        views,
    ))
}

/// One cited image: its identifier and, when the source gives it, its size.
struct Image {
    id: String,
    size: Option<(u32, u32)>,
}

/// The identifiers of the cited views' images, in order.
async fn images(
    settings: &Settings,
    ark: &page::Ark,
    views: &[u16],
    fetch: &dyn PortalFetch,
) -> Result<Vec<Image>, ResolveError> {
    if views.is_empty() {
        return Ok(Vec::new());
    }
    match settings.image_source {
        ImageSource::Manifest => {
            let answer = fetch
                .get(&format!(
                    "/iiif/ark:/{}/{}/manifest.json",
                    ark.naan, ark.name
                ))
                .await?;
            let canvases = page::manifest_canvases(&answer)?;
            views
                .iter()
                .map(|view| {
                    let canvas = canvases.get(usize::from(*view) - 1).ok_or_else(|| {
                        ResolveError::UnexpectedResponse(
                            "mnesys: the manifest lists fewer images than the row".to_owned(),
                        )
                    })?;
                    Ok(Image {
                        id: canvas.image.clone(),
                        size: Some((canvas.width, canvas.height)),
                    })
                })
                .collect()
        }
        ImageSource::Visualizer => {
            let mut images = Vec::with_capacity(views.len());
            for (start, end) in windows(views) {
                let answer = fetch
                    .get(&format!(
                        "/visualizer/api?arkName={}&start={}&end={}&group=0",
                        ark.name,
                        start - 1,
                        end - 1
                    ))
                    .await?;
                for view in views.iter().filter(|view| (start..=end).contains(view)) {
                    images.push(Image {
                        id: page::window_image(&answer, start - 1, view - 1)?,
                        size: None,
                    });
                }
            }
            Ok(images)
        }
    }
}

/// The one-based `(first, last)` views of the requests that cover `views`,
/// ascending: neighbouring views share a request, up to [`MAX_WINDOW`].
fn windows(views: &[u16]) -> Vec<(u16, u16)> {
    let mut windows: Vec<(u16, u16)> = Vec::new();
    for view in views {
        match windows.last_mut() {
            Some((start, end)) if view - *start < MAX_WINDOW => *end = *view,
            _ => windows.push((*view, *view)),
        }
    }
    windows
}

/// The image of one view, for a `display: "iiif"` archive. The service is
/// level 0 and serves the whole image only, so the picture is the full image
/// and the thumbnail is the portal's own, a few kilobytes. The size comes
/// from the manifest, or from the image's `info.json`.
async fn iiif_image(
    settings: &Settings,
    ark: &page::Ark,
    image: &Image,
    fetch: &dyn PortalFetch,
) -> Result<ArchiveImage, ResolveError> {
    let path = format!("/iiif/ark:/{}/{}/{}", ark.naan, ark.name, image.id);
    let thumbnail = format!("{}/images/{}_thumbnail.jpg", settings.origin, image.id);
    let base = format!("{}{path}", settings.origin);
    Ok(match image.size {
        Some((width, height)) => ArchiveImage {
            picture: format!("{base}/full/max/0/default.jpg"),
            thumbnail,
            width,
            height,
        },
        None => ArchiveImage {
            thumbnail,
            ..image_info(&fetch.get(&format!("{path}/info.json")).await?)?.image(&base)
        },
    })
}
