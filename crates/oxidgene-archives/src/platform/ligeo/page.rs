//! Reading a Ligeo portal's answers: the results page and the register's
//! IIIF manifest.
//!
//! The results are a table, `table#resultats`, whose header row
//! (`tr.entete`) names the columns of the rows (`tr.pair`, `tr.impair`), or
//! a list of notices whose items carry their own labels. Which columns or
//! items a search shows depends on the portal, so each is found by its
//! header text or label; the viewer link of a row
//! (`/ark:/<naan>/<id>/<tag>/<group>/layout:table…`) carries the register's
//! address and, in its `title`, the image count.

use serde::Deserialize;

use super::place::{Place, places};
use super::settings::{Columns, Names};
use crate::ResolveError;
use crate::citation::{CallNumber, CitationGrammar};
use crate::platform::markup::{
    attribute, attributes, first_number, fold, split_after, strip_tags, text_after, unreadable,
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

    /// Reads a viewer link: `/ark:/<naan>/<id>/<tag>/<group>`, followed on
    /// a results page by named segments (`/layout:table`, `/idsearch:…`).
    pub(super) fn parse(href: &str) -> Option<Self> {
        let segments: Vec<_> = href
            .strip_prefix("/ark:/")?
            .split('/')
            .take_while(|segment| !segment.contains(':'))
            .collect();
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

/// What the adapter keeps of a result row besides what selection compares:
/// the register to open, and every place and parish the row names, against
/// which the cited ones are matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    /// `None` for a register listed without a viewer link: not digitised.
    pub(super) register: Option<Register>,
    pub(super) places: Vec<Place>,
    pub(super) parishes: Vec<String>,
}

/// The rows of a results page, and the count the page announces.
pub(super) struct Found {
    pub(super) total: Option<usize>,
    pub(super) rows: Vec<Candidate<Row>>,
}

/// The results of a search. A page without the results container is not a
/// Ligeo page: an anti-bot challenge when it bears a known signature, a
/// changed shape otherwise. The results are a table whose header names the
/// columns (`tr.entete`), or notices whose items are labelled (`div.items >
/// strong.arc_libelle_strong`), in a list (`tr.arc_pair`, `tr.arc_impair`)
/// or in a finding aid (`li.arc_notice`).
pub(super) fn results(answer: &str, columns: &Columns) -> Result<Found, ResolveError> {
    // A search's results, or a finding aid's notices.
    let container = answer
        .find("id=\"arc_liste_update\"")
        .or_else(|| answer.find("id=\"arc_fonds_notice\""));
    let Some(start) = container else {
        return Err(unreadable(
            answer,
            "ligeo: the answer is not a results page".to_owned(),
        ));
    };
    let list = &answer[start..];
    let list = list.split("<!-- fin-res -->").next().unwrap_or(list);
    let total = text_after(list, "nb_reponses\"><span>")
        .or_else(|| text_after(list, "arc_nbr_reponses\">"))
        .and_then(|text| first_number(&text));
    let table = list
        .split_once("id=\"resultats\"")
        .map(|(_, table)| table.split("</table>").next().unwrap_or(table));
    let rows = match table {
        Some(table) if table.contains("<tr class=\"entete") => table_rows(table, columns)?,
        Some(table) if table.contains("<tr class=\"arc_") => {
            notice_rows(&split_after(table, "<tr class=\"arc_"), columns)
        }
        _ if list.contains("<li class=\"arc_notice") => {
            notice_rows(&split_after(list, "<li class=\"arc_notice"), columns)
        }
        // No result is no register: the portal shows its help text instead.
        _ => {
            return Ok(Found {
                total: total.or(Some(0)),
                rows: Vec::new(),
            });
        }
    };
    Ok(Found { total, rows })
}

/// The texts of a row, by folded header or label.
struct Cells(Vec<(String, String)>);

impl Cells {
    /// The texts of the cells `names` names, each once, joined: the acts of a
    /// document-type column and of an act column read together.
    fn get(&self, names: Option<&Names>) -> Option<String> {
        let mut texts: Vec<&str> = Vec::new();
        for name in names?.list() {
            let wanted = fold(name);
            for (_, text) in self.0.iter().filter(|(name, _)| *name == wanted) {
                if !text.is_empty() && !texts.contains(&text.as_str()) {
                    texts.push(text);
                }
            }
        }
        (!texts.is_empty()).then(|| texts.join(" ; "))
    }
}

/// A cell's text: tags stripped, and the dashes and bullets that portals
/// put around a heading's parts trimmed (`2 E 1/1 - `, ` • 1843 1852`).
fn cell_text(html: &str) -> String {
    strip_tags(html)
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '\u{2022}' | ':'))
        .to_owned()
}

