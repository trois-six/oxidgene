//! The Netherlands: the municipalities of CBS's current list under their
//! province, and the municipalities merged away since 1812 from Wikidata.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, fold, unfiled};
use crate::wikidata::{FormerQuery, coordinates, former_municipalities};
use crate::xlsx;

const CBS_URL: &str = "https://www.cbs.nl/-/media/cbs/onze-diensten/methoden/classificaties/overig";
/// Wikidata's property for the CBS municipality code.
const CODE_PROPERTY: &str = "P382";
const FORMER_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 wd:Q2039348 .";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let municipalities = current(fetcher).await?;
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    let mut places = Vec::new();
    for m in municipalities.values() {
        let mut base = unfiled(Country::Netherlands, &m.name, &m.code, Kind::Commune);
        base.coordinates = centres.get(&m.code).copied();
        file(&mut places, &base, "", &m.province, true);
    }
    let official = places.len();

    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold(&p.name), p.region_name().to_string()))
        .collect();
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q55",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["nl", "fy"],
            before_year: None,
            undated: false,
        },
    )
    .await?;
    for commune in &former {
        let Some(now) = commune.codes.iter().find_map(|c| municipalities.get(c)) else {
            continue;
        };
        if known.contains(&(fold(&commune.name), now.province.clone())) {
            continue;
        }
        let own = if municipalities.contains_key(&commune.code) {
            ""
        } else {
            &commune.code
        };
        let mut base = unfiled(
            Country::Netherlands,
            &commune.name,
            own,
            Kind::FormerCommune,
        );
        base.valid_from.clone_from(&commune.start);
        base.valid_until.clone_from(&commune.end);
        base.successor = Some(now.code.clone());
        base.coordinates = commune.coordinates;
        file(&mut places, &base, "", &now.province, false);
    }
    eprintln!(
        "Netherlands: {official} rows from CBS, {} from Wikidata",
        places.len() - official
    );
    Ok(places)
}

struct Municipality {
    code: String,
    name: String,
    province: String,
}

/// The newest yearly list CBS has published, by municipality code.
async fn current(fetcher: &Fetcher) -> Result<HashMap<String, Municipality>> {
    let (year, _, _) = crate::calendar::today();
    let mut workbook = None;
    for year in (year - 2..=year + 1).rev() {
        let name = format!("gemeenten-alfabetisch-{year}.xlsx");
        if let Some(bytes) = fetcher
            .optional_bytes(&name, &format!("{CBS_URL}/{name}"))
            .await?
        {
            eprintln!("Netherlands: CBS {name}");
            workbook = Some(bytes);
            break;
        }
    }
    let sheets = xlsx::sheets(&workbook.context("no CBS municipality list was found")?)?;
    // The list is the sheet headed by "Gemeentecode"; another explains it.
    let rows = sheets
        .into_iter()
        .find(|rows| {
            rows.first()
                .and_then(|r| r.get("A"))
                .is_some_and(|a| a == "Gemeentecode")
        })
        .context("the CBS workbook has no municipality sheet")?;
    let cell = |row: &HashMap<String, String>, column: &str| {
        row.get(column)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    Ok(rows
        .iter()
        .skip(1)
        .filter(|row| !cell(row, "A").is_empty())
        .map(|row| {
            let code = cell(row, "A");
            (
                code.clone(),
                Municipality {
                    code,
                    name: cell(row, "C"),
                    province: cell(row, "F"),
                },
            )
        })
        .collect())
}
