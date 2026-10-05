//! Reading the portal's answers: the results page's rows, the viewer
//! endpoint's image window and the register's IIIF manifest.
//!
//! The results page is server-rendered. Each register is one
//! `li.element-list` whose markup the shared scans read: the title, the
//! displayed period, the call number where the portal shows one, the image
//! count (`315 medias`), the context list and the ARK of the register's first
//! image. Nothing else of a response is read, and a response that lacks what
//! the adapter needs is reported as a changed shape without its content.

use serde::Deserialize;
use serde_json::Value;

use crate::ResolveError;
use crate::platform::markup;

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("mnesys: {detail}"))
}

/// A register's persistent address, `/ark:/<naan>/<name>`, and its first
/// image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ark {
    pub(super) naan: String,
    pub(super) name: String,
    /// The first image's identifier, the last segment of the row's link.
    pub(super) first_image: String,
}

impl Ark {
    /// The register's address on `origin`.
    pub(super) fn register(&self, origin: &str) -> String {
        format!("{origin}/ark:/{}/{}", self.naan, self.name)
    }

    /// The address of the image `image` of this register on `origin`: the
    /// target of a view, which opens the viewer on it.
    pub(super) fn image(&self, origin: &str, image: &str) -> String {
        format!("{}/{image}", self.register(origin))
    }
}

/// One register of the results page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) title: String,
    /// The `Date` shown beside the title.
    pub(super) period: Option<String>,
    pub(super) call_number: Option<String>,
    pub(super) images: Option<u16>,
    /// The collection the portal files the register under, from the first
    /// context entry (`Contexte : Registres paroissiaux numérisés`).
    pub(super) collection: Option<String>,
    /// The other context entries, in order: the locality, the parish or
    /// establishment, the act, the period, depending on the portal.
    pub(super) context: Vec<String>,
    pub(super) ark: Ark,
}

/// The page of results: the total the portal reports, and this page's rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Results {
    pub(super) total: usize,
    pub(super) rows: Vec<Row>,
}

/// Reads a results page, requested with `resultsPerPage` rows at most.
pub(super) fn results(page: &str, per_page: usize) -> Result<Results, ResolveError> {
    if page.contains("class=\"no-result\"") {
        return Ok(Results {
            total: 0,
            rows: Vec::new(),
        });
    }
    let total: usize = markup::text_after(page, "class=\"result\">")
        .and_then(|text| markup::first_number(&text))
        .ok_or_else(|| unexpected("the results page shows no total"))?;
    let rows = markup::split_after(page, "<li class=\"element-list\">")
        .into_iter()
        .map(row)
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() != total.min(per_page) {
        return Err(unexpected("the rows do not match the reported total"));
    }
    Ok(Results { total, rows })
}

/// The text of the first `<span>` after `marker`.
fn span_after(html: &str, marker: &str) -> Option<String> {
    let rest = &html[html.find(marker)?..];
    markup::text_after(rest, "<span>").filter(|text| !text.is_empty())
}

fn row(html: &str) -> Result<Row, ResolveError> {
    let title = span_after(html, "<h2>").ok_or_else(|| unexpected("a row lacks its title"))?;
    let ark = markup::attribute(html, "href")
        .and_then(|link| ark(&link))
        .ok_or_else(|| unexpected("a row lacks its ARK"))?;
    let (collection, context) = context(html);
    if context.is_empty() {
        return Err(unexpected("a row lacks its context"));
    }
    Ok(Row {
        title,
        period: span_after(html, "<h3>Date</h3>"),
        call_number: markup::text_after(html, "class=\"referenceCodes\">")
            .map(|text| {
                text.strip_suffix("(Cote)")
                    .map_or(text.clone(), |code| code.trim().to_owned())
            })
            .filter(|text| !text.is_empty()),
        images: markup::text_after(html, "class=\"info-list-picture\">")
            .and_then(|text| markup::first_number(&text)),
        collection,
        context,
        ark,
    })
}

/// A row's context list: the collection entry, shown as
/// `Contexte : <collection>`, and the other entries in order.
fn context(html: &str) -> (Option<String>, Vec<String>) {
    let Some(start) = html.find("<ul class=\"context") else {
        return (None, Vec::new());
    };
    let list = &html[start..];
    let list = &list[..list.find("</ul>").unwrap_or(list.len())];
    let mut collection = None;
    let mut entries = Vec::new();
    for item in markup::split_after(list, "<li>") {
        let text = markup::strip_tags(item);
        if item.contains("class=\"context-content\"") {
            let name = text.strip_prefix("Contexte :").unwrap_or(&text).trim();
            collection = Some(name.to_owned()).filter(|name| !name.is_empty());
        } else if !text.is_empty() {
            entries.push(text);
        }
    }
    (collection, entries)
}

/// Reads `/ark:/<naan>/<name>/<image>`, with an optional origin before it.
fn ark(link: &str) -> Option<Ark> {
    let path = &link[link.find("/ark:/")? + "/ark:/".len()..];
    let mut parts = path.split('/');
    let (naan, name, first_image) = (parts.next()?, parts.next()?, parts.next()?);
    (parts.next().is_none() && is_naan(naan) && is_name(name) && is_image_id(first_image)).then(
        || Ark {
            naan: naan.to_owned(),
            name: name.to_owned(),
            first_image: first_image.to_owned(),
        },
    )
}

fn is_naan(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_name(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// An image identifier: a UUID, as hexadecimal groups and hyphens.
pub(super) fn is_image_id(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
}

/// The identifier of the image at zero-based `index` in a viewer window that
/// starts at `start`. The endpoint answers an array for a window that starts
/// at the first image and an object keyed by index otherwise.
pub(super) fn window_image(answer: &str, start: u16, index: u16) -> Result<String, ResolveError> {
    let window: Value =
        serde_json::from_str(answer).map_err(|_| unexpected("the viewer window is not JSON"))?;
    let entry = match &window {
        Value::Array(entries) => entries.get(usize::from(index - start)),
        Value::Object(entries) => entries.get(&index.to_string()),
        _ => None,
    };
    entry
        .and_then(|entry| entry.get("uuid"))
        .and_then(Value::as_str)
        .filter(|uuid| is_image_id(uuid))
        .map(str::to_owned)
        .ok_or_else(|| unexpected("the viewer window lacks the image"))
}

/// One image of a register's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Canvas {
    pub(super) image: String,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Deserialize)]
struct Manifest {
    items: Vec<ManifestCanvas>,
}

#[derive(Deserialize)]
struct ManifestCanvas {
    id: String,
    width: u32,
    height: u32,
}

/// The images of a register's IIIF Presentation 3 manifest, in order: each
/// canvas's id ends `/<image id>/canvas/<n>`. Nothing else of the manifest
/// is read; its service entries are labelled loosely and name the portal's
/// own host.
pub(super) fn manifest_canvases(answer: &str) -> Result<Vec<Canvas>, ResolveError> {
    let manifest: Manifest = serde_json::from_str(answer)
        .map_err(|_| unexpected("the manifest lacks items with id and size"))?;
    manifest
        .items
        .into_iter()
        .map(|canvas| {
            let image = canvas
                .id
                .split_once("/canvas/")
                .and_then(|(before, _)| before.rsplit('/').next())
                .filter(|image| is_image_id(image))
                .filter(|_| canvas.width > 0 && canvas.height > 0)
                .ok_or_else(|| unexpected("a manifest canvas lacks its image or size"))?;
            Ok(Canvas {
                image: image.to_owned(),
                width: canvas.width,
                height: canvas.height,
            })
        })
        .collect()
}
