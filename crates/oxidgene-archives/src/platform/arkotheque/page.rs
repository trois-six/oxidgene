//! Reading the request interface's answers: the search's result rows, the
//! viewer's image list, and an image service's `info.json`.
//!
//! The search answer lists its registers twice: `resultats.results` gives
//! each one's record reference and call number, and `resultats.html` renders
//! the same registers, in the same order, as table rows whose cells carry
//! the locality, parish, acts and period, and whose viewer button carries
//! the viewer address and the image count. The rows are read with a few
//! attribute scans; no HTML parser is needed for them.

use serde::Deserialize;

use crate::ResolveError;

/// One register of a search answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    /// The record reference, `arko_fiche_…`.
    pub(super) record: String,
    /// The call number; empty on some tables.
    pub(super) call_number: String,
    /// The `data-champ` cells, by name, as displayed.
    pub(super) cells: Vec<(String, String)>,
    /// The image count shown beside the viewer button.
    pub(super) images: Option<u16>,
    /// `/_recherche-api/visionneuse-infos/…`, when the register has images.
    pub(super) viewer: Option<String>,
}

impl Row {
    pub(super) fn cell(&self, name: &str) -> Option<&str> {
        self.cells
            .iter()
            .find(|(cell, _)| cell == name)
            .map(|(_, value)| value.as_str())
    }
}

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("arkotheque: {detail}"))
}

#[derive(Deserialize)]
struct SearchAnswer {
    resultats: Resultats,
}

#[derive(Deserialize)]
struct Resultats {
    results: Vec<ResultEntry>,
    html: String,
}

#[derive(Deserialize)]
struct ResultEntry {
    #[serde(rename = "refUnique")]
    record: String,
    #[serde(default)]
    intitule: Option<String>,
}

/// The rows of a search answer.
pub(super) fn search_rows(answer: &str) -> Result<Vec<Row>, ResolveError> {
    let answer: SearchAnswer = serde_json::from_str(answer)
        .map_err(|_| unexpected("the search answer lacks resultats.results or html"))?;
    let rendered: Vec<&str> = answer
        .resultats
        .html
        .split("<tr class=\"resultat_container")
        .skip(1)
        .collect();
    if rendered.len() != answer.resultats.results.len() {
        return Err(unexpected("the rendered rows do not match the results"));
    }
    Ok(answer
        .resultats
        .results
        .into_iter()
        .zip(rendered)
        .map(|(entry, html)| Row {
            record: entry.record,
            call_number: entry.intitule.unwrap_or_default().trim().to_owned(),
            cells: cells(html),
            images: image_count(html),
            viewer: attribute(html, "data-visionneuse-url")
                .filter(|address| address.starts_with("/_recherche-api/visionneuse-infos/")),
        })
        .collect())
}

/// Every `data-champ` element's name and text.
fn cells(html: &str) -> Vec<(String, String)> {
    let mut cells = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("data-champ=\"") {
        rest = &rest[at + "data-champ=\"".len()..];
        let Some(name_end) = rest.find('"') else {
            break;
        };
        let name = &rest[..name_end];
        let Some(text_start) = rest.find('>') else {
            break;
        };
        let text = &rest[text_start + 1..];
        let text = &text[..text.find('<').unwrap_or(text.len())];
        let text = decode_entities(text.trim());
        if !text.is_empty() {
            cells.push((name.to_owned(), text));
        }
        rest = &rest[text_start..];
    }
    cells
}

/// `(46 images)` beside the viewer button.
fn image_count(html: &str) -> Option<u16> {
    let at = html.find("class=\"nombre_images\">")? + "class=\"nombre_images\">".len();
    let digits: String = html[at..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The decoded value of the first `name="…"` attribute.
fn attribute(html: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let start = html.find(&marker)? + marker.len();
    let end = html[start..].find('"')?;
    Some(decode_entities(&html[start..start + end]))
}

/// The character references the portal's markup uses.
fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        decoded.push_str(&rest[..at]);
        rest = &rest[at..];
        let reference = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| Some((entity(&rest[1..end])?, end)));
        match reference {
            Some((character, end)) => {
                decoded.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    })
}

/// One image of a register, as the viewer endpoint lists it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct Source {
    /// `/_recherche-images/show/…/<index>`: the image's IIIF base on the
    /// portal's origin.
    pub(super) src: String,
    /// `/ark:<naan>/<name>…`, the image's persistent address.
    #[serde(rename = "ARKLink", default)]
    pub(super) ark: Option<String>,
}

#[derive(Deserialize)]
struct ViewerAnswer {
    medias: Vec<Media>,
}

#[derive(Deserialize)]
struct Media {
    sources: Vec<Source>,
}

