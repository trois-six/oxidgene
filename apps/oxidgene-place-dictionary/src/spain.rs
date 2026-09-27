//! Spain: the municipalities of INE's current code list, each under its
//! province and autonomous community, the province names in use before the
//! co-official forms, and the municipalities Wikidata records as dissolved.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, fold, unfiled};
use crate::wikidata::{FormerQuery, coordinates, former_municipalities};
use crate::xlsx;

const INE_URL: &str = "https://www.ine.es/daco/daco42/codmun";
/// Wikidata's property for the INE municipality code.
const CODE_PROPERTY: &str = "P772";
const FORMER_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 ?class . \
     VALUES ?class { wd:Q55863584 wd:Q2074737 }";

/// Provinces by INE code, as INE names them today.
const PROVINCES: &[(&str, &str)] = &[
    ("01", "Araba/Álava"),
    ("02", "Albacete"),
    ("03", "Alicante/Alacant"),
    ("04", "Almería"),
    ("05", "Ávila"),
    ("06", "Badajoz"),
    ("07", "Illes Balears"),
    ("08", "Barcelona"),
    ("09", "Burgos"),
    ("10", "Cáceres"),
    ("11", "Cádiz"),
    ("12", "Castellón/Castelló"),
    ("13", "Ciudad Real"),
    ("14", "Córdoba"),
    ("15", "A Coruña"),
    ("16", "Cuenca"),
    ("17", "Girona"),
    ("18", "Granada"),
    ("19", "Guadalajara"),
    ("20", "Gipuzkoa"),
    ("21", "Huelva"),
    ("22", "Huesca"),
    ("23", "Jaén"),
    ("24", "León"),
    ("25", "Lleida"),
    ("26", "La Rioja"),
    ("27", "Lugo"),
    ("28", "Madrid"),
    ("29", "Málaga"),
    ("30", "Murcia"),
    ("31", "Navarra"),
    ("32", "Ourense"),
    ("33", "Asturias"),
    ("34", "Palencia"),
    ("35", "Las Palmas"),
    ("36", "Pontevedra"),
    ("37", "Salamanca"),
    ("38", "Santa Cruz de Tenerife"),
    ("39", "Cantabria"),
    ("40", "Segovia"),
    ("41", "Sevilla"),
    ("42", "Soria"),
    ("43", "Tarragona"),
    ("44", "Teruel"),
    ("45", "Toledo"),
    ("46", "Valencia/València"),
    ("47", "Valladolid"),
    ("48", "Bizkaia"),
    ("49", "Zamora"),
    ("50", "Zaragoza"),
    ("51", "Ceuta"),
    ("52", "Melilla"),
];

/// Autonomous communities by INE code, as INE names them.
const COMMUNITIES: &[(&str, &str)] = &[
    ("01", "Andalucía"),
    ("02", "Aragón"),
    ("03", "Principado de Asturias"),
    ("04", "Illes Balears"),
    ("05", "Canarias"),
    ("06", "Cantabria"),
    ("07", "Castilla y León"),
    ("08", "Castilla-La Mancha"),
    ("09", "Cataluña/Catalunya"),
    ("10", "Comunitat Valenciana"),
    ("11", "Extremadura"),
    ("12", "Galicia"),
    ("13", "Comunidad de Madrid"),
    ("14", "Región de Murcia"),
    ("15", "Comunidad Foral de Navarra"),
    ("16", "País Vasco/Euskadi"),
    ("17", "La Rioja"),
    ("18", "Ceuta"),
    ("19", "Melilla"),
];

