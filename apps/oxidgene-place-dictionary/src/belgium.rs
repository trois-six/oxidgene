//! Belgium: its municipalities and their sections — the communes merged by
//! the fusions of 1977 and after, which records still name — from Wikidata,
//! filed under their province and region. Statbel's code list sits behind a
//! bot challenge no generator can pass; Wikidata carries the same NIS codes.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Place, file, unfiled};
use crate::table::Table;
use crate::wikidata::{Former, FormerQuery, former_municipalities, wikidata_date};
use oxidgene_core::search::fold_words;

/// Wikidata's property for the NIS code.
const CODE_PROPERTY: &str = "P1567";
const MUNICIPALITY_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 ?class . \
     VALUES ?class { wd:Q493522 wd:Q15273785 }";

/// Municipalities with their NIS code, end date if merged, and names.
const MUNICIPALITIES: &str = r#"
SELECT ?item ?code ?end ?nl ?fr ?de ?coord WHERE {
  ?item wdt:P1567 ?code ; p:P31 ?statement .
  ?statement ps:P31 ?class . VALUES ?class { wd:Q493522 wd:Q15273785 }
  FILTER(STRLEN(?code) = 5)
  OPTIONAL { ?item wdt:P576 ?dissolved }
  OPTIONAL { ?statement pq:P582 ?ended }
  BIND(COALESCE(?dissolved, ?ended) AS ?end)
  OPTIONAL { ?item rdfs:label ?nl . FILTER(LANG(?nl) = "nl") }
  OPTIONAL { ?item rdfs:label ?fr . FILTER(LANG(?fr) = "fr") }
  OPTIONAL { ?item rdfs:label ?de . FILTER(LANG(?de) = "de") }
  OPTIONAL { ?item wdt:P625 ?coord }
}"#;

/// The sections of the municipalities: the communes of before the fusions.
const SECTIONS: &str = r#"
SELECT ?item ?code ?parent ?nl ?fr ?de ?coord WHERE {
  ?item wdt:P31 wd:Q2785216 .
  OPTIONAL { ?item wdt:P1567 ?code }
  OPTIONAL { ?item wdt:P131 ?municipality . ?municipality wdt:P1567 ?parent . FILTER(STRLEN(?parent) = 5) }
  OPTIONAL { ?item rdfs:label ?nl . FILTER(LANG(?nl) = "nl") }
  OPTIONAL { ?item rdfs:label ?fr . FILTER(LANG(?fr) = "fr") }
  OPTIONAL { ?item rdfs:label ?de . FILTER(LANG(?de) = "de") }
  OPTIONAL { ?item wdt:P625 ?coord }
}"#;

/// The German-speaking municipalities, whose names are German.
const GERMAN_SPEAKING: &[&str] = &[
    "63001", "63012", "63013", "63023", "63040", "63048", "63061", "63067", "63087",
];

/// Province and region of a NIS code, by its first digits.
fn filing(code: &str) -> Option<(&'static str, &'static str)> {
    const FLANDERS: &str = "Vlaams Gewest";
    const WALLONIA: &str = "Région wallonne";
    const BRUSSELS: &str = "Région de Bruxelles-Capitale/Brussels Hoofdstedelijk Gewest";
    Some(match code.get(..2)? {
        "21" => ("", BRUSSELS),
        "23" | "24" => ("Vlaams-Brabant", FLANDERS),
        "25" => ("Brabant wallon", WALLONIA),
        _ => match code.get(..1)? {
            "1" => ("Antwerpen", FLANDERS),
            "3" => ("West-Vlaanderen", FLANDERS),
            "4" => ("Oost-Vlaanderen", FLANDERS),
            "5" => ("Hainaut", WALLONIA),
            "6" => ("Liège", WALLONIA),
            "7" => ("Limburg", FLANDERS),
            "8" => ("Luxembourg", WALLONIA),
            "9" => ("Namur", WALLONIA),
            _ => return None,
        },
    })
}

/// A name in the municipality's own language; both in Brussels.
fn local_name(code: &str, nl: &str, fr: &str, de: &str) -> String {
    let pick =
        |first: &str, second: &str| if first.is_empty() { second } else { first }.to_string();
    match code.get(..2) {
        _ if GERMAN_SPEAKING.contains(&code) && !de.is_empty() => de.to_string(),
        Some("21") if !fr.is_empty() && !nl.is_empty() && fr != nl => format!("{fr}/{nl}"),
        Some("21" | "25") => pick(fr, nl),
        _ => match filing(code) {
            Some((_, "Région wallonne")) => pick(fr, nl),
            _ => pick(nl, fr),
        },
    }
}