/// The rows of a table, each cell named by its column's header. A column the
/// settings name that the table lacks is a changed shape.
fn table_rows(table: &str, columns: &Columns) -> Result<Vec<Candidate<Row>>, ResolveError> {
    let mut headers: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    for fragment in split_after(table, "<tr class=\"") {
        if fragment.starts_with("entete") {
            let names: Vec<String> = fragment
                .split("<th")
                .skip(1)
                .map(|cell| {
                    fold(&cell_text(
                        cell.split_once('>').map_or("", |(_, rest)| rest),
                    ))
                })
                .collect();
            if !columns
                .all()
                .all(|column| column.list().iter().any(|name| names.contains(&fold(name))))
            {
                return Err(unexpected("the results table lacks a configured column"));
            }
            headers = Some(names);
        } else if ["pair", "impair", "arc_pair", "arc_impair"]
            .iter()
            .any(|class| fragment.starts_with(class))
        {
            let headers = headers
                .as_ref()
                .ok_or_else(|| unexpected("rows come before the table header"))?;
            let cells = fragment.split("<td").skip(1).map(|cell| {
                let body = cell.split_once('>').map_or("", |(_, rest)| rest);
                cell_text(body.split("</td>").next().unwrap_or(body))
            });
            let cells = Cells(headers.iter().cloned().zip(cells).collect());
            rows.push(candidate(&cells, columns, fragment));
        }
    }
    Ok(rows)
}

/// The classes of a notice's heading parts, read as cells of those names:
/// `span.cote`, `span.unittitle`, `span.date`.
const HEADING_PARTS: [&str; 3] = ["cote", "unittitle", "date"];

/// The rows of a list of notices. Each item names its own cell
/// (`<strong class="arc_libelle_strong">Commune : </strong>…`); the heading's
/// parts are cells named by their class, and the whole heading
/// (`div.title`) the cell `title`. A notice shows only the items it has, so
/// a missing one is no changed shape.
fn notice_rows(notices: &[&str], columns: &Columns) -> Vec<Candidate<Row>> {
    notices
        .iter()
        .map(|notice| {
            let mut cells = Vec::new();
            for item in split_after(notice, "class=\"arc_libelle_strong\">") {
                let Some((label, rest)) = item.split_once("</strong>") else {
                    continue;
                };
                let value = rest.split("</div>").next().unwrap_or(rest);
                cells.push((fold(&strip_tags(label)), cell_text(value)));
            }
            for part in HEADING_PARTS {
                for span in split_after(notice, &format!("<span class=\"{part}\">")) {
                    cells.push((
                        part.to_owned(),
                        cell_text(span.split("</span>").next().unwrap_or(span)),
                    ));
                }
            }
            if let Some(heading) = split_after(notice, "<div class=\"title\">").first() {
                cells.push((
                    "title".to_owned(),
                    cell_text(heading.split("</div>").next().unwrap_or(heading)),
                ));
            }
            candidate(&Cells(cells), columns, notice)
        })
        .collect()
}

