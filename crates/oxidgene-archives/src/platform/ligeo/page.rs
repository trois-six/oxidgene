//! Reading a Ligeo portal's answers: the results page and the register's
//! IIIF manifest.
//!
//! The results page is an HTML table, `table#resultats`: a header row
//! (`tr.entete`) and one row per register (`tr.pair`, `tr.impair`). Which
//! columns it has depends on the portal, so each is found by its header text;
//! the viewer link of a row (`/ark:/<naan>/<id>/<tag>/<group>/layout:table…`)
//! carries the register's address and, in its `title`, the image count.

use serde::Deserialize;

use super::Columns;
use crate::ResolveError;
use crate::citation::CitationGrammar;
use crate::platform::markup::{
    attribute, attributes, first_number, fold, is_challenge, split_after, strip_tags, text_after,
};
use crate::platform::select::{Candidate, number_range};

fn unexpected(detail: &str) -> ResolveError {
    ResolveError::UnexpectedResponse(format!("ligeo: {detail}"))
}

/// What the adapter keeps of a register to open it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Register {
    /// `/ark:/<naan>/<id>`.
    ark: String,
    /// `<tag>/<group>`: `daogrp/0`, or `daoloc/0` for some collections.
    media: String,
}

impl Register {
    /// The register's viewer path, which a view number follows.
    pub(super) fn viewer(&self) -> String {
        format!("{}/{}", self.ark, self.media)
    }

    /// The register's IIIF Presentation manifest.
    pub(super) fn manifest(&self) -> String {
        format!("{}/manifest", self.ark)
    }

    /// Reads a viewer link: `/ark:/<naan>/<id>/<tag>/<group>`, followed by
    /// `/layout:…` on a results page.
    pub(super) fn parse(href: &str) -> Option<Self> {
        let path = href.split("/layout:").next()?;
        let segments: Vec<_> = path.strip_prefix("/ark:/")?.split('/').collect();
        let [naan, id, tag, group] = segments[..] else {
            return None;
        };
        let valid = is_digits(naan)
            && is_name(id)
            && !tag.is_empty()
            && tag.bytes().all(|byte| byte.is_ascii_lowercase())
            && is_digits(group);
        valid.then(|| Self {
            ark: format!("/ark:/{naan}/{id}"),
            media: format!("{tag}/{group}"),
        })
    }
}

/// A non-empty run of ASCII digits.
fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// A non-empty ARK name: letters, digits, `.`, `_` and `-`.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

/// The rows of a results page, and the count the page announces.
pub(super) struct Found {
    pub(super) total: Option<usize>,
    pub(super) rows: Vec<Candidate<Option<Register>>>,
}

/// The results of a search. A page without the results container is not a
/// Ligeo page: an anti-bot challenge when it bears a known signature, a
/// changed shape otherwise.
pub(super) fn results(answer: &str, columns: &Columns) -> Result<Found, ResolveError> {
    if !answer.contains("id=\"arc_liste_update\"") {
        return Err(if is_challenge(answer) {
            ResolveError::Challenged
        } else {
            unexpected("the answer is not a results page")
        });
    }
    let total = text_after(answer, "nb_reponses\"><span>")
        .or_else(|| text_after(answer, "arc_nbr_reponses\">"))
        .and_then(|text| first_number(&text));
    // No table is no register: the portal shows its help text instead.
    let Some((_, table)) = answer.split_once("id=\"resultats\"") else {
        return Ok(Found {
            total: total.or(Some(0)),
            rows: Vec::new(),
        });
    };
    let table = table.split("</table>").next().unwrap_or(table);

    let mut layout = None;
    let mut rows = Vec::new();
    for fragment in split_after(table, "<tr class=\"") {
        if fragment.starts_with("entete") {
            layout = Some(Layout::read(fragment, columns)?);
        } else if fragment.starts_with("pair") || fragment.starts_with("impair") {
            let layout = layout
                .as_ref()
                .ok_or_else(|| unexpected("rows come before the table header"))?;
            rows.push(layout.row(fragment));
        }
    }
    Ok(Found { total, rows })
}

/// The position of each configured column.
struct Layout {
    locality: Option<usize>,
    title: Option<usize>,
    acts: Option<usize>,
    parish: Option<usize>,
    period: Option<usize>,
    call_number: Option<usize>,
    numbers: Option<usize>,
}

impl Layout {
    fn read(header: &str, columns: &Columns) -> Result<Self, ResolveError> {
        let headers: Vec<String> = header
            .split("<th")
            .skip(1)
            .map(|cell| {
                fold(&strip_tags(
                    cell.split_once('>').map_or("", |(_, rest)| rest),
                ))
            })
            .collect();
        let find = |name: &Option<String>| -> Result<Option<usize>, ResolveError> {
            let Some(name) = name else {
                return Ok(None);
            };
            let wanted = fold(name);
            headers
                .iter()
                .position(|header| *header == wanted)
                .map(Some)
                .ok_or_else(|| unexpected("the results table lacks a configured column"))
        };
        Ok(Self {
            locality: find(&columns.locality)?,
            title: find(&columns.title)?,
            acts: find(&columns.acts)?,
            parish: find(&columns.parish)?,
            period: find(&columns.period)?,
            call_number: find(&columns.call_number)?,
            numbers: find(&columns.numbers)?,
        })
    }

