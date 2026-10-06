//! Reading the pages of CAOMEC2: a territory's search form, a search's
//! results, and the image viewer of a register.
//!
//! The portal writes XHTML in ISO-8859-1, which the transports decode as
//! UTF-8: an accented letter of a label reads as U+FFFD. Commune names are
//! capitals without accents (`SAINT-EXEMPLE`), so they survive the decoding.

use crate::ResolveError;
use crate::platform::markup::{self, decode_entities, first_number, strip_tags};

fn unreadable(page: &str, detail: &str) -> ResolveError {
    markup::unreadable(page, format!("caomec2: {detail}"))
}

/// The communes the territory's search form lists, as its options write
/// them: `SAINT-EXEMPLE`, `EXAMPLEVILLE (HOPITAL)`.
pub(super) fn communes(page: &str) -> Result<Vec<String>, ResolveError> {
    let start = page
        .find("name=\"commune\"")
        .filter(|at| {
            page[..*at]
                .rfind('<')
                .is_some_and(|tag| page[tag..].starts_with("<select"))
        })
        .ok_or_else(|| unreadable(page, "the search form has no commune list"))?;
    let end = page[start..]
        .find("</select>")
        .map_or(page.len(), |end| start + end);
    Ok(markup::split_after(&page[start..end], "<option")
        .into_iter()
        .filter_map(|option| {
            let tag = &option[..option.find('>')?];
            markup::attribute(tag, "value").filter(|value| !value.trim().is_empty())
        })
        .map(|value| decode_entities(&value).trim().to_owned())
        .collect())
}

/// Whether the search form still has the fields a search sends.
#[cfg(any(test, feature = "live"))]
pub(super) fn has_search_fields(page: &str) -> bool {
    ["name=\"commune\"", "name=\"typeacte\"", "name=\"annee\""]
        .iter()
        .all(|field| page.contains(field))
}

/// One row of a search's results: a register of a commune's year, as one
/// kind of act or all of them (`Tous actes`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) commune: String,
    pub(super) year: String,
    /// The kind of act as labelled: `Naissance`, `Tous actes`.
    pub(super) act: String,
    /// The act type the row's link names, if any (`AC_NA`).
    pub(super) typeacte: Option<String>,
}

/// The text of the row's cell of `class`.
fn cell(row: &str, class: &str) -> Option<String> {
    let marker = format!("<td class=\"{class}\">");
    let start = row.find(&marker)? + marker.len();
    let end = row[start..].find("</td>")? + start;
    Some(strip_tags(&row[start..end])).filter(|text| !text.is_empty())
}

/// The act type a row's link names: `typeacte=AC_NA`.
fn typeacte(row: &str) -> Option<String> {
    let start = row.find("typeacte=")? + "typeacte=".len();
    let value: String = row[start..]
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || *c == '_')
        .collect();
    (!value.is_empty()).then_some(value)
}

/// The rows of a search's first page (twenty at most), each opening the
/// viewer (`<tr onclick="window.location='osd.php…'">`). A page without the
/// count the portal states (`results-nb`) is not the results, nor one
/// counting rows it does not list.
pub(super) fn results(page: &str) -> Result<Vec<Row>, ResolveError> {
    let total = markup::text_after(page, "id=\"results-nb\">")
        .and_then(|text| first_number::<usize>(&text))
        .ok_or_else(|| unreadable(page, "the results have no count"))?;
    let rows = markup::split_after(page, "<tr onclick=")
        .into_iter()
        .filter(|row| row.contains("osd.php?"))
        .filter_map(|row| {
            let row = &row[..row.find("</tr>").unwrap_or(row.len())];
            Some(Row {
                commune: cell(row, "commune")?,
                year: cell(row, "annee")?,
                act: cell(row, "acte")?,
                typeacte: typeacte(row),
            })
        })
        .collect::<Vec<_>>();
    if total > 0 && rows.is_empty() {
        return Err(unreadable(page, "the results list no row"));
    }
    Ok(rows)
}

/// The number of images the viewer of a register lists (`#imgstrip`, one
/// `div#thn<i>` each).
#[cfg(any(test, feature = "live"))]
pub(super) fn image_count(page: &str) -> Result<usize, ResolveError> {
    if !page.contains("id=\"imgstrip\"") {
        return Err(unreadable(page, "the viewer has no image strip"));
    }
    Ok(page.matches("id=\"thn").count())
}
