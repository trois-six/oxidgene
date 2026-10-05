//! The `registre` module: a locality list in the search form, and a results
//! fragment that a plain `GET` returns.
//!
//! The form's locality `<select>` maps each commune's name to the portal's
//! identifier; the fragment lists the matching registers as `a.Row` rows
//! whose `span.Cell` cells follow the header's columns. Those differ between
//! portals (Charente-Maritime has collection and observation columns, Oise
//! a parish column), so the header names each cell's role.

use super::{Found, Register, Search, Settings, choose, get, number_after, query, unexpected};
use crate::ResolveError;
use crate::citation::{Act, CitationParts};
use crate::platform::markup::{self, decode_entities, first_number, fold};
use crate::platform::select::Candidate;
use crate::transport::PortalFetch;

pub(super) async fn find(
    settings: &Settings,
    citation: &CitationParts,
    localities: &[&str],
    fetch: &dyn PortalFetch,
) -> Result<Found, ResolveError> {
    let Search::Registre {
        locality,
        act,
        year,
        licence,
    } = &settings.search
    else {
        return Err(unexpected("settings of another search"));
    };
    let form = get(fetch, &format!("{}/registre.html", settings.base)).await?;
    let options = options(&form, locality);
    if options.is_empty() {
        // A licence the reader accepts on the portal's own page stands in
        // front of the form, and is never accepted for them.
        return if *licence {
            Ok(Found::ReaderStep)
        } else {
            Err(unexpected("the search page lists no locality"))
        };
    }
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let Some((commune, _)) = options
        .iter()
        .find(|(_, label)| wanted.contains(&fold(label)))
    else {
        return Ok(Found::Many(0));
    };

    let mut pairs = vec![(locality.as_str(), commune.as_str())];
    if let Some(value) = settings.act_value(&citation.act) {
        pairs.push((act.as_str(), value));
    }
    let year_text = citation.year.map(|year| year.to_string());
    if let Some(text) = &year_text {
        pairs.push((year.as_str(), text.as_str()));
    }
    pairs.push(("ajax", "true"));
    let answer = get(
        fetch,
        &format!("{}/registre_liste.html?{}", settings.base, query(&pairs)),
    )
    .await?;
    let (rows, total) = rows(&answer, &citation.act)?;
    Ok(
        match choose(settings, &rows, citation, localities, fetch).await? {
            Ok(chosen) => Found::One(chosen),
            Err(matches) => Found::Many(matches.max(total)),
        },
    )
}

/// The `(value, label)` of each option of the `<select name="…">` of the
/// form, the empty "choose" option left out.
pub(super) fn options(form: &str, name: &str) -> Vec<(String, String)> {
    // The leading space keeps `data-othername="commune"` from matching.
    let marker = format!(" name=\"{name}\"");
    let Some(at) = form.find(&marker) else {
        return Vec::new();
    };
    let select = &form[at..];
    let select = &select[..select.find("</select>").unwrap_or(select.len())];
    markup::split_after(select, "<option value=\"")
        .into_iter()
        .filter_map(|option| {
            let (value, rest) = option.split_once('"')?;
            let label = rest.split_once('>')?.1;
            let label = decode_entities(label[..label.find('<').unwrap_or(label.len())].trim());
            (!value.is_empty() && !label.is_empty()).then(|| (value.to_owned(), label))
        })
        .collect()
}

/// What a result column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    CallNumber,
    Locality,
    Act,
    Parish,
    /// The observations column, where one portal writes the parish
    /// (`Paroisse Saint-Exemple`).
    Observations,
    Period,
    Other,
}

fn column(header: &str) -> Column {
    match fold(header).as_str() {
        "cote" => Column::CallNumber,
        "commune" => Column::Locality,
        "actes" => Column::Act,
        "paroisse" => Column::Parish,
        "lacunes observations" => Column::Observations,
        "periode" | "dates extremes" => Column::Period,
        _ => Column::Other,
    }
}

/// The columns of the results header: `<span class="Cell">Cote</span>` ….
fn columns(html: &str) -> Vec<Column> {
    let header = &html[..html.find("<a class='Row").unwrap_or(html.len())];
    markup::split_after(header, "<span class=\"Cell\">")
        .into_iter()
        .map(|cell| {
            let text = cell[..cell.find("</span>").unwrap_or(cell.len())].replace("<br/>", " ");
            column(&decode_entities(&text))
        })
        .collect()
}

/// A parish written as an observation: `Paroisse Saint-Exemple`.
fn observed_parish(text: &str) -> Option<String> {
    let name = text.strip_prefix("Paroisse ")?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// A table of décennales listed among the births, deaths or marriages of a
/// search: its act cell names tables only.
fn is_table_listing(act: &str) -> bool {
    fold(act).starts_with("tables ")
}

/// The registers of a results fragment, and the total it announces. A
/// listing of tables is left out of a search for a register's acts.
pub(super) fn rows(
    html: &str,
    cited: &Act,
) -> Result<(Vec<Candidate<Register>>, usize), ResolveError> {
    if html.contains("Pas de résultat") {
        return Ok((Vec::new(), 0));
    }
    let total = markup::text_after(html, "class=\"total\">")
        .and_then(|text| first_number::<usize>(&text))
        .ok_or_else(|| unexpected("the results lack their total"))?;
    let columns = columns(html);
    if !columns.contains(&Column::CallNumber)
        || !columns.contains(&Column::Locality)
        || !columns.contains(&Column::Period)
    {
        return Err(unexpected("the results header lacks a column"));
    }
    let mut rows = Vec::new();
    for row in markup::split_after(html, "<a class='Row") {
        // A register not yet digitized shows an alert in place of a viewer
        // address; the total still counts it.
        let Some(id) = number_after(row, "?id=") else {
            continue;
        };
        let cells: Vec<String> = markup::split_after(row, "<span class='Cell")
            .into_iter()
            .map(|cell| markup::text_after(cell, "<span>").unwrap_or_default())
            .collect();
        if cells.len() != columns.len() {
            return Err(unexpected("a row does not match the results header"));
        }
        let cell = |wanted: Column| {
            columns
                .iter()
                .position(|column| *column == wanted)
                .map(|at| cells[at].clone())
                .filter(|text| !text.is_empty())
        };
        let act = cell(Column::Act);
        if matches!(cited, Act::Register(_)) && act.as_deref().is_some_and(is_table_listing) {
            continue;
        }
        rows.push(Candidate {
            locality: cell(Column::Locality),
            call_number: cell(Column::CallNumber),
            act,
            parish: cell(Column::Parish)
                .or_else(|| cell(Column::Observations).and_then(|text| observed_parish(&text))),
            period: cell(Column::Period),
            images: None,
            payload: Register { id },
        });
    }
    Ok((rows, total))
}