/// The municipalities without an end date, by NIS code, with their local
/// name and centre.
fn live_municipalities(table: &Table) -> Result<Live> {
    let [code, end, nl, fr, de, coord] =
        table.columns(["code", "end", "nl", "fr", "de", "coord"])?;
    let mut live: HashMap<String, (String, Option<Coordinates>)> = HashMap::new();
    for row in &table.rows {
        if wikidata_date(&row[end]).is_none() {
            let name = local_name(&row[code], &row[nl], &row[fr], &row[de]);
            live.insert(
                row[code].clone(),
                (name, Coordinates::from_wkt(&row[coord])),
            );
        }
    }
    Ok(live)
}

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let live = live_municipalities(&fetcher.sparql(MUNICIPALITIES).await?)?;
    let mut places = Vec::new();
    let mut codes: Vec<_> = live.keys().cloned().collect();
    codes.sort();
    for code in &codes {
        let (name, coordinates) = &live[code];
        let Some((province, region)) = filing(code) else {
            continue;
        };
        let mut base = unfiled(Country::Belgium, name, code, Kind::Commune);
        base.coordinates = *coordinates;
        file(&mut places, &base, province, region, true);
        // Until 1995 these were the province of Brabant.
        if matches!(&code[..2], "21" | "23" | "24" | "25") {
            file(&mut places, &base, "Brabant", region, false);
        }
    }
    let municipalities = places.len();

    // Municipalities merged since the fusions (2019, 2025), each to the
    // municipality holding its land today.
    let merged = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q31",
            instance: MUNICIPALITY_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["nl", "fr", "de"],
            before_year: None,
            undated: false,
        },
    )
    .await?;
    let merged_into = file_merged(&mut places, &merged, &live);

    // The sections, filed under the municipality holding them today.
    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold_words(&p.name), p.subdivision.clone()))
        .collect();
    let sections = fetcher.sparql(SECTIONS).await?;
    file_sections(&mut places, &sections, &live, &merged_into, &known)?;
    eprintln!(
        "Belgium: {municipalities} municipality rows, {} merged communes and sections",
        places.len() - municipalities
    );
    Ok(places)
}

/// Live municipalities: name and coordinates, by code.
type Live = HashMap<String, (String, Option<Coordinates>)>;

/// Files the municipalities `merged` since the fusions, each under the one
/// holding its land today; that one's code by the merged one's.
fn file_merged(places: &mut Vec<Place>, merged: &[Former], live: &Live) -> HashMap<String, String> {
    let mut merged_into: HashMap<String, String> = HashMap::new();
    for commune in merged {
        let Some(now) = commune.codes.iter().find(|c| live.contains_key(*c)) else {
            continue;
        };
        merged_into.insert(commune.code.clone(), now.clone());
        let Some((province, region)) = filing(now) else {
            continue;
        };
        let own = if live.contains_key(&commune.code) {
            ""
        } else {
            &commune.code
        };
        let mut base = unfiled(Country::Belgium, &commune.name, own, Kind::FormerCommune);
        base.valid_until.clone_from(&commune.end);
        base.successor = Some(now.clone());
        base.coordinates = commune.coordinates;
        file(places, &base, province, region, false);
    }
    merged_into
}

/// Files the sections of `sections` under the municipality holding them
/// today, but for a name already `known` in its province.
fn file_sections(
    places: &mut Vec<Place>,
    sections: &Table,
    live: &Live,
    merged_into: &HashMap<String, String>,
    known: &HashSet<(String, String)>,
) -> Result<()> {
    let [code, parent, nl, fr, de, coord] =
        sections.columns(["code", "parent", "nl", "fr", "de", "coord"])?;
    let mut seen = HashSet::new();
    for row in &sections.rows {
        let parent = merged_into.get(&row[parent]).unwrap_or(&row[parent]);
        if !live.contains_key(parent)
            || !seen.insert((row[code].clone(), parent.clone(), row[nl].clone()))
        {
            continue;
        }
        let Some((province, region)) = filing(parent) else {
            continue;
        };
        let name = local_name(parent, &row[nl], &row[fr], &row[de]);
        if name.is_empty() || known.contains(&(fold_words(&name), province.to_string())) {
            continue;
        }
        let mut base = unfiled(Country::Belgium, &name, &row[code], Kind::FormerCommune);
        base.successor = Some(parent.clone());
        base.coordinates = Coordinates::from_wkt(&row[coord]);
        file(places, &base, province, region, false);
        if matches!(&parent[..2], "21" | "23" | "24" | "25") {
            file(places, &base, "Brabant", region, false);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nis_code_says_its_province_and_region() {
        assert_eq!(filing("11002"), Some(("Antwerpen", "Vlaams Gewest")));
        assert_eq!(filing("25005").map(|f| f.0), Some("Brabant wallon"));
        assert_eq!(filing("21004").map(|f| f.0), Some(""));
        assert_eq!(filing("00000"), None);
    }

    #[test]
    fn names_follow_the_language_of_the_region() {
        assert_eq!(
            local_name("11002", "Gemeente A", "Commune A", ""),
            "Gemeente A"
        );
        assert_eq!(
            local_name("92094", "Gemeente B", "Commune B", ""),
            "Commune B"
        );
        assert_eq!(
            local_name("21009", "Gemeente C", "Commune C", ""),
            "Commune C/Gemeente C"
        );
        assert_eq!(
            local_name("63023", "Gemeente D", "Commune D", "Gemeinde D"),
            "Gemeinde D"
        );
    }
}
