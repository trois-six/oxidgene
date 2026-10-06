//! Reading the Gers portal's search pages: the locality lists of the form,
//! which every answer repeats, the results table, and a viewer's list of
//! views.

use crate::ResolveError;
use crate::platform::markup::{self, first_number, fold, strip_tags};

fn unreadable(page: &str, detail: &str) -> ResolveError {
    markup::unreadable(page, format!("archives32: {detail}"))
}

/// The values of a select of the page (`<select name="lieu"`), its
/// "every locality" choice (`all`) left out, as written: some carry a
/// space the search needs.
pub(super) fn options(page: &str, name: &str) -> Vec<String> {
    let marker = format!("<select name=\"{name}\"");
    let Some(start) = page.find(&marker) else {
        return Vec::new();
    };
    let end = page[start..]
        .find("</select>")
        .map_or(page.len(), |end| start + end);
    markup::split_after(&page[start..end], "<option")
        .into_iter()
        .filter_map(|option| markup::attribute(&option[..option.find('>')?], "value"))
        .filter(|value| value != "all" && !value.trim().is_empty())
        .collect()
}

/// A column of the results table, by its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Locality,
    Former,
    Parish,
    Annex,
    Period,
    CallNumber,
    Content,
    Views,
    Other,
}

impl Column {
    fn of(heading: &str) -> Self {
        match fold(heading).as_str() {
            "commune" | "bureau" => Self::Locality,
            "ancienne commune" => Self::Former,
            "paroisse" => Self::Parish,
            "annexe" => Self::Annex,
            "periode" | "annee" => Self::Period,
            "cote" => Self::CallNumber,
            "contenu" => Self::Content,
            "vues" => Self::Views,
            _ => Self::Other,
        }
    }
}

/// One register the results list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) locality: Option<String>,
    pub(super) former: Option<String>,
    /// The parish, or failing it the annex (`Saint-Exemple (Exampleville -
    /// annexe de Sampleton)`), read up to its parenthesis.
    pub(super) parish: Option<String>,
    pub(super) period: Option<String>,
    pub(super) call_number: Option<String>,
    /// The content's first line: the acts (`Naissances / Mariages /
    /// Décès`), before the notes.
    pub(super) acts: Option<String>,
    pub(super) images: Option<u16>,
    /// The viewer's query, `id=…&fichier=<first view>&lieu=…&annee=…`.
    pub(super) viewer: Option<String>,
}

/// The cells of a table row, each as its markup.
fn cells(row: &str) -> Vec<&str> {
    markup::split_after(row, "<td")
        .into_iter()
        .map(|cell| {
            let content = &cell[cell.find('>').map_or(cell.len(), |end| end + 1)..];
            &content[..content.find("</td>").unwrap_or(content.len())]
        })
        .collect()
}

fn text(cell: &str) -> Option<String> {
    Some(strip_tags(cell)).filter(|text| !text.is_empty() && text != "/")
}

/// The page without its comments, which hide old headings
/// (`<!--<td>Type</td>-->`).
fn uncommented(page: &str) -> String {
    let mut kept = String::with_capacity(page.len());
    let mut rest = page;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    kept.push_str(rest);
    kept
}

/// The rows of a search's answer, read by the headings of its table; `None`
/// for a page holding no results table, the form before any search.
pub(super) fn rows(page: &str) -> Result<Option<Vec<Row>>, ResolveError> {
    let Some(start) = page.find("tableau_td") else {
        return Ok(None);
    };
    let end = page[start..]
        .find("</table>")
        .map_or(page.len(), |end| start + end);
    let table = uncommented(&page[start..end]);
    let mut lines = markup::split_after(&table, "<tr").into_iter();
    let heading = lines
        .next()
        .filter(|row| row.contains("entete"))
        .ok_or_else(|| unreadable(page, "the results table has no headings"))?;
    let columns: Vec<Column> = cells(heading)
        .into_iter()
        .map(|cell| Column::of(&strip_tags(cell)))
        .collect();
    if !columns.contains(&Column::Locality) || !columns.contains(&Column::Views) {
        return Err(unreadable(
            page,
            "the results table has no locality or views column",
        ));
    }
    Ok(Some(
        lines
            .filter_map(|line| {
                let cells = cells(line);
                (cells.len() == columns.len()).then(|| row(&columns, &cells))
            })
            .collect(),
    ))
}

fn row(columns: &[Column], cells: &[&str]) -> Row {
    let mut row = Row::default();
    let mut annex = None;
    for (column, cell) in columns.iter().zip(cells) {
        match column {
            Column::Locality => row.locality = text(cell),
            Column::Former => row.former = text(cell),
            Column::Parish => row.parish = text(cell),
            Column::Annex => {
                annex = text(cell).map(|annex| {
                    annex
                        .split(" (")
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_owned()
                });
            }
            Column::Period => row.period = text(cell),
            Column::CallNumber => row.call_number = text(cell),
            Column::Content => {
                let first = &cell[..cell.find("<br").unwrap_or(cell.len())];
                row.acts = text(first);
            }
            Column::Views => {
                row.images = first_number(&strip_tags(cell));
                row.viewer = markup::attribute(cell, "href")
                    .and_then(|href| href.strip_prefix("../visu/?").map(str::to_owned))
                    .filter(|query| viewer_first(query).is_some());
            }
            Column::Other => {}
        }
    }
    row.parish = row.parish.or(annex);
    row
}

/// The first view's identifier a viewer's query names (`fichier=<id>`).
pub(super) fn viewer_first(query: &str) -> Option<u64> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("fichier="))
        .and_then(|id| id.parse().ok())
}

/// The viewer's query opened on view identifier `id`.
pub(super) fn viewer_at(query: &str, id: u64) -> String {
    query
        .split('&')
        .map(|pair| {
            if pair.starts_with("fichier=") {
                format!("fichier={id}")
            } else {
                pair.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// The identifiers of a viewer's views, in order: its `select#fichier`.
pub(super) fn views(page: &str) -> Result<Vec<u64>, ResolveError> {
    let start = page
        .find("id=\"fichier\"")
        .ok_or_else(|| unreadable(page, "the viewer has no list of views"))?;
    let end = page[start..]
        .find("</select>")
        .map_or(page.len(), |end| start + end);
    // The viewer quotes its values with apostrophes (`value='22994'`).
    let ids: Vec<u64> = markup::split_after(&page[start..end], "<option")
        .into_iter()
        .filter_map(|option| {
            let tag = &option[..option.find('>')?];
            let value = tag[tag.find("value=")? + "value=".len()..].trim_start_matches(['\'', '"']);
            let digits = value
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(value.len());
            value[..digits].parse().ok()
        })
        .collect();
    if ids.is_empty() {
        return Err(unreadable(page, "the viewer lists no view"));
    }
    Ok(ids)
}
