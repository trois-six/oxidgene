//! Switzerland: every commune since 1960 from the Federal Statistical
//! Office's official commune register, and the communes merged away before
//! 1960 from Wikidata. Each is filed under its canton.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Place, file, unfiled};
use crate::table::Table;
use crate::wikidata::{FormerQuery, coordinates, former_municipalities};
use oxidgene_core::search::fold_words;

/// The register's API: a snapshot of every canton, district and commune
/// valid on a date, and the mutations between two dates.
const AGV_URL: &str = "https://www.agvchapp.bfs.admin.ch/api/communes";
/// The register records mutations from this day on.
const REGISTER_START: &str = "01-01-1960";

/// The Swiss municipality number, Wikidata's property for it, and the class
/// of its former municipalities.
const CODE_PROPERTY: &str = "P771";
const FORMER_INSTANCE: &str = "?item p:P31 ?statement . ?statement ps:P31 wd:Q685309 .";

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let register = Register::load(fetcher).await?;
    let centres = coordinates(fetcher, CODE_PROPERTY).await?;
    let mut places = register.places(&centres);
    let official = places.len();

    let known: HashSet<(String, String)> = places
        .iter()
        .map(|p| (fold_words(&p.name), p.region_name().to_string()))
        .collect();
    let former = former_municipalities(
        fetcher,
        &FormerQuery {
            country: "Q39",
            instance: FORMER_INSTANCE,
            code: CODE_PROPERTY,
            languages: &["de", "fr", "it", "rm"],
            before_year: Some(1960),
            undated: true,
        },
    )
    .await?;
    for commune in former {
        let Some(successor) = commune
            .codes
            .iter()
            .find_map(|c| register.live_by_code.get(c))
        else {
            continue;
        };
        let canton = register.canton_of(*successor);
        if known.contains(&(fold_words(&commune.name), canton.to_string())) {
            continue;
        }
        let mut base = unfiled(
            Country::Switzerland,
            &commune.name,
            &commune.code,
            Kind::FormerCommune,
        );
        base.valid_from = commune.start;
        base.valid_until = commune.end;
        base.successor = Some(register.records[*successor].code.clone());
        base.coordinates = commune.coordinates;
        file(&mut places, &base, "", canton, false);
    }
    eprintln!(
        "Switzerland: {official} rows from the BFS, {} from Wikidata",
        places.len() - official
    );
    Ok(places)
}

#[derive(Debug, Clone)]
struct Record {
    level: u8,
    code: String,
    parent: String,
    name: String,
    /// `YYYY-MM-DD`.
    valid_from: String,
    valid_to: Option<String>,
}

struct Register {
    records: Vec<Record>,
    /// (level, historical code) → record. A code is unique within a level
    /// only: a district and a commune may share one.
    by_historical: HashMap<(u8, String), usize>,
    /// Historical code → the historical codes it became.
    successors: HashMap<String, Vec<String>>,
    /// Commune number → its live record.
    live_by_code: HashMap<String, usize>,
    /// Historical code of each record, by index.
    historical: Vec<String>,
}

