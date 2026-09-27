//! Portugal: the municipalities (concelhos) and civil parishes (freguesias)
//! of the official administrative map (CAOP) for the mainland, those of the
//! Azores and Madeira and the parishes merged in 2013 from Wikidata, each
//! under its district or autonomous region.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, fold, unfiled};
use crate::wikidata::{FormerQuery, coordinates, former_municipalities};

/// The DGT's OGC API over the CAOP, mainland only.
const CAOP_URL: &str = "https://ogcapi.dgterritorio.gov.pt/collections";
const PAGE: usize = 1000;
/// Wikidata's property for the INE code (DICO for a municipality, DICOFRE
/// for a parish).
const CODE_PROPERTY: &str = "P6324";
const FORMER_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 wd:Q56046844 .";

/// Municipalities and parishes of the autonomous regions, current ones only.
const ISLANDS: &str = r#"
SELECT ?code ?label WHERE {
  ?item wdt:P6324 ?code .
  FILTER(STRSTARTS(?code, "3") || STRSTARTS(?code, "4"))
  FILTER NOT EXISTS { ?item wdt:P31 wd:Q56046844 }
  FILTER NOT EXISTS { ?item wdt:P576 [] }
  ?item rdfs:label ?label . FILTER(LANG(?label) = "pt")
}"#;

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    // Municipality code → (name, district or region).
    let mut municipalities: HashMap<String, (String, String)> = HashMap::new();
    for feature in caop(fetcher, "municipios").await? {
        let p = &feature["properties"];
        let (Some(code), Some(name), Some(district)) = (
            p["dtmn"].as_str(),
            p["municipio"].as_str(),
            p["distrito_ilha"].as_str(),
        ) else {
            continue;
        };
        municipalities.insert(code.to_string(), (name.to_string(), district.to_string()));
    }
    // Parish code → (name, municipality code).
    let mut parishes: HashMap<String, (String, String)> = HashMap::new();
    for feature in caop(fetcher, "freguesias").await? {
        let p = &feature["properties"];
        let (Some(code), Some(name)) = (p["dtmnfr"].as_str(), p["freguesia"].as_str()) else {
            continue;
        };
        parishes.insert(code.to_string(), (name.to_string(), code[..4].to_string()));
    }

    // The islands: Madeira's codes start with 3, the Azores' with 4.
    let islands = fetcher.sparql(ISLANDS).await?;
    let (code, label) = (islands.column("code")?, islands.column("label")?);
    for row in &islands.rows {
        let code = row[code].as_str();
        let region = if code.starts_with('3') {
            "Região Autónoma da Madeira"
        } else {
            "Região Autónoma dos Açores"
        };
        match code.len() {
            4 => {
                municipalities.insert(code.to_string(), (row[label].clone(), region.to_string()));
            }
            6 => {
                parishes.insert(
                    code.to_string(),
                    (parish_name(&row[label]).to_string(), code[..4].to_string()),
                );
            }
            _ => {}
        }
    }

    let mut places = Vec::new();
    let mut codes: Vec<_> = municipalities.keys().cloned().collect();
    codes.sort();
    for code in &codes {
        let (name, district) = &municipalities[code];
        let mut base = unfiled(Country::Portugal, name, code, Kind::Commune);
        base.coordinates = centres.get(code).copied();
        file(&mut places, &base, "", district, true);
    }
    let mut codes: Vec<_> = parishes.keys().cloned().collect();
    codes.sort();
    for code in &codes {
        let (name, municipality) = &parishes[code];
        let Some((municipality_name, district)) = municipalities.get(municipality) else {
            continue;
        };
        let mut base = unfiled(Country::Portugal, name, code, Kind::Parish);
        base.coordinates = centres.get(code).copied();
        file(&mut places, &base, municipality_name, district, true);
    }
    let live = places.len();

    // Parishes merged in 2013 (and earlier), filed under the municipality
    // they belonged to.
    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold(&p.name), p.subdivision.clone()))
        .collect();
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q45",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["pt"],
            before_year: None,
            undated: true,
        },
    )
    .await?;
    for parish in former {
        let municipality = [&parish.code]
            .into_iter()
            .chain(&parish.codes)
            .filter_map(|c| c.get(..4))
            .find(|c| municipalities.contains_key(*c));
        let Some(municipality) = municipality else {
            continue;
        };
        let (municipality_name, district) = &municipalities[municipality];
        let name = parish_name(&parish.name);
        if known.contains(&(fold(name), municipality_name.clone())) {
            continue;
        }
        let successor = parish
            .codes
            .iter()
            .find(|c| parishes.contains_key(*c))
            .cloned()
            .unwrap_or_else(|| municipality.to_string());
        let own = if parishes.contains_key(&parish.code) {
            ""
        } else {
            &parish.code
        };
        let mut base = unfiled(Country::Portugal, name, own, Kind::FormerCommune);
        base.valid_until = parish.end;
        base.successor = Some(successor);
        base.coordinates = parish.coordinates;
        file(&mut places, &base, municipality_name, district, false);
    }
    eprintln!(
        "Portugal: {live} municipalities and parishes, {} former parishes",
        places.len() - live
    );
    Ok(places)
}

/// Every feature of a CAOP collection, without geometry, page by page.
async fn caop(fetcher: &Fetcher, collection: &str) -> Result<Vec<serde_json::Value>> {
    let mut features = Vec::new();
    for page in 0.. {
        let url = format!(
            "{CAOP_URL}/{collection}/items?f=json&limit={PAGE}&offset={}&skipGeometry=true",
            page * PAGE
        );
        let bytes = fetcher
            .bytes(&format!("caop-{collection}-{page}.json"), &url)
            .await?;
        let json: serde_json::Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("the CAOP answered no JSON for {collection}"))?;
        let batch = json["features"].as_array().cloned().unwrap_or_default();
        let done = batch.len() < PAGE;
        features.extend(batch);
        if done {
            break;
        }
    }
    Ok(features)
}

/// Wikidata sometimes titles a parish "Freguesia de Lourinhã".
fn parish_name(label: &str) -> &str {
    [
        "Freguesia de ",
        "Freguesia do ",
        "Freguesia da ",
        "Freguesia dos ",
        "Freguesia das ",
    ]
    .iter()
    .find_map(|prefix| label.strip_prefix(prefix))
    .unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::parish_name;

    #[test]
    fn parishes_are_filed_by_their_name() {
        assert_eq!(parish_name("Freguesia de Vila A"), "Vila A");
        assert_eq!(parish_name("Vila B"), "Vila B");
    }
}