/// A row's candidate: what selection compares, read from the cells the
/// settings name, the viewer link's title standing in for an act or a range
/// of numbers the cells lack.
fn candidate(cells: &Cells, columns: &Columns, html: &str) -> Candidate<Row> {
    let link = viewer_link(html);
    let label = link.as_ref().and_then(|link| link.label.as_deref());
    let title = cells.get(columns.title.as_ref());

    let (places, parishes) = match (cells.get(columns.locality.as_ref()), &title) {
        (Some(locality), _) => {
            let parishes = cells
                .get(columns.parish.as_ref())
                .map(|text| places(&text).into_iter().map(|place| place.name).collect())
                .unwrap_or_default();
            (places(&locality), parishes)
        }
        (None, Some(title)) => {
            let head = title_head(title);
            let locality = title_locality(head);
            let parish = title_parish(head, &locality);
            let mut place = Place::new(&locality);
            place.parish.clone_from(&parish);
            (vec![place], parish.into_iter().collect())
        }
        (None, None) => (Vec::new(), Vec::new()),
    };

    let act = cells
        .get(columns.acts.as_ref())
        .and_then(|acts| act_code(&acts, true))
        .or_else(|| title.as_deref().and_then(|title| act_code(title, false)))
        // The link's own title names the register where nothing else does
        // (`BMS` on parish registers).
        .or_else(|| label.and_then(|label| act_code(label, true)));
    let call_number = cells
        .get(columns.call_number.as_ref())
        .map(|text| call_number_of(&text))
        .filter(|text| !text.is_empty())
        .or_else(|| title.as_deref().and_then(title_call_number))
        // A link whose title names the register by its call number alone
        // (`3 vues - 9 Mi 99`).
        .or_else(|| {
            label
                .filter(|label| is_call_number(label))
                .map(str::to_owned)
        });
    // The numbers a register spans, from their own column, or after a range
    // word in the title or the link's label (`n° 1 à 1586`).
    let numbers = match cells.get(columns.numbers.as_ref()) {
        Some(numbers) => number_range(&numbers, false),
        None => title
            .as_deref()
            .and_then(|title| number_range(title, true))
            .or_else(|| label.and_then(|label| number_range(label, true))),
    };

    Candidate {
        locality: places.first().map(|place| place.name.clone()),
        call_number,
        act,
        parish: parishes
            .first()
            .cloned()
            .or_else(|| places.first().and_then(|place| place.parish.clone())),
        period: cells.get(columns.period.as_ref()),
        images: link.as_ref().and_then(|link| link.images),
        numbers,
        payload: Row {
            register: link.map(|link| link.register),
            places,
            parishes,
        },
    }
}

/// A call number cell up to the heading's next part: `9 E 99 /1` for
/// `9 E 99 /1 - Décès`.
fn call_number_of(text: &str) -> String {
    text.split(" - ")
        .next()
        .unwrap_or(text)
        .trim_end_matches([' ', '-'])
        .to_owned()
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

/// The call number a heading starts with, before its first ` - `: `9 M 99`
/// in `9 M 99 - Exampleville - 1901`. It starts with a digit, unlike a
/// locality and its period (`EXAMPLEVILLE 1746/1753 - Exampleville`).
fn leading_call_number(title: &str) -> Option<(&str, &str)> {
    let (first, rest) = title.split_once(" - ")?;
    (first.starts_with(|c: char| c.is_ascii_digit()) && is_call_number(first))
        .then(|| (first.trim(), rest))
}

/// A title without the call number some headings start with: `Exampleville
/// - 1901` for `9 M 99 - Exampleville - 1901`.
fn title_head(title: &str) -> &str {
    leading_call_number(title).map_or(title, |(_, rest)| rest)
}

/// The locality a title begins with: up to the first ` : `, `, `, `. `,
/// `.- `, ` - ` or ` n°`, or the first word starting with a digit
/// (`Exampleville. 1 E 1 registre paroissial : …`, `Exampleville.- Baptêmes`,
/// `EXAMPLEVILLE 1746/1753 - Exampleville`, `Bureau de Exampleville n° 1 à
/// 500`).
fn title_locality(title: &str) -> String {
    let digit = title
        .match_indices(' ')
        .find(|(at, _)| title[at + 1..].starts_with(|c: char| c.is_ascii_digit()))
        .map(|(at, _)| at);
    let end = [" : ", ", ", ". ", ".- ", " - ", " n°", " N°"]
        .iter()
        .filter_map(|separator| title.find(separator))
        .chain(digit)
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

/// Whether a text is shaped as a citation's call number is (Archive Portals
/// §5.1): `9 E 99`, `1 Mi 912`, not `Exampleville` or `acte 26`.
fn is_call_number(text: &str) -> bool {
    CallNumber::is_shaped(text.trim())
}

/// The call number a title carries: before its first ` - ` (`9 M 99 -
/// Exampleville`), or after its first sentence, up to the first comma or
/// the first word that is not one (`Muret, paroisse de Saint-Jacques. 1 GG
/// 8, registre paroissial : …`).
fn title_call_number(title: &str) -> Option<String> {
    if let Some((call_number, _)) = leading_call_number(title) {
        return Some(call_number.to_owned());
    }
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
