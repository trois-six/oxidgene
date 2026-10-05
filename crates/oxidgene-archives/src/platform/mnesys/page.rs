//! Reading the portal's answers: the search form's lists, the results
//! page's rows, the viewer endpoint's image window and state, and the
//! register's IIIF manifest.
//!
//! The results page is server-rendered. Each register is one
//! `li.element-list` whose markup the shared scans read: the title, the
//! displayed period, the call numbers where the portal shows them, the image
//! count (`315 medias`, or `2 lots 892 medias` over several lots), the context
//! list and the ARK of the register's first image. Nothing else of a response
//! is read, and a response that lacks what the adapter needs is reported as a
//! changed shape without its content.

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
    /// The call numbers the row shows, the register's own first: one cell
    /// may hold several (`3Q19 (Cote), 6NUM3/001/003 (Cote)`, `446, 1RP1031`).
    pub(super) call_numbers: Vec<String>,
    /// The count of the register's images, when the row shows it: `None`
    /// when it shows no count, or one over several lots (`2 lots 892
    /// medias`), one of which may be a document rather than images.
    pub(super) images: Option<u16>,
    /// The collection the portal files the register under, from the first
    /// context entry (`Contexte : Registres paroissiaux numérisés`).
    pub(super) collection: Option<String>,
    /// The other context entries, in order: the locality, the parish or
    /// establishment, the act, the period, depending on the portal; the
    /// last repeats the title.
    pub(super) context: Vec<String>,
    /// The ARK of the first image; `None` for a register listed without
    /// images (`Manque`) or hosted elsewhere, whose row links its record
    /// only.
    pub(super) ark: Option<Ark>,
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
    // The first link to an image: a row without images links its record
    // only, `/ark:/<naan>/<name>`.
    let ark = markup::attributes(html, "href")
        .iter()
        .find_map(|link| ark(link));
    let pictures = markup::text_after(html, "class=\"info-list-picture\">");
    let counts_media = pictures
        .as_deref()
        .and_then(markup::first_number::<u32>)
        .is_some_and(|count| count > 0);
    if ark.is_none() && counts_media {
        return Err(unexpected("a row lacks its ARK"));
    }
    let (collection, context) = context(html);
    if context.is_empty() {
        return Err(unexpected("a row lacks its context"));
    }
    Ok(Row {
        title,
        period: span_after(html, "<h3>Date</h3>"),
        call_numbers: markup::text_after(html, "class=\"referenceCodes\">")
            .map(|cell| call_numbers(&cell))
            .unwrap_or_default(),
        images: pictures.as_deref().and_then(image_count),
        collection,
        context,
        ark,
    })
}

/// The image count of `315 medias`; none for `2 lots 892 medias`, whose
/// count spans every lot.
fn image_count(text: &str) -> Option<u16> {
    if text.contains("lot") {
        return None;
    }
    markup::first_number(text)
}

/// The call numbers of a `Cote` cell, the register's own first. A part's
/// `(Cote)` label (`(Cote(s))`, `(Cote/Cotes extrêmes)`) is dropped, a part
/// with another note (`(Ancienne cote …)`) is not the register's, and a bare
/// number (`446`), an internal number, is listed last.
pub(super) fn call_numbers(cell: &str) -> Vec<String> {
    let mut numbers: Vec<String> = cell
        .split(", ")
        .filter_map(call_number)
        .map(str::to_owned)
        .collect();
    numbers.sort_by_key(|number| number.bytes().all(|byte| byte.is_ascii_digit()));
    numbers
}

