//! Luxembourg: its communes under their canton, and the communes merged
//! since 1978, from Wikidata. Luxembourg publishes no downloadable register
//! of them; Wikidata carries the official LAU code of each.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Place, file, unfiled};
use crate::wikidata::{FormerQuery, former_municipalities};
use oxidgene_core::search::fold_words;

/// Wikidata's property for the LAU code.
const CODE_PROPERTY: &str = "P782";
const CURRENT: &str = r#"
SELECT ?item ?code ?label ?canton ?coord WHERE {
  ?item wdt:P31 wd:Q2919801 ; wdt:P782 ?code .
  FILTER NOT EXISTS { ?item wdt:P576 [] }
  ?item rdfs:label ?label . FILTER(LANG(?label) = "fr")
  OPTIONAL { ?item wdt:P131 ?c . ?c wdt:P31 wd:Q1146429 ; rdfs:label ?canton . FILTER(LANG(?canton) = "fr") }
  OPTIONAL { ?item wdt:P625 ?coord }
}"#;
const FORMER_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 wd:Q134577111 .";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let table = fetcher.sparql(CURRENT).await?;
    let (code, label, canton, coord) = (
        table.column("code")?,
        table.column("label")?,
        table.column("canton")?,
        table.column("coord")?,
    );
    let mut cantons: HashMap<String, String> = HashMap::new();
    let mut places = Vec::new();
    for row in &table.rows {
        let canton = canton_name(&row[canton]);
        cantons.insert(row[code].clone(), canton.to_string());
        let mut base = unfiled(Country::Luxembourg, &row[label], &row[code], Kind::Commune);
        base.coordinates = Coordinates::from_wkt(&row[coord]);
        file(&mut places, &base, "", canton, true);
    }
    let live = places.len();

    let known: HashSet<String> = places.iter().map(|p| fold_words(&p.name)).collect();
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q32",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["fr", "lb", "de"],
            before_year: None,
            undated: true,
        },
    )
    .await?;
    for commune in former {
        let Some((successor, canton)) = commune.codes.iter().find_map(|c| cantons.get_key_value(c))
        else {
            continue;
        };
        if known.contains(&fold_words(&commune.name)) {
            continue;
        }
        let mut base = unfiled(
            Country::Luxembourg,
            &commune.name,
            &commune.code,
            Kind::FormerCommune,
        );
        base.valid_from = commune.start;
        base.valid_until = commune.end;
        base.successor = Some(successor.clone());
        base.coordinates = commune.coordinates;
        file(&mut places, &base, "", canton, false);
    }
    eprintln!(
        "Luxembourg: {live} communes, {} former communes",
        places.len() - live
    );
    Ok(places)
}

/// "Canton de Redange" is filed as Redange.
fn canton_name(label: &str) -> &str {
    ["Canton de ", "Canton d’", "Canton d'", "Canton du "]
        .iter()
        .find_map(|prefix| label.strip_prefix(prefix))
        .unwrap_or(label)
}

#[cfg(test)]
mod tests {
    use super::canton_name;

    #[test]
    fn cantons_are_filed_by_their_name() {
        assert_eq!(canton_name("Canton de Ville A"), "Ville A");
        assert_eq!(canton_name("Canton d’Ville B"), "Ville B");
        assert_eq!(canton_name("Ville C"), "Ville C");
    }
}