    fn row(&self, html: &str) -> Candidate<Option<Register>> {
        let cells: Vec<String> = html
            .split("<td")
            .skip(1)
            .map(|cell| {
                let body = cell.split_once('>').map_or("", |(_, rest)| rest);
                strip_tags(body.split("</td>").next().unwrap_or(body))
            })
            .collect();
        let cell = |index: Option<usize>| {
            index
                .and_then(|index| cells.get(index))
                .filter(|text| !text.is_empty())
                .cloned()
        };
        let link = viewer_link(html);
        let label = link.as_ref().and_then(|link| link.label.as_deref());

        let title = cell(self.title);
        let (locality, parish) = match &title {
            Some(title) => {
                let locality = title_locality(title);
                let parish = title_parish(title, &locality);
                (Some(locality), parish)
            }
            None => (
                cell(self.locality).map(|text| clean_locality(&text)),
                cell(self.parish),
            ),
        };
        let act = match (&title, cell(self.acts)) {
            (Some(title), _) => act_code(title, false),
            (None, Some(acts)) => act_code(&acts, true),
            // The link's own title names the register where there is no
            // act column (`BMS` on parish registers).
            (None, None) => label.and_then(|label| act_code(label, true)),
        };
        let call_number =
            cell(self.call_number).or_else(|| title.as_deref().and_then(title_call_number));
        // The numbers a register spans, from their own column, or after a
        // range word in the title or the link's label (`n° 1 à 1586`).
        let numbers = match cell(self.numbers) {
            Some(numbers) => number_range(&numbers, false),
            None => title
                .as_deref()
                .and_then(|title| number_range(title, true))
                .or_else(|| label.and_then(|label| number_range(label, true))),
        };

        Candidate {
            locality,
            call_number,
            act,
            parish,
            period: cell(self.period),
            images: link.as_ref().and_then(|link| link.images),
            numbers,
            payload: link.map(|link| link.register),
        }
    }
}

/// A row's viewer link.
struct ViewerLink {
    register: Register,
    images: Option<u16>,
    /// What the link's `title` names after the image count.
    label: Option<String>,
}

/// The first viewer link of a row, with what its `title` shows:
/// `120 vues  dont 104 indexées - <label> (ouvre la visionneuse)`.
fn viewer_link(row: &str) -> Option<ViewerLink> {
    let href = attributes(row, "href")
        .into_iter()
        .find(|href| href.starts_with("/ark:/") && Register::parse(href).is_some())?;
    let register = Register::parse(&href)?;
    let from_link = &row[row.find(&format!("href=\"{href}"))?..];
    let title = attribute(from_link, "title").unwrap_or_default();
    let images = title
        .contains("vue")
        .then(|| first_number(&title))
        .flatten();
    let label = title.split_once(" - ").map(|(_, label)| {
        label
            .trim_end_matches("(ouvre la visionneuse)")
            .trim()
            .to_owned()
    });
    Some(ViewerLink {
        register,
        images,
        label,
    })
}

/// A locality cell without the thesaurus qualifier some portals add:
/// `Exampleville (commune ; Exampledept, France)`.
fn clean_locality(text: &str) -> String {
    text.split(" (")
        .next()
        .and_then(|name| name.split(" -- ").next())
        .unwrap_or(text)
        .trim()
        .to_owned()
}

/// The locality a title begins with: up to the first ` : `, `, ` or `. `
/// (`Exampleville. 1 E 1 registre paroissial : …`).
fn title_locality(title: &str) -> String {
    let end = [" : ", ", ", ". "]
        .iter()
        .filter_map(|separator| title.find(separator))
        .min()
        .unwrap_or(title.len());
    title[..end].trim().to_owned()
}

/// The parish a title names after its locality: `Muret : Saint-Jacques,
/// paroisse de Muret : …` and `Muret, paroisse de Saint-Jacques. …` both give
/// `Saint-Jacques`.
fn title_parish(title: &str, locality: &str) -> Option<String> {
    let rest = title.get(locality.len()..)?;
    let parish = if let Some(after) = rest.strip_prefix(" : ") {
        let segment = after.split([',', ':']).next()?.trim();
        // `Muret : baptêmes, mariages` names acts, not a parish.
        act_code(segment, false).is_none().then_some(segment)?
    } else {
        let after = rest.split_once("paroisse de ")?.1;
        after.split(['.', ':']).next()?.trim()
    };
    (!parish.is_empty()).then(|| parish.to_owned())
}

/// The call number a title carries after its first sentence, up to the
/// first comma or the first word that is not one:
/// `Muret, paroisse de Saint-Jacques. 1 GG 8, registre paroissial : …`.
fn title_call_number(title: &str) -> Option<String> {
    let after = title.split_once(". ")?.1;
    let mut words = Vec::new();
    for word in after.split(' ') {
        let ends = word.ends_with(',');
        let word = word.trim_end_matches(',');
        let shaped = word
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
            && word.bytes().all(|byte| {
                byte.is_ascii_digit() || byte.is_ascii_uppercase() || b"/._-".contains(&byte)
            });
        if !shaped {
            break;
        }
        words.push(word);
        if ends {
            break;
        }
    }
    let call_number = words.join(" ");
    call_number
        .bytes()
        .any(|byte| byte.is_ascii_digit())
        .then_some(call_number)
}

