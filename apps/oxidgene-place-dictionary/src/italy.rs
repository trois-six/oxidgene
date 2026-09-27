//! Italy: the communes of ISTAT's current list, the communes it records as
//! suppressed since 1861, and the names and codes communes bore before a
//! rename or a change of province since 1991. Each is filed under its
//! province (or metropolitan city, free consortium) and region.

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Country, Kind, Place, file, unfiled};
use crate::table::{Table, decode_mixed};
use crate::wikidata::coordinates;

const ISTAT_URL: &str = "https://www.istat.it/storage/codici-unita-amministrative";
/// Wikidata's property for the ISTAT commune code.
const CODE_PROPERTY: &str = "P635";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let current = current(fetcher).await?;
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    let mut places = Vec::new();
    for commune in current.values() {
        let mut base = unfiled(Country::Italy, &commune.name, &commune.code, Kind::Commune);
        base.coordinates = centres.get(&commune.code).copied();
        file(&mut places, &base, &commune.province, &commune.region, true);
    }
    let live = places.len();

    let changes = zipped_csv(
        fetcher,
        "Variazioni-amministrative-e-territoriali-dal-1991.zip",
    )
    .await?;
    let suppressed = zipped_csv(fetcher, "Elenco-comuni-soppressi.zip").await?;

    // Where each code went: into the commune that absorbed it, or to the new
    // code a change of province gave it.
    let (kind, code, name, new_code, date) = (
        changes.column("Tipo variazione")?,
        changes.column("Codice Comune formato alfanumerico")?,
        changes.column("Denominazione Comune")?,
        column_starting(&changes, "Codice del Comune associato alla variazione")?,
        changes.column("Data decorrenza validità amministrativa")?,
    );
    let (s_code, s_name, s_date, s_into) = (
        suppressed.column("Codice Comune")?,
        suppressed.column("Denominazione Comune")?,
        suppressed.column("Data evento")?,
        suppressed.column("Codice del Comune associato alla variazione")?,
    );
    let mut next: HashMap<&str, &str> = suppressed
        .rows
        .iter()
        .map(|r| (r[s_code].as_str(), r[s_into].as_str()))
        .collect();
    for row in &changes.rows {
        if row[kind].trim() == "AP" && row[code] != row[new_code] {
            next.insert(&row[code], &row[new_code]);
        }
    }

    // Renames and changes of province since 1991: the commune still exists.
    let provinces: HashMap<&str, (&str, &str)> = current
        .values()
        .map(|c| {
            (
                c.code.get(..3).unwrap_or_default(),
                (c.province.as_str(), c.region.as_str()),
            )
        })
        .collect();
    for row in &changes.rows {
        if !matches!(row[kind].trim(), "CD" | "AP") {
            continue;
        }
        let Some(now) = follow(&current, &next, &row[new_code]) else {
            continue;
        };
        if row[name] == now.name && row[code] == now.code {
            continue;
        }
        let mut base = unfiled(Country::Italy, &row[name], &row[code], Kind::FormerName);
        base.valid_until = iso(&row[date]);
        base.successor = Some(now.code.clone());
        base.coordinates = centres.get(&now.code).copied();
        // The province the old code belonged to, when it still exists;
        // otherwise today's.
        let (province, region) = provinces
            .get(row[code].get(..3).unwrap_or_default())
            .copied()
            .unwrap_or((&now.province, &now.region));
        file(&mut places, &base, province, region, false);
    }
    let renamed = places.len() - live;

    // Suppressed communes, each followed to the commune holding its land.
    for row in &suppressed.rows {
        // Communes ceded abroad in 1947 name no Italian successor.
        let Some(now) = follow(&current, &next, &row[s_into]) else {
            continue;
        };
        let mut base = unfiled(
            Country::Italy,
            &row[s_name],
            &row[s_code],
            Kind::FormerCommune,
        );
        base.valid_until = iso(&row[s_date]);
        base.successor = Some(now.code.clone());
        base.coordinates = centres.get(&row[s_code]).copied();
        file(&mut places, &base, &now.province, &now.region, false);
    }
    eprintln!(
        "Italy: {live} communes, {renamed} former names and codes, {} suppressed communes",
        places.len() - live - renamed
    );
    Ok(places)
}

struct Commune {
    code: String,
    name: String,
    province: String,
    region: String,
}

async fn current(fetcher: &Fetcher) -> Result<HashMap<String, Commune>> {
    let name = "Elenco-comuni-italiani.csv";
    let bytes = fetcher.bytes(name, &format!("{ISTAT_URL}/{name}")).await?;
    let table =
        Table::parse(&decode_mixed(&bytes), ';').with_context(|| format!("cannot read {name}"))?;
    let (code, commune, region, province) = (
        table.column("Codice Comune formato alfanumerico")?,
        table.column("Denominazione (Italiana e straniera)")?,
        table.column("Denominazione Regione")?,
        column_starting(
            &table,
            "Denominazione dell'Unità territoriale sovracomunale",
        )?,
    );
    Ok(table
        .rows
        .iter()
        .filter(|r| !r[code].is_empty())
        .map(|r| {
            (
                r[code].clone(),
                Commune {
                    code: r[code].clone(),
                    name: r[commune].clone(),
                    province: r[province].clone(),
                    region: r[region].clone(),
                },
            )
        })
        .collect())
}

/// The current commune a code leads to, through the communes it was merged
/// into.
fn follow<'a>(
    current: &'a HashMap<String, Commune>,
    merged_into: &HashMap<&str, &str>,
    code: &str,
) -> Option<&'a Commune> {
    let mut code = code;
    for _ in 0..16 {
        if let Some(commune) = current.get(code) {
            return Some(commune);
        }
        code = merged_into.get(code)?;
    }
    None
}

/// The first column whose header starts with `prefix`: ISTAT headers carry
/// line breaks and footnote marks.
fn column_starting(table: &Table, prefix: &str) -> Result<usize> {
    table
        .column_where(|h| h.starts_with(prefix))
        .with_context(|| format!("no column starting with `{prefix}`"))
}

async fn zipped_csv(fetcher: &Fetcher, name: &str) -> Result<Table> {
    let bytes = fetcher.bytes(name, &format!("{ISTAT_URL}/{name}")).await?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let entry = archive
        .file_names()
        .find(|n| n.ends_with(".csv"))
        .with_context(|| format!("{name} holds no CSV"))?
        .to_string();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name(&entry)?, &mut bytes)?;
    Table::parse(&decode_mixed(&bytes), ';').with_context(|| format!("cannot read {entry}"))
}

/// `DD/MM/YYYY` as `YYYY-MM-DD`.
fn iso(date: &str) -> Option<String> {
    let mut parts = date.trim().split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(d), Some(m), Some(y)) if y.len() == 4 => Some(format!("{y}-{m:0>2}-{d:0>2}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn istat_dates_read_as_iso() {
        assert_eq!(iso("16/09/1947").as_deref(), Some("1947-09-16"));
        assert_eq!(iso("").as_deref(), None);
    }

    #[test]
    fn a_suppressed_commune_leads_to_the_commune_holding_its_land() {
        let current = HashMap::from([(
            "099001".to_string(),
            Commune {
                code: "099001".to_string(),
                name: "Comune A".to_string(),
                province: "Provincia A".to_string(),
                region: "Regione A".to_string(),
            },
        )]);
        let merged = HashMap::from([("099902", "099901"), ("099901", "099001")]);
        assert_eq!(follow(&current, &merged, "099902").unwrap().code, "099001");
        assert!(follow(&current, &merged, "").is_none());
    }
}
