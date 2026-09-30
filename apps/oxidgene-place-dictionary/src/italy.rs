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
    let changes = Changes::read(&changes)?;
    let suppressed = Suppressed::read(&suppressed)?;
    let history = History {
        next: successors(&changes, &suppressed),
        current: &current,
        centres: &centres,
    };
    history.former_names(&changes, &mut places);
    let renamed = places.len() - live;
    history.suppressed_communes(&suppressed, &mut places);
    eprintln!(
        "Italy: {live} communes, {renamed} former names and codes, {} suppressed communes",
        places.len() - live - renamed
    );
    Ok(places)
}

/// The changes since 1991, with the indexes of the columns read.
struct Changes<'t> {
    table: &'t Table,
    kind: usize,
    code: usize,
    name: usize,
    new_code: usize,
    date: usize,
}

impl<'t> Changes<'t> {
    fn read(table: &'t Table) -> Result<Self> {
        let [kind, code, name, date] = table.columns([
            "Tipo variazione",
            "Codice Comune formato alfanumerico",
            "Denominazione Comune",
            "Data decorrenza validità amministrativa",
        ])?;
        let new_code = column_starting(table, "Codice del Comune associato alla variazione")?;
        Ok(Self {
            table,
            kind,
            code,
            name,
            new_code,
            date,
        })
    }
}

/// The suppressed communes, with the indexes of the columns read.
struct Suppressed<'t> {
    table: &'t Table,
    code: usize,
    name: usize,
    date: usize,
    into: usize,
}

impl<'t> Suppressed<'t> {
    fn read(table: &'t Table) -> Result<Self> {
        let [code, name, date, into] = table.columns([
            "Codice Comune",
            "Denominazione Comune",
            "Data evento",
            "Codice del Comune associato alla variazione",
        ])?;
        Ok(Self {
            table,
            code,
            name,
            date,
            into,
        })
    }
}

/// Where each code went: into the commune that absorbed it, or to the new
/// code a change of province gave it.
fn successors<'t>(changes: &Changes<'t>, suppressed: &Suppressed<'t>) -> HashMap<&'t str, &'t str> {
    let mut next: HashMap<&str, &str> = suppressed
        .table
        .rows
        .iter()
        .map(|r| (r[suppressed.code].as_str(), r[suppressed.into].as_str()))
        .collect();
    for row in &changes.table.rows {
        if row[changes.kind].trim() == "AP" && row[changes.code] != row[changes.new_code] {
            next.insert(&row[changes.code], &row[changes.new_code]);
        }
    }
    next
}

/// What the communes of the past became.
struct History<'a> {
    next: HashMap<&'a str, &'a str>,
    current: &'a HashMap<String, Commune>,
    centres: &'a HashMap<String, crate::place::Coordinates>,
}

impl History<'_> {
    /// Renames and changes of province since 1991: the commune still exists.
    fn former_names(&self, changes: &Changes<'_>, places: &mut Vec<Place>) {
        let provinces: HashMap<&str, (&str, &str)> = self
            .current
            .values()
            .map(|c| {
                (
                    c.code.get(..3).unwrap_or_default(),
                    (c.province.as_str(), c.region.as_str()),
                )
            })
            .collect();
        for row in &changes.table.rows {
            if !matches!(row[changes.kind].trim(), "CD" | "AP") {
                continue;
            }
            let Some(now) = follow(self.current, &self.next, &row[changes.new_code]) else {
                continue;
            };
            let (name, code) = (&row[changes.name], &row[changes.code]);
            if *name == now.name && *code == now.code {
                continue;
            }
            let mut base = unfiled(Country::Italy, name, code, Kind::FormerName);
            base.valid_until = iso(&row[changes.date]);
            base.successor = Some(now.code.clone());
            base.coordinates = self.centres.get(&now.code).copied();
            // The province the old code belonged to, when it still exists;
            // otherwise today's.
            let (province, region) = provinces
                .get(code.get(..3).unwrap_or_default())
                .copied()
                .unwrap_or((&now.province, &now.region));
            file(places, &base, province, region, false);
        }
    }

    /// Suppressed communes, each followed to the commune holding its land.
    fn suppressed_communes(&self, suppressed: &Suppressed<'_>, places: &mut Vec<Place>) {
        for row in &suppressed.table.rows {
            // Communes ceded abroad in 1947 name no Italian successor.
            let Some(now) = follow(self.current, &self.next, &row[suppressed.into]) else {
                continue;
            };
            let code = &row[suppressed.code];
            let mut base = unfiled(
                Country::Italy,
                &row[suppressed.name],
                code,
                Kind::FormerCommune,
            );
            base.valid_until = iso(&row[suppressed.date]);
            base.successor = Some(now.code.clone());
            base.coordinates = self.centres.get(code).copied();
            file(places, &base, &now.province, &now.region, false);
        }
    }
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