fn call_number(part: &str) -> Option<&str> {
    let mut number = part.trim();
    while number.ends_with(')')
        && let Some(at) = number.rfind(" (")
    {
        let note = &number[at + 2..];
        if note.starts_with("Cote") {
            number = number[..at].trim_end();
        } else if note.starts_with(|c: char| c.is_ascii_digit()) {
            // A volume number, `2 E 558 (96)`, belongs to the call number.
            break;
        } else {
            return None;
        }
    }
    (!number.is_empty()).then_some(number)
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
pub(super) fn ark(link: &str) -> Option<Ark> {
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

/// The image count of a register, from the viewer's state for its first
/// image (`/visualizer/api?arkName=<name>&uuid=<image>`): `counts.media`,
/// the images of the lot that image opens.
pub(super) fn viewer_count(answer: &str) -> Result<u16, ResolveError> {
    let state: Value =
        serde_json::from_str(answer).map_err(|_| unexpected("the viewer state is not JSON"))?;
    state
        .pointer("/counts/media")
        .and_then(Value::as_u64)
        .and_then(|count| u16::try_from(count).ok())
        .ok_or_else(|| unexpected("the viewer state lacks the image count"))
}

/// The labels a select of the search form offers, by the input's name: the
/// form renders each select as an `enhanced-select` element whose
/// `data-name` is the input's name without its `[]` and whose
/// `data-options` lists the labels as JSON.
pub(super) fn options(form: &str, name: &str) -> Option<Vec<String>> {
    let tag = select_tag(form, name)?;
    markup::attribute(tag, "data-options").and_then(|options| serde_json::from_str(&options).ok())
}

/// The opening tag of a select of the form, by the input's name.
fn select_tag<'f>(form: &'f str, name: &str) -> Option<&'f str> {
    let marker = format!("data-name=\"{}\"", name.trim_end_matches("[]"));
    let at = form.find(&marker)?;
    // The element's own attributes, up to the end of its opening tag.
    let start = form[..at].rfind('<').unwrap_or(at);
    let tag = &form[start..];
    Some(&tag[..tag.find('>').unwrap_or(tag.len())])
}

/// Whether the form declares the input `name` as it is written: a select
/// (`enhanced-select`) under `name[]`, since the portal answers an error to
/// a select's value sent without the brackets, or a plain input under its
/// own name. The live checks test the settings with it.
#[cfg(any(test, feature = "live"))]
pub(super) fn declares(form: &str, name: &str) -> bool {
    match name.strip_suffix("[]") {
        Some(_) => select_tag(form, name).is_some(),
        None => select_tag(form, name).is_none() && form.contains(&format!("name=\"{name}\"")),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_call_number_of_a_cell() {
        for (cell, expected) in [
            ("6NUM8/999/050 (Cote)", vec!["6NUM8/999/050"]),
            ("9MI_X999 (Cote(s))", vec!["9MI_X999"]),
            ("9 Mi 9999 (Cote/Cotes extrêmes)", vec!["9 Mi 9999"]),
            ("9 E 999 (99)", vec!["9 E 999 (99)"]),
            ("9 E 999 (99) (Cote)", vec!["9 E 999 (99)"]),
            ("999, 9RP9999", vec!["9RP9999", "999"]),
            (
                "9Q99 (Cote), 9NUM9/999/999 (Cote)",
                vec!["9Q99", "9NUM9/999/999"],
            ),
            ("9E999/9, 9E999/8 (Ancienne cote aux AD)", vec!["9E999/9"]),
        ] {
            assert_eq!(call_numbers(cell), expected, "{cell}");
        }
    }

    #[test]
    fn counts_the_images_of_one_lot_only() {
        assert_eq!(image_count("315 medias"), Some(315));
        assert_eq!(image_count("1 media"), Some(1));
        assert_eq!(image_count("2 lots 892 medias"), None);
        assert_eq!(
            viewer_count(r#"{"counts": {"media": 891, "group": 2}}"#),
            Ok(891)
        );
        assert!(viewer_count(r#"{"counts": {}}"#).is_err());
    }

    #[test]
    fn tells_a_select_from_a_plain_input() {
        let form = r#"<div class="enhanced-select multiselect" data-name="1-date" data-options="[&quot;1836&quot;,&quot;1866&quot;]"></div>
            <input type="text" class="form-control" id="0-title" name="0-title" value=""/>"#;
        assert_eq!(
            options(form, "1-date[]"),
            Some(vec!["1836".to_owned(), "1866".to_owned()])
        );
        assert!(declares(form, "1-date[]"));
        assert!(!declares(form, "1-date"));
        assert!(declares(form, "0-title"));
        assert!(!declares(form, "0-title[]"));
        assert!(!declares(form, "2-date"));
    }
}