/// The images of a register, in order. Nothing else of the answer is read:
/// its other fields name internal hosts and files.
pub(super) fn viewer_sources(answer: &str) -> Result<Vec<Source>, ResolveError> {
    let answer: ViewerAnswer = serde_json::from_str(answer)
        .map_err(|_| unexpected("the viewer answer lacks medias[].sources"))?;
    let mut sources = answer
        .medias
        .into_iter()
        .next()
        .map(|media| media.sources)
        .unwrap_or_default();
    if sources.is_empty()
        || !sources.iter().all(|source| {
            source.src.starts_with("/_recherche-images/") && !source.src.contains(['?', '#'])
        })
    {
        return Err(unexpected("the viewer answer lists no image path"));
    }
    for source in &mut sources {
        source.ark = source
            .ark
            .take()
            .filter(|ark| ark.starts_with("/ark:") && !ark.contains(['?', '#', ' ']));
    }
    Ok(sources)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

/// What OxidGene reads of an IIIF Image API 2 `info.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct ImageInfo {
    pub(super) width: u32,
    pub(super) height: u32,
    #[serde(default)]
    sizes: Vec<Size>,
    #[serde(default)]
    profile: serde_json::Value,
}

/// The longest side the viewer asks for where the service scales freely.
const PICTURE_BOUND: u32 = 2048;

/// The narrowest listed size a gallery tile is built from.
const THUMBNAIL_MIN_WIDTH: u32 = 150;

pub(super) fn image_info(answer: &str) -> Result<ImageInfo, ResolveError> {
    let info: ImageInfo =
        serde_json::from_str(answer).map_err(|_| unexpected("info.json lacks the image size"))?;
    if info.width == 0 || info.height == 0 {
        return Err(unexpected("info.json gives an empty image"));
    }
    Ok(info)
}

impl ImageInfo {
    /// Whether the service scales to any size: IIIF level 2.
    fn scales_freely(&self) -> bool {
        let level = match &self.profile {
            serde_json::Value::Array(entries) => entries.first(),
            other => Some(other),
        };
        level
            .and_then(serde_json::Value::as_str)
            .is_some_and(|level| level.ends_with("level2.json"))
    }

    /// The size parameter of the picture: bounded to the screen where the
    /// service scales freely, the full image otherwise.
    pub(super) fn picture_size(&self) -> String {
        if self.scales_freely() && self.width.max(self.height) > PICTURE_BOUND {
            format!("!{PICTURE_BOUND},{PICTURE_BOUND}")
        } else {
            "full".to_owned()
        }
    }

    /// The size parameter of the thumbnail: the smallest listed size wide
    /// enough for a tile, which every service level serves.
    pub(super) fn thumbnail_size(&self) -> String {
        self.sizes
            .iter()
            .filter(|size| size.width >= THUMBNAIL_MIN_WIDTH)
            .min_by_key(|size| size.width)
            .or_else(|| self.sizes.iter().max_by_key(|size| size.width))
            .map_or_else(
                || "full".to_owned(),
                |size| format!("{},{}", size.width, size.height),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_character_references_of_the_markup() {
        assert_eq!(
            decode_entities("Type d&#039;actes &amp; &quot;registres&quot; &#x2014; &lt;b&gt;"),
            "Type d'actes & \"registres\" \u{2014} <b>"
        );
        assert_eq!(decode_entities("A & B &unknown; &"), "A & B &unknown; &");
    }

    #[test]
    fn reads_cells_skipping_empty_ones() {
        let html = r#"<td><span data-champ="commune" data-type-champ="x">Bourg (Le)</span></td>
            <td><!-- Valeur vide pour le champ: paroisse --></td>
            <td><span data-champ="cote">  </span></td>
            <td><span data-champ="date">1700 &amp; 1701</span></td>"#;
        assert_eq!(
            cells(html),
            [
                ("commune".to_owned(), "Bourg (Le)".to_owned()),
                ("date".to_owned(), "1700 & 1701".to_owned())
            ]
        );
    }

    #[test]
    fn picks_picture_and_thumbnail_sizes() {
        let info = image_info(
            r#"{"width": 3352, "height": 2248,
                "sizes": [{"width": 105, "height": 70}, {"width": 210, "height": 141}, {"width": 419, "height": 281}],
                "profile": ["http://iiif.io/api/image/2/level2.json", {}]}"#,
        )
        .unwrap();
        assert_eq!(info.picture_size(), "!2048,2048");
        assert_eq!(info.thumbnail_size(), "210,141");

        let level0 = image_info(r#"{"width": 1000, "height": 800, "profile": "http://iiif.io/api/image/2/level0.json"}"#)
            .unwrap();
        assert_eq!(level0.picture_size(), "full");
        assert_eq!(level0.thumbnail_size(), "full");
        assert!(image_info(r#"{"width": 0, "height": 10}"#).is_err());
    }
}
