//! United Kingdom: Great Britain from the ONS Index of Place Names, Northern
//! Ireland from Wikidata, each place under its historic county and under
//! today's ceremonial county.

use std::io::Read;

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Nation, Place, Region};
use crate::table::{Table, decode_mixed};

/// The ONS Open Geography Portal publishes each edition of the Index of Place
/// Names as its own item; searching them is how the latest is found.
const ARCGIS_URL: &str = "https://www.arcgis.com/sharing/rest";
const IPN_SEARCH: &str =
    r#"title:"Index of Place Names" AND owner:ONSGeography_data AND type:"CSV Collection""#;
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Settlements and civil parishes of the six counties of Northern Ireland.
/// Those counties are both the historic and the ceremonial ones.
const WIKIDATA_NORTHERN_IRELAND: &str = r#"
SELECT ?place ?label ?type ?countyLabel ?coord WHERE {
  VALUES ?county { wd:Q189592 wd:Q190678 wd:Q190684 wd:Q192208 wd:Q192229 wd:Q192761 }
  VALUES ?type { wd:Q515 wd:Q3957 wd:Q532 wd:Q5084 wd:Q486972 wd:Q188509 wd:Q3910694 }
  ?place wdt:P7959 ?county ; wdt:P31 ?type ; rdfs:label ?label .
  FILTER(LANG(?label) = "en")
  OPTIONAL { ?place wdt:P625 ?coord }
  ?county rdfs:label ?countyLabel . FILTER(LANG(?countyLabel) = "en")
}"#;
const CIVIL_PARISH: &str = "http://www.wikidata.org/entity/Q3910694";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let mut places = great_britain(fetcher).await?;
    let britain = places.len();
    places.extend(northern_ireland(fetcher).await?);
    eprintln!(
        "United Kingdom: {britain} rows from the ONS, {} from Wikidata",
        places.len() - britain
    );
    Ok(places)
}

/// The newest edition's item id and title.
async fn latest_ipn(fetcher: &Fetcher) -> Result<(String, String)> {
    let url = format!(
        "{ARCGIS_URL}/search?f=json&num=100&q={}",
        IPN_SEARCH.replace(' ', "%20").replace('"', "%22")
    );
    let bytes = fetcher.bytes("ipn-editions.json", &url).await?;
    let found: serde_json::Value = serde_json::from_slice(&bytes)?;
    let items = found["results"]
        .as_array()
        .context("the portal search returned no results list")?;
    items
        .iter()
        .filter_map(|item| {
            let title = item["title"].as_str()?;
            if title.contains("User Guide") {
                return None;
            }
            Some((ipn_edition(title)?, item["id"].as_str()?, title))
        })
        .max_by_key(|(edition, _, _)| *edition)
        .map(|(_, id, title)| (id.to_string(), title.to_string()))
        .context("no edition of the Index of Place Names was found")
}

/// (year, month) of a title such as "Index of Place Names (July 2024) in GB".
fn ipn_edition(title: &str) -> Option<(u16, usize)> {
    let (_, rest) = title.split_once('(')?;
    let (date, _) = rest.split_once(')')?;
    let (month, year) = date.split_once(' ')?;
    Some((year.parse().ok()?, MONTHS.iter().position(|m| *m == month)?))
}

