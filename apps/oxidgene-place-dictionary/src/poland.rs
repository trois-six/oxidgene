//! Poland: the towns and rural communes (gminy) of the TERYT register kept
//! by Statistics Poland, each under its powiat and voivodeship.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, unfiled};
use crate::table::Table;
use crate::wikidata::coordinates;

/// The register's download page: an ASP.NET form whose button posts back
/// for the file.
const TERYT_URL: &str = "https://eteryt.stat.gov.pl/eTeryt/rejestr_teryt/udostepnianie_danych/baza_teryt/uzytkownicy_indywidualni/pobieranie/pliki_pelne.aspx";
const TERC_BUTTON: &str = "ctl00$body$BTERCUrzedowyPobierz";
/// Wikidata's property for the TERYT municipality code.
const CODE_PROPERTY: &str = "P1653";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let terc = terc(fetcher).await?;
    let (voivodeship, powiat, gmina, kind, name, detail) = (
        terc.column("WOJ")?,
        terc.column("POW")?,
        terc.column("GMI")?,
        terc.column("RODZ")?,
        terc.column("NAZWA")?,
        terc.column("NAZWA_DOD")?,
    );
    let mut voivodeships = HashMap::new();
    let mut powiats = HashMap::new();
    let mut units = Vec::new();
    for row in &terc.rows {
        match (row[powiat].is_empty(), row[gmina].is_empty()) {
            (true, true) => {
                voivodeships.insert(row[voivodeship].clone(), capitalized(&row[name]));
            }
            (false, true) => {
                // A city with powiat rights is its own powiat.
                let powiat_name = if row[detail].starts_with("powiat") {
                    format!("powiat {}", row[name])
                } else {
                    String::new()
                };
                powiats.insert(format!("{}{}", row[voivodeship], row[powiat]), powiat_name);
            }
            _ => units.push(row),
        }
    }

    // Towns are urban gminy (1) and the towns of urban-rural ones (4); a
    // rural gmina (2) is named after its seat, which is a place of its own
    // unless a town of that name already stands in the powiat. The
    // urban-rural gmina (3), its rural part (5) and city districts (8, 9)
    // name no further place.
    let towns: HashSet<(String, &str)> = units
        .iter()
        .filter(|r| matches!(r[kind].as_str(), "1" | "4"))
        .map(|r| (format!("{}{}", r[voivodeship], r[powiat]), r[name].as_str()))
        .collect();
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    let mut places = Vec::new();
    for row in units {
        let county = format!("{}{}", row[voivodeship], row[powiat]);
        match row[kind].as_str() {
            "1" | "4" => {}
            "2" if !towns.contains(&(county.clone(), row[name].as_str())) => {}
            _ => continue,
        }
        let code = format!("{county}{}{}", row[gmina], row[kind]);
        let mut base = unfiled(Country::Poland, &row[name], &code, Kind::Commune);
        base.coordinates = centres.get(&code).copied();
        let powiat_name = powiats.get(&county).cloned().unwrap_or_default();
        let region = voivodeships
            .get(&row[voivodeship])
            .with_context(|| format!("unknown voivodeship {}", row[voivodeship]))?;
        file(&mut places, &base, &powiat_name, region, true);
    }
    eprintln!("Poland: {} towns and communes from TERYT", places.len());
    Ok(places)
}

/// The TERC file, from the register's form: the page is fetched for its
/// view state, then posted back as if its button had been pressed.
async fn terc(fetcher: &Fetcher) -> Result<Table> {
    let page = fetcher.bytes("teryt-form.html", TERYT_URL).await?;
    let page = String::from_utf8_lossy(&page);
    let mut fields: Vec<(String, String)> = hidden_fields(&page);
    fields.push(("__EVENTTARGET".to_string(), TERC_BUTTON.to_string()));
    fields.push(("__EVENTARGUMENT".to_string(), String::new()));
    let fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let archive = fetcher
        .post_form("teryt-terc.zip", TERYT_URL, &fields)
        .await?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .context("the TERYT form did not answer with an archive")?;
    let entry = archive
        .file_names()
        .find(|n| n.ends_with(".csv"))
        .context("the TERC archive holds no CSV")?
        .to_string();
    let mut text = String::new();
    archive.by_name(&entry)?.read_to_string(&mut text)?;
    Table::parse(&text, ';').with_context(|| format!("cannot read {entry}"))
}

/// The `(name, value)` of every hidden input of an HTML form.
fn hidden_fields(html: &str) -> Vec<(String, String)> {
    html.split("<input")
        .skip(1)
        .filter_map(|tag| {
            let tag = &tag[..tag.find('>')?];
            if !tag.contains("type=\"hidden\"") {
                return None;
            }
            Some((attribute(tag, "name")?, unescape(&attribute(tag, "value")?)))
        })
        .collect()
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let at = tag.find(&key)? + key.len();
    Some(tag[at..at + tag[at..].find('"')?].to_string())
}

fn unescape(text: &str) -> String {
    text.replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// "KUJAWSKO-POMORSKIE" as Kujawsko-Pomorskie.
fn capitalized(name: &str) -> String {
    name.to_lowercase()
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voivodeships_are_written_capitalised() {
        assert_eq!(capitalized("KUJAWSKO-POMORSKIE"), "Kujawsko-Pomorskie");
        assert_eq!(capitalized("ŁÓDZKIE"), "Łódzkie");
    }

    #[test]
    fn reads_the_hidden_fields_of_a_form() {
        let html = r#"<input type="hidden" name="__VIEWSTATE" id="v" value="a&amp;b" /><input type="text" name="q" value="x" />"#;
        assert_eq!(
            hidden_fields(html),
            [("__VIEWSTATE".to_string(), "a&b".to_string())]
        );
    }
}