/// The act of a register as the codes `select` reads: the series its text
/// names in the citation vocabulary (`RP` for `Recensement de population`),
/// the kinds its text names (`BMS`), or `TD` for decennial tables. With
/// `codes`, a word made of act letters (`N`, `B, M, S`, `BMS`) names kinds
/// too; a title would take an initial for one.
pub(super) fn act_code(text: &str, codes: bool) -> Option<String> {
    if let Some(series) = CitationGrammar::default().series_of(text) {
        return Some(series.code().to_owned());
    }
    let folded = fold(text);
    let words: Vec<&str> = folded.split(' ').collect();
    if words.iter().any(|word| word.starts_with("decennal")) {
        return Some("TD".to_owned());
    }
    let mut kinds = String::new();
    let mut add = |letter: char| {
        if !kinds.contains(letter) {
            kinds.push(letter);
        }
    };
    for word in &words {
        match *word {
            word if word.starts_with("baptem") => add('B'),
            word if word.starts_with("mariage") => add('M'),
            word if word.starts_with("naissance") => add('N'),
            "deces" => add('D'),
            word if word.starts_with("sepultur") => add('S'),
            _ => {}
        }
    }
    if codes {
        for token in text.split(|c: char| !c.is_alphanumeric()) {
            if !token.is_empty() && token.chars().all(|c| "NBMDS".contains(c)) {
                token.chars().for_each(&mut add);
            }
        }
    }
    if !kinds.is_empty() {
        return Some(kinds);
    }
    words.iter().any(|word| word.starts_with("tabl")).then(|| {
        if words.iter().any(|word| word.starts_with("annuel")) {
            "TA".to_owned()
        } else {
            "TD".to_owned()
        }
    })
}

/// The path of an absolute address, without query or fragment.
pub(super) fn path_of(address: &str) -> Option<&str> {
    let after_scheme = address.find("://")? + 3;
    let path = &address[address[after_scheme..].find('/')? + after_scheme..];
    (!path.contains(['?', '#', ' ', '\\']) && !path.starts_with("//")).then_some(path)
}

/// One canvas of a register's manifest: one view. Its declared size is not
/// its image's, which the image service's `info.json` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Canvas {
    /// The view's own persistent address, when the manifest gives one.
    pub(super) ark: Option<String>,
    /// The image service's address.
    pub(super) service: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    sequences: Vec<Sequence>,
}

#[derive(Deserialize)]
struct Sequence {
    canvases: Vec<RawCanvas>,
}

#[derive(Deserialize)]
struct RawCanvas {
    width: u32,
    height: u32,
    #[serde(rename = "ligeoPermalink", default)]
    permalink: Option<String>,
    #[serde(default)]
    images: Vec<Annotation>,
}

#[derive(Deserialize)]
struct Annotation {
    resource: Resource,
}

#[derive(Deserialize)]
struct Resource {
    #[serde(default)]
    service: Option<Service>,
}

#[derive(Deserialize)]
struct Service {
    #[serde(rename = "@id")]
    id: String,
}

/// The canvases of a register's manifest, in order. Nothing else of it is
/// read: its renderings and file names name server paths.
pub(super) fn manifest(answer: &str) -> Result<Vec<Canvas>, ResolveError> {
    let manifest: Manifest = serde_json::from_str(answer)
        .map_err(|_| unexpected("the manifest lacks sequences[].canvases"))?;
    let canvases = manifest
        .sequences
        .into_iter()
        .next()
        .map(|sequence| sequence.canvases)
        .unwrap_or_default();
    if canvases.is_empty()
        || canvases
            .iter()
            .any(|canvas| canvas.width == 0 || canvas.height == 0)
    {
        return Err(unexpected("the manifest lists no sized canvas"));
    }
    Ok(canvases
        .into_iter()
        .map(|canvas| Canvas {
            ark: canvas.permalink,
            service: canvas
                .images
                .into_iter()
                .next()
                .and_then(|annotation| annotation.resource.service)
                .map(|service| service.id),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_names_its_locality_first() {
        for (title, locality) in [
            (
                "Exampleville : baptêmes, mariages, sépultures",
                "Exampleville",
            ),
            (
                "Exampleville, paroisse de Saint-Exemple. 1 GG 8",
                "Exampleville",
            ),
            // A communal register, its call number after a full stop.
            (
                "Exampleville. 1 E 1 registre paroissial : baptêmes",
                "Exampleville",
            ),
            ("Exampleville-sur-Mer", "Exampleville-sur-Mer"),
        ] {
            assert_eq!(title_locality(title), locality, "{title}");
        }
        assert_eq!(
            title_call_number("Exampleville. 1 E 1 registre paroissial : baptêmes").as_deref(),
            Some("1 E 1")
        );
    }
}