impl Register {
    async fn load(fetcher: &Fetcher) -> Result<Self> {
        let (y, m, d) = crate::calendar::today();
        let today = format!("{d:02}-{m:02}-{y}");
        let mutations = csv(
            fetcher,
            "ch-mutations.csv",
            &format!(
                "{AGV_URL}/mutations?startPeriod={REGISTER_START}&endPeriod={today}&includeTerritoryExchange=false"
            ),
        )
        .await?;
        let [date, initial, terminal] = mutations.columns([
            "MutationDate",
            "InitialHistoricalCode",
            "TerminalHistoricalCode",
        ])?;
        let mut successors: HashMap<String, Vec<String>> = HashMap::new();
        let mut dates: Vec<String> = vec![REGISTER_START.to_string(), today.clone()];
        for row in &mutations.rows {
            successors
                .entry(row[initial].clone())
                .or_default()
                .push(row[terminal].clone());
            dates.push(row[date].replace('.', "-"));
        }
        dates.sort();
        dates.dedup();

        // Every record is valid on the register's first day or on the day
        // of the mutation that created it: the snapshots of those days hold
        // them all, with their parents.
        let mut records = Vec::new();
        let mut historical = Vec::new();
        let mut by_historical = HashMap::new();
        for day in &dates {
            let snapshot = csv(
                fetcher,
                &format!("ch-snapshot-{day}.csv"),
                &format!("{AGV_URL}/snapshot?date={day}"),
            )
            .await?;
            let [id, code, from, to, level, parent, name] = snapshot.columns([
                "HistoricalCode",
                "BfsCode",
                "ValidFrom",
                "ValidTo",
                "Level",
                "Parent",
                "Name",
            ])?;
            for row in &snapshot.rows {
                let key = (row[level].parse().unwrap_or(0), row[id].clone());
                if by_historical.contains_key(&key) {
                    continue;
                }
                by_historical.insert(key, records.len());
                historical.push(row[id].clone());
                records.push(Record {
                    level: row[level].parse().unwrap_or(0),
                    code: row[code].clone(),
                    parent: row[parent].clone(),
                    name: row[name].clone(),
                    valid_from: iso(&row[from]),
                    valid_to: Some(iso(&row[to])).filter(|d| !d.is_empty()),
                });
            }
        }
        let live_by_code = records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.level == 3 && r.valid_to.is_none())
            .map(|(i, r)| (r.code.clone(), i))
            .collect();
        Ok(Self {
            records,
            by_historical,
            successors,
            live_by_code,
            historical,
        })
    }

    /// The canton a commune record belongs to, through its district.
    fn canton_of(&self, index: usize) -> &str {
        let mut record = &self.records[index];
        for _ in 0..4 {
            if record.level == 1 {
                return canton_name(&record.name);
            }
            match self
                .by_historical
                .get(&(record.level.saturating_sub(1), record.parent.clone()))
            {
                Some(&parent) => record = &self.records[parent],
                None => break,
            }
        }
        ""
    }

    /// Where a commune record ended up: the live record it became, and
    /// whether it is still the same commune (renamed, renumbered or moved to
    /// another district) rather than merged into another.
    fn fate(&self, mut index: usize) -> (Option<usize>, bool) {
        let mut same = true;
        for _ in 0..32 {
            let record = &self.records[index];
            if record.valid_to.is_none() {
                return (Some(index), same);
            }
            let Some(next) = self
                .successors
                .get(&self.historical[index])
                .and_then(|codes| {
                    codes
                        .iter()
                        .find_map(|c| self.by_historical.get(&(3, c.clone())))
                })
            else {
                break;
            };
            let next_record = &self.records[*next];
            // A commune that several others joined is a merger, even when
            // it kept one of their names or numbers.
            let merged = self
                .successors
                .iter()
                .filter(|(_, targets)| targets.contains(&self.historical[*next]))
                .count()
                > 1;
            same &= !merged && (next_record.code == record.code || next_record.name == record.name);
            index = *next;
        }
        (None, false)
    }

    fn places(&self, centres: &HashMap<String, Coordinates>) -> Vec<Place> {
        let mut places = Vec::new();
        for (index, record) in self.records.iter().enumerate() {
            if record.level != 3 {
                continue;
            }
            let (live, same) = self.fate(index);
            let kind = match live {
                Some(l) if l == index => Kind::Commune,
                Some(l) if same => {
                    let now = &self.records[l];
                    // Only a district changed: the live record already says
                    // everything this one would.
                    if now.name == record.name
                        && now.code == record.code
                        && self.canton_of(l) == self.canton_of(index)
                    {
                        continue;
                    }
                    Kind::FormerName
                }
                _ => Kind::FormerCommune,
            };
            let mut base = unfiled(Country::Switzerland, &record.name, &record.code, kind);
            if record.valid_from.as_str() > "1848-09-12" {
                base.valid_from = Some(record.valid_from.clone());
            }
            base.valid_until = record.valid_to.clone();
            base.successor = live
                .filter(|&l| l != index)
                .map(|l| self.records[l].code.clone());
            // A former name stands where the commune it still is does.
            base.coordinates = centres
                .get(&record.code)
                .or_else(|| {
                    live.filter(|_| kind == Kind::FormerName)
                        .and_then(|l| centres.get(&self.records[l].code))
                })
                .copied();
            file(
                &mut places,
                &base,
                "",
                self.canton_of(index),
                kind == Kind::Commune,
            );
        }
        places
    }
}

/// A canton by its first official name: "Bern / Berne" is filed as Bern,
/// "Graubünden / Grigioni / Grischun" as Graubünden.
fn canton_name(name: &str) -> &str {
    name.split(" / ").next().unwrap_or(name).trim()
}

/// `DD.MM.YYYY` as `YYYY-MM-DD`; empty stays empty.
fn iso(date: &str) -> String {
    let mut parts = date.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(d), Some(m), Some(y)) => format!("{y}-{m}-{d}"),
        _ => String::new(),
    }
}

async fn csv(fetcher: &Fetcher, name: &str, url: &str) -> Result<Table> {
    let bytes = fetcher.bytes(name, url).await?;
    let text = String::from_utf8(bytes).with_context(|| format!("{name} is not UTF-8"))?;
    Table::parse(&text, ',').with_context(|| format!("cannot read {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_cantons_read_as_filed() {
        assert_eq!(iso("01.04.1961"), "1961-04-01");
        assert_eq!(iso(""), "");
        assert_eq!(
            canton_name("Graubünden / Grigioni / Grischun"),
            "Graubünden"
        );
        assert_eq!(canton_name("Genève"), "Genève");
    }
}