async fn great_britain(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let (item, title) = latest_ipn(fetcher).await?;
    eprintln!("United Kingdom: {title}");
    let archive = fetcher
        .bytes(
            &format!("ipn-{item}.zip"),
            &format!("{ARCGIS_URL}/content/items/{item}/data"),
        )
        .await?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    let name = archive
        .file_names()
        .find(|n| n.ends_with(".csv"))
        .context("the IPN archive holds no CSV")?
        .to_string();
    let mut bytes = Vec::new();
    archive.by_name(&name)?.read_to_end(&mut bytes)?;
    let table = Table::parse(&decode_mixed(&bytes), ',')?;

    let (name, kind, historic, ceremonial, nation, latitude, longitude) = (
        table.dated_column("place", "nm")?,
        table.column("descnm")?,
        table.column("ctyhistnm")?,
        table.column("ctyltnm")?,
        table.dated_column("ctry", "nm")?,
        table.column("lat")?,
        table.column("long")?,
    );
    let mut places = Vec::new();
    for row in &table.rows {
        // Localities and built-up areas are the places; parishes and Welsh
        // communities are what parish registers were kept by. Wards,
        // districts and the counties themselves are not places one is born in.
        let kind = match row[kind].as_str() {
            "LOC" | "BUA" => Kind::Settlement,
            // "Sheffield, unparished area" is the part of a district no
            // parish covers, not a parish.
            "PAR" | "COM" if !row[name].ends_with("unparished area") => Kind::Parish,
            _ => continue,
        };
        let Some(nation) = Nation::from_english(&row[nation]) else {
            continue;
        };
        let coordinates = match (row[latitude].parse(), row[longitude].parse()) {
            (Ok(latitude), Ok(longitude)) => Some(Coordinates {
                latitude,
                longitude,
            }),
            _ => None,
        };
        let base = Place {
            name: ipn_name(&row[name], kind),
            code: String::new(),
            subdivision: String::new(),
            region: Region::Nation(nation),
            country: Country::UnitedKingdom,
            kind,
            valid_from: None,
            valid_until: None,
            successor: None,
            coordinates,
        };
        for county in [&row[historic], &row[ceremonial]] {
            if !county.is_empty() {
                places.push(Place {
                    subdivision: county.clone(),
                    ..base.clone()
                });
            }
        }
    }
    Ok(places)
}

/// The IPN files a locality under its main word, "Haddlesey, East" or
/// "Dell, The", and tells homonyms apart with a parenthesis,
/// "Aberdour (Fife)", which the county column already does. A parish name
/// may hold a real comma ("Cromdale, Inverallan and Advie").
fn ipn_name(raw: &str, kind: Kind) -> String {
    let name = match raw.rsplit_once(" (") {
        Some((name, rest)) if rest.ends_with(')') => name,
        _ => raw,
    };
    match name.split_once(", ") {
        Some((head, qualifier)) if kind == Kind::Settlement && !qualifier.contains(", ") => {
            format!("{qualifier} {head}")
        }
        _ => name.to_string(),
    }
}

async fn northern_ireland(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let table = fetcher.sparql(WIKIDATA_NORTHERN_IRELAND).await?;
    let (label, kind, county, coord) = (
        table.column("label")?,
        table.column("type")?,
        table.column("countyLabel")?,
        table.column("coord")?,
    );
    Ok(table
        .rows
        .iter()
        .map(|row| Place {
            name: row[label].clone(),
            code: String::new(),
            subdivision: row[county]
                .strip_prefix("County ")
                .unwrap_or(&row[county])
                .to_string(),
            region: Region::Nation(Nation::NorthernIreland),
            country: Country::UnitedKingdom,
            kind: if row[kind] == CIVIL_PARISH {
                Kind::Parish
            } else {
                Kind::Settlement
            },
            valid_from: None,
            valid_until: None,
            successor: None,
            coordinates: Coordinates::from_wkt(&row[coord]),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{ipn_edition, ipn_name};
    use crate::place::Kind;

    #[test]
    fn reads_the_edition_from_a_title() {
        assert_eq!(
            ipn_edition("Index of Place Names  (July 2024) in GB"),
            Some((2024, 6))
        );
        assert_eq!(ipn_edition("Index of Place Names in GB"), None);
    }

    #[test]
    fn ipn_names_are_written_as_they_are_said() {
        assert_eq!(ipn_name("Dell, The", Kind::Settlement), "The Dell");
        assert_eq!(
            ipn_name("Village, East (Parish A)", Kind::Settlement),
            "East Village"
        );
        assert_eq!(
            ipn_name("Village A (Shire B)", Kind::Settlement),
            "Village A"
        );
        assert_eq!(
            ipn_name("Parish A, Parish B and Parish C", Kind::Parish),
            "Parish A, Parish B and Parish C"
        );
    }
}