/// Province names in use before a law gave the province its co-official
/// form, and the day that law took effect.
const FORMER_PROVINCE_NAMES: &[(&str, &str, &str)] = &[
    ("01", "Álava", "2011-07-06"),
    ("07", "Baleares", "1997-04-26"),
    ("15", "La Coruña", "1998-03-05"),
    ("17", "Gerona", "1992-03-01"),
    ("20", "Guipúzcoa", "2011-07-06"),
    ("25", "Lérida", "1992-03-01"),
    ("32", "Orense", "1998-03-05"),
    ("48", "Vizcaya", "2011-07-06"),
];

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let provinces: HashMap<&str, &str> = PROVINCES.iter().copied().collect();
    let communities: HashMap<&str, &str> = COMMUNITIES.iter().copied().collect();
    let municipalities = current(fetcher).await?;
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;

    let mut places = Vec::new();
    let mut live: HashMap<String, (&str, &str)> = HashMap::new();
    for m in &municipalities {
        let province = provinces
            .get(&m.code[..2])
            .with_context(|| format!("unknown province {}", &m.code[..2]))?;
        let community = communities
            .get(m.community.as_str())
            .with_context(|| format!("unknown autonomous community {}", m.community))?;
        live.insert(m.code.clone(), (province, community));
        let mut base = unfiled(Country::Spain, &m.name, &m.code, Kind::Commune);
        base.coordinates = centres.get(&m.code).copied();
        file(&mut places, &base, province, community, true);
        for (_, former, _) in FORMER_PROVINCE_NAMES.iter().filter(|p| p.0 == &m.code[..2]) {
            file(&mut places, &base, former, community, false);
        }
    }
    let official = places.len();

    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold(&p.name), p.subdivision.clone()))
        .collect();
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q29",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["es", "ca", "gl", "eu"],
            before_year: None,
            undated: false,
        },
    )
    .await?;
    for commune in former {
        let Some((successor, (province, community))) = commune
            .codes
            .iter()
            .find_map(|c| live.get_key_value(&five_digits(c)))
        else {
            continue;
        };
        if known.contains(&(fold(&commune.name), province.to_string())) {
            continue;
        }
        let own = five_digits(&commune.code);
        let own = if live.contains_key(&own) {
            String::new()
        } else {
            own
        };
        let mut base = unfiled(Country::Spain, &commune.name, &own, Kind::FormerCommune);
        base.valid_from.clone_from(&commune.start);
        base.valid_until.clone_from(&commune.end);
        base.successor = Some(successor.clone());
        base.coordinates = commune.coordinates;
        file(&mut places, &base, province, community, false);
        for (_, name, until) in FORMER_PROVINCE_NAMES
            .iter()
            .filter(|p| p.0 == &successor[..2])
        {
            if commune.end.as_deref().is_none_or(|end| end < *until) {
                file(&mut places, &base, name, community, false);
            }
        }
    }
    eprintln!(
        "Spain: {official} rows from INE, {} from Wikidata",
        places.len() - official
    );
    Ok(places)
}

struct Municipality {
    code: String,
    name: String,
    community: String,
}

/// The newest yearly list INE has published.
async fn current(fetcher: &Fetcher) -> Result<Vec<Municipality>> {
    let (year, _, _) = crate::calendar::today();
    let mut workbook = None;
    for year in (year - 2..=year + 1).rev() {
        let name = format!("diccionario{:02}.xlsx", year % 100);
        if let Some(bytes) = fetcher
            .optional_bytes(&name, &format!("{INE_URL}/{name}"))
            .await?
        {
            eprintln!("Spain: INE {name}");
            workbook = Some(bytes);
            break;
        }
    }
    let rows = xlsx::first_sheet(&workbook.context("no INE municipality list was found")?)?;
    let cell = |row: &HashMap<String, String>, column: &str| {
        row.get(column)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            let (community, province, municipality) =
                (cell(row, "A"), cell(row, "B"), cell(row, "C"));
            // Header rows hold words where the codes go.
            if !(community.len() == 2 && province.len() == 2 && municipality.len() == 3)
                || !(community.clone() + &province + &municipality)
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            {
                return None;
            }
            Some(Municipality {
                code: format!("{province}{municipality}"),
                name: natural_name(&cell(row, "E")),
                community,
            })
        })
        .collect())
}

/// INE sorts a name by its main word, "Iglesuela del Cid, La"; this writes
/// it the way it is said, "La Iglesuela del Cid", in each language of a
/// bilingual name.
fn natural_name(name: &str) -> String {
    name.split('/')
        .map(|part| match part.rsplit_once(", ") {
            Some((head, article)) if article.len() <= 4 => {
                if article.ends_with('\'') {
                    format!("{article}{head}")
                } else {
                    format!("{article} {head}")
                }
            }
            _ => part.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// INE codes are five digits; Wikidata sometimes adds the check digit.
fn five_digits(code: &str) -> String {
    code.get(..5).unwrap_or(code).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_written_as_they_are_said() {
        assert_eq!(natural_name("Villa A, La"), "La Villa A");
        assert_eq!(natural_name("Villa A, L'"), "L'Villa A");
        assert_eq!(
            natural_name("Villa A, O/Villa A, El"),
            "O Villa A/El Villa A"
        );
        assert_eq!(natural_name("Villa A"), "Villa A");
    }

    #[test]
    fn every_province_belongs_to_a_known_code() {
        assert_eq!(PROVINCES.len(), 52);
        assert_eq!(COMMUNITIES.len(), 19);
    }
}
