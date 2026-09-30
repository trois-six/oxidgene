//! France: every commune since 1943 from INSEE's Code officiel géographique,
//! the communes merged away before 1943 from Wikidata, and each one under
//! every département and region name it has been filed under.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Place, Region, file};
use crate::table::Table;
use crate::wikidata::{push_unique, wikidata_date};
use oxidgene_core::search::fold_words;

/// INSEE publishes a new vintage of the COG every year; data.gouv.fr lists
/// them all, which is how the latest is found.
const COG_DATASET_URL: &str =
    "https://www.data.gouv.fr/api/1/datasets/code-officiel-geographique-1/";
const GEO_API_URL: &str = "https://geo.api.gouv.fr/communes";

/// INSEE's history starts on this day; a record starting then may be older.
const COG_HORIZON: &str = "1943-01-01";
/// The day the thirteen current metropolitan regions replaced the old ones.
const REGIONS_2016: &str = "2016-01-01";

/// Names a département bore before a date. A département whose territory
/// changed with its code (Seine, Seine-et-Oise, Corse) is listed here too:
/// its communes were renumbered at the same date, so a commune's old code
/// alone says which name it was filed under.
const FORMER_DEPARTMENT_NAMES: &[(&str, &str, &str)] = &[
    ("04", "Basses-Alpes", "1970-04-13"),
    ("17", "Charente-Inférieure", "1941-09-04"),
    ("20", "Corse", "1976-01-01"),
    ("22", "Côtes-du-Nord", "1990-02-27"),
    ("44", "Loire-Inférieure", "1957-03-09"),
    ("64", "Basses-Pyrénées", "1969-10-10"),
    ("75", "Seine", "1968-01-01"),
    ("76", "Seine-Inférieure", "1955-01-18"),
    ("78", "Seine-et-Oise", "1968-01-01"),
];

/// The regions of 1982–2015, by département. Overseas regions kept their
/// names and are not listed.
fn region_before_2016(department: &str) -> Option<&'static str> {
    Some(match department {
        "67" | "68" => "Alsace",
        "24" | "33" | "40" | "47" | "64" => "Aquitaine",
        "03" | "15" | "43" | "63" => "Auvergne",
        "14" | "50" | "61" => "Basse-Normandie",
        "21" | "58" | "71" | "89" => "Bourgogne",
        "22" | "29" | "35" | "56" => "Bretagne",
        "18" | "28" | "36" | "37" | "41" | "45" => "Centre",
        "08" | "10" | "51" | "52" => "Champagne-Ardenne",
        "20" | "2A" | "2B" => "Corse",
        "25" | "39" | "70" | "90" => "Franche-Comté",
        "27" | "76" => "Haute-Normandie",
        "75" | "77" | "78" | "91" | "92" | "93" | "94" | "95" => "Île-de-France",
        "11" | "30" | "34" | "48" | "66" => "Languedoc-Roussillon",
        "19" | "23" | "87" => "Limousin",
        "54" | "55" | "57" | "88" => "Lorraine",
        "09" | "12" | "31" | "32" | "46" | "65" | "81" | "82" => "Midi-Pyrénées",
        "59" | "62" => "Nord-Pas-de-Calais",
        "44" | "49" | "53" | "72" | "85" => "Pays de la Loire",
        "02" | "60" | "80" => "Picardie",
        "16" | "17" | "79" | "86" => "Poitou-Charentes",
        "04" | "05" | "06" | "13" | "83" | "84" => "Provence-Alpes-Côte d'Azur",
        "01" | "07" | "26" | "38" | "42" | "69" | "73" | "74" => "Rhône-Alpes",
        _ => return None,
    })
}

/// Code changes that keep the commune itself: a new name, a new
/// département, a new chef-lieu. Every other event ends the commune.
const SAME_COMMUNE_EVENTS: &[&str] = &["10", "41", "50"];

const WIKIDATA_COORDINATES: &str = r#"
SELECT ?code ?coord WHERE { ?item wdt:P374 ?code ; wdt:P625 ?coord . }"#;

/// French labels of the items bearing the INSEE codes put in place of CODES.
const WIKIDATA_LABELS: &str = r#"
SELECT ?code ?label WHERE {
  VALUES ?code { CODES }
  ?item wdt:P374 ?code ; rdfs:label ?label . FILTER(LANG(?label) = "fr")
}"#;
const LABEL_BATCH: usize = 800;

/// Communes of France dissolved before INSEE's history begins.
const WIKIDATA_FORMER: &str = r#"
SELECT ?item ?label ?start ?end ?coord ?successor WHERE {
  ?item p:P31 ?statement . ?statement ps:P31 wd:Q484170 .
  OPTIONAL { ?item wdt:P576 ?dissolved }
  OPTIONAL { ?statement pq:P582 ?ended }
  BIND(COALESCE(?dissolved, ?ended) AS ?end)
  FILTER(BOUND(?end) && YEAR(?end) < 1943)
  ?item rdfs:label ?label . FILTER(LANG(?label) = "fr")
  OPTIONAL { ?item wdt:P571 ?start }
  OPTIONAL { ?item wdt:P625 ?coord }
  OPTIONAL { ?item wdt:P1366 ?next . ?next wdt:P374 ?successor }
}"#;

/// Where those communes lay: the commune or département that contains them,
/// one or two levels up (a canton or an arrondissement often sits between).
const WIKIDATA_FORMER_PARENTS: &str = r#"
SELECT ?item ?code ?department ?code2 ?department2 WHERE {
  ?item p:P31 ?statement . ?statement ps:P31 wd:Q484170 .
  OPTIONAL { ?item wdt:P576 ?dissolved }
  OPTIONAL { ?statement pq:P582 ?ended }
  BIND(COALESCE(?dissolved, ?ended) AS ?end)
  FILTER(BOUND(?end) && YEAR(?end) < 1943)
  ?item wdt:P131 ?parent .
  OPTIONAL { ?parent wdt:P374 ?code }
  OPTIONAL { ?parent wdt:P2586 ?department }
  OPTIONAL {
    ?parent wdt:P131 ?grandparent .
    OPTIONAL { ?grandparent wdt:P374 ?code2 }
    OPTIONAL { ?grandparent wdt:P2586 ?department2 }
  }
}"#;

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let cog = Cog::load(fetcher).await?;
    let centres = centres(fetcher).await?;
    let wikidata = WikidataCodes::load(fetcher, &cog).await?;

    let mut places = Vec::new();
    let mut known = HashSet::new();
    for index in 0..cog.records.len() {
        for place in cog.places_of(index, &centres, &wikidata) {
            known.insert((fold_words(&place.name), place.subdivision.clone()));
            places.push(place);
        }
    }
    let insee = places.len();

    let former = FormerCommunes::load(fetcher).await?;
    for place in former.places(&cog, &centres) {
        if !known.contains(&(fold_words(&place.name), place.subdivision.clone())) {
            places.push(place);
        }
    }
    eprintln!(
        "France: {insee} rows from INSEE, {} from Wikidata",
        places.len() - insee
    );
    Ok(places)
}

fn department_of(code: &str) -> &str {
    let width = if code.starts_with("97") { 3 } else { 2 };
    code.get(..width).unwrap_or(code)
}

/// A date column: `None` when empty.
fn date(field: &str) -> Option<String> {
    (!field.is_empty()).then(|| field.to_string())
}

/// One name a commune bore between two dates under one code.
#[derive(Debug)]
struct Record {
    arrondissement: bool,
    code: String,
    name: String,
    /// `None` when the commune already existed when INSEE's history begins.
    start: Option<String>,
    end: Option<String>,
}

impl Record {
    fn covers(&self, day: &str) -> bool {
        self.start.as_deref().is_none_or(|s| s <= day)
            && self.end.as_deref().is_none_or(|e| day < e)
    }

    fn began_before(&self, day: &str) -> bool {
        self.start.as_deref().is_none_or(|s| s < day)
    }
}

/// An event that turned a commune of one code into one of another.
#[derive(Debug)]
struct Event {
    kind: String,
    code_after: String,
}

/// Where a record's commune stands today.
struct Fate {
    live: Option<usize>,
    /// Only renames and renumberings on the way: the commune still exists.
    same_commune: bool,
}

struct Cog {
    records: Vec<Record>,
    by_code: HashMap<String, Vec<usize>>,
    /// (code before, date, arrondissement?) → events.
    events: HashMap<(String, String, bool), Vec<Event>>,
    /// Code after → (date, code before), for renumberings only.
    renumbered_from: HashMap<String, Vec<(String, String)>>,
    /// Current département code → (name, current region name).
    departments: HashMap<String, (String, String)>,
}

impl Cog {
    async fn load(fetcher: &Fetcher) -> Result<Self> {
        let (year, files) = latest_cog(fetcher).await?;
        eprintln!("France: COG {year}");
        let table = |name: String| {
            let files = &files;
            async move {
                let url = files
                    .get(&name)
                    .with_context(|| format!("the COG {year} has no {name}"))?;
                let bytes = fetcher.bytes(&name, url).await?;
                let text =
                    String::from_utf8(bytes).with_context(|| format!("{name} is not UTF-8"))?;
                Table::parse(&text, ',').with_context(|| format!("cannot read {name}"))
            }
        };
        let history = table("v_commune_depuis_1943.csv".to_string()).await?;
        let movements = table(format!("v_mvt_commune_{year}.csv")).await?;
        let departments = table(format!("v_departement_{year}.csv")).await?;
        let regions = table(format!("v_region_{year}.csv")).await?;

        let mut records = Vec::new();
        let (typecom, code, name, start, end) = (
            history.column("TYPECOM")?,
            history.column("COM")?,
            history.column("LIBELLE")?,
            history.column("DATE_DEBUT")?,
            history.column("DATE_FIN")?,
        );
        for row in &history.rows {
            records.push(Record {
                arrondissement: row[typecom] == "ARM",
                code: row[code].clone(),
                name: row[name].clone(),
                start: date(&row[start]).filter(|s| s != COG_HORIZON),
                end: date(&row[end]),
            });
        }
        let mut by_code: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, record) in records.iter().enumerate() {
            by_code.entry(record.code.clone()).or_default().push(index);
        }

        let mut events: HashMap<_, Vec<Event>> = HashMap::new();
        let mut renumbered_from: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let (kind, day, type_before, code_before, type_after, code_after) = (
            movements.column("MOD")?,
            movements.column("DATE_EFF")?,
            movements.column("TYPECOM_AV")?,
            movements.column("COM_AV")?,
            movements.column("TYPECOM_AP")?,
            movements.column("COM_AP")?,
        );
        for row in &movements.rows {
            // Only a commune becoming a commune carries it forward; the rows
            // that turn it into a commune déléguée or associée describe what
            // is left of it inside its successor.
            if row[type_before] != row[type_after] || !["COM", "ARM"].contains(&&*row[type_after]) {
                continue;
            }
            if row[kind] == "41" || row[kind] == "50" {
                renumbered_from
                    .entry(row[code_after].clone())
                    .or_default()
                    .push((row[day].clone(), row[code_before].clone()));
            }
            events
                .entry((
                    row[code_before].clone(),
                    row[day].clone(),
                    row[type_before] == "ARM",
                ))
                .or_default()
                .push(Event {
                    kind: row[kind].clone(),
                    code_after: row[code_after].clone(),
                });
        }
        for list in events.values_mut() {
            list.sort_by(|a, b| a.code_after.cmp(&b.code_after));
        }

        let region_names: HashMap<_, _> = {
            let (code, name) = (regions.column("REG")?, regions.column("LIBELLE")?);
            regions
                .rows
                .iter()
                .map(|r| (r[code].clone(), r[name].clone()))
                .collect()
        };
        let (code, region, name) = (
            departments.column("DEP")?,
            departments.column("REG")?,
            departments.column("LIBELLE")?,
        );
        let departments = departments
            .rows
            .iter()
            .map(|r| {
                let region = region_names
                    .get(&r[region])
                    .with_context(|| format!("unknown region {}", r[region]))?;
                Ok((r[code].clone(), (r[name].clone(), region.clone())))
            })
            .collect::<Result<_>>()?;

        Ok(Self {
            records,
            by_code,
            events,
            renumbered_from,
            departments,
        })
    }

    fn record_at(&self, code: &str, day: &str, arrondissement: bool) -> Option<usize> {
        self.by_code.get(code)?.iter().copied().find(|&i| {
            self.records[i].arrondissement == arrondissement && self.records[i].covers(day)
        })
    }

    /// The live record a commune's territory ended up in, if any.
    fn fate(&self, mut index: usize) -> Fate {
        let mut same_commune = true;
        // Chains are a handful of steps; the bound only guards a cycle in
        // the source.
        for _ in 0..64 {
            let record = &self.records[index];
            let Some(end) = &record.end else {
                return Fate {
                    live: Some(index),
                    same_commune,
                };
            };
            let key = (record.code.clone(), end.clone(), record.arrondissement);
            let next = self.events.get(&key).and_then(|events| {
                events.iter().find_map(|event| {
                    let next = self.record_at(&event.code_after, end, record.arrondissement)?;
                    Some((next, SAME_COMMUNE_EVENTS.contains(&&*event.kind)))
                })
            });
            let Some((next, same)) = next else { break };
            same_commune &= same;
            index = next;
        }
        Fate {
            live: None,
            same_commune: false,
        }
    }

    /// The département a live commune's code belonged to on a given day,
    /// walking its renumberings backwards.
    fn department_at(&self, live_code: &str, day: &str) -> String {
        let mut code = live_code.to_string();
        for _ in 0..8 {
            let earlier = self.renumbered_from.get(&code).and_then(|changes| {
                changes
                    .iter()
                    .filter(|(changed, _)| day < changed.as_str())
                    .min_by(|a, b| a.0.cmp(&b.0))
            });
            match earlier {
                Some((_, before)) => code = before.clone(),
                None => break,
            }
        }
        department_of(&code).to_string()
    }

    fn places_of(
        &self,
        index: usize,
        centres: &HashMap<String, Coordinates>,
        wikidata: &WikidataCodes,
    ) -> Vec<Place> {
        let record = &self.records[index];
        let fate = self.fate(index);
        let live = fate.live.map(|i| &self.records[i]);
        let kind = match fate.live {
            Some(live) if live == index && record.arrondissement => Kind::MunicipalArrondissement,
            Some(live) if live == index => Kind::Commune,
            Some(_) if fate.same_commune => Kind::FormerName,
            _ => Kind::FormerCommune,
        };
        let own = department_of(&record.code);
        let today = live.map_or(own, |l| department_of(&l.code));
        let name = wikidata
            .accented(&record.code, &record.name)
            .unwrap_or(&record.name);
        // A former name of a commune that still exists stands where it does.
        let located = match kind {
            Kind::FormerName => live.map_or(&record.code, |l| &l.code),
            _ => &record.code,
        };
        let coordinates = centres
            .get(located)
            .or_else(|| wikidata.coordinates.get(&record.code))
            .copied();
        let base = Place {
            name: name.to_string(),
            code: record.code.clone(),
            subdivision: String::new(),
            region: Region::Named(String::new()),
            country: Country::France,
            kind,
            valid_from: record.start.clone(),
            valid_until: record.end.clone(),
            successor: live
                .filter(|_| kind != Kind::Commune && kind != Kind::MunicipalArrondissement)
                .map(|l| l.code.clone()),
            coordinates,
            current: false,
        };

        let mut places = Vec::new();
        let old_region = region_before_2016(today).map(str::to_string);
        // A code renumbered since (a commune of Seine-et-Oise, say) never
        // stood under today's name for its département.
        if own == today
            && let Some((department, region)) = self.departments.get(today)
        {
            file(&mut places, &base, department, region, true);
            if record.began_before(REGIONS_2016)
                && let Some(old) = &old_region
            {
                file(&mut places, &base, department, old, false);
            }
        }
        let region = old_region.or_else(|| self.departments.get(today).map(|d| d.1.clone()));
        if let Some(region) = region {
            for (_, former, until) in FORMER_DEPARTMENT_NAMES.iter().filter(|d| d.0 == own) {
                if record.began_before(until) {
                    file(&mut places, &base, former, &region, false);
                }
            }
        }
        places
    }
}

/// The latest COG vintage, and the URL of each of its files by file name.
async fn latest_cog(fetcher: &Fetcher) -> Result<(u16, HashMap<String, String>)> {
    let bytes = fetcher.bytes("cog-dataset.json", COG_DATASET_URL).await?;
    let dataset: serde_json::Value = serde_json::from_slice(&bytes)?;
    let resources = dataset["resources"]
        .as_array()
        .context("the COG dataset lists no resources")?;
    // Titles read "Millésime 2026 : Communes depuis 1943".
    let vintage = |resource: &serde_json::Value| -> Option<(u16, String)> {
        let title = resource["title"].as_str()?;
        let year = title.strip_prefix("Millésime ")?.get(..4)?.parse().ok()?;
        Some((year, resource["url"].as_str()?.to_string()))
    };
    let year = resources
        .iter()
        .filter_map(vintage)
        .map(|(year, _)| year)
        .max()
        .context("the COG dataset has no vintage")?;
    let files = resources
        .iter()
        .filter_map(vintage)
        .filter(|(y, _)| *y == year)
        .filter_map(|(_, url)| Some((url.rsplit('/').next()?.to_string(), url)))
        .collect();
    Ok((year, files))
}

/// The centre of every current commune and municipal arrondissement.
async fn centres(fetcher: &Fetcher) -> Result<HashMap<String, Coordinates>> {
    let mut centres = HashMap::new();
    for kind in ["commune-actuelle", "arrondissement-municipal"] {
        let url = format!("{GEO_API_URL}?type={kind}&fields=code,centre&format=json");
        let bytes = fetcher.bytes(&format!("geo-{kind}.json"), &url).await?;
        let communes: serde_json::Value = serde_json::from_slice(&bytes)?;
        for commune in communes
            .as_array()
            .context("geo.api did not answer a list")?
        {
            let code = commune["code"].as_str().context("a commune has no code")?;
            let point = &commune["centre"]["coordinates"];
            if let (Some(longitude), Some(latitude)) = (point[0].as_f64(), point[1].as_f64()) {
                centres.insert(
                    code.to_string(),
                    Coordinates {
                        latitude,
                        longitude,
                    },
                );
            }
        }
    }
    Ok(centres)
}

/// Coordinates Wikidata holds for INSEE codes, and its spelling of the
/// former communes INSEE writes without accents ("Eglise" for "Église").
struct WikidataCodes {
    coordinates: HashMap<String, Coordinates>,
    labels: HashMap<String, Vec<String>>,
}

impl WikidataCodes {
    async fn load(fetcher: &Fetcher, cog: &Cog) -> Result<Self> {
        let table = fetcher.sparql(WIKIDATA_COORDINATES).await?;
        let (code, coord) = (table.column("code")?, table.column("coord")?);
        let mut coordinates = HashMap::new();
        for row in &table.rows {
            if let Some(point) = Coordinates::from_wkt(&row[coord]) {
                coordinates.entry(row[code].clone()).or_insert(point);
            }
        }

        // Asking for the label of every code times the query service out;
        // only the unaccented names of former communes need one.
        let mut codes: Vec<&str> = cog
            .records
            .iter()
            .filter(|r| r.end.is_some() && r.name.is_ascii())
            .map(|r| r.code.as_str())
            .collect();
        codes.sort_unstable();
        codes.dedup();
        let mut labels: HashMap<String, Vec<String>> = HashMap::new();
        for batch in codes.chunks(LABEL_BATCH) {
            let values = batch.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>();
            let query = WIKIDATA_LABELS.replace("CODES", &values.join(" "));
            let table = fetcher.sparql(&query).await?;
            let (code, label) = (table.column("code")?, table.column("label")?);
            for row in &table.rows {
                labels
                    .entry(row[code].clone())
                    .or_default()
                    .push(row[label].clone());
            }
        }
        Ok(Self {
            coordinates,
            labels,
        })
    }

    /// Wikidata's spelling of `name`, when it only adds accents or case.
    fn accented(&self, code: &str, name: &str) -> Option<&str> {
        let key = fold_words(name);
        self.labels.get(code)?.iter().find_map(|label| {
            (label != name && fold_words(label) == key && accents(label) > accents(name))
                .then_some(label.as_str())
        })
    }
}

fn accents(name: &str) -> usize {
    name.chars().filter(|c| !c.is_ascii()).count()
}

/// A commune dissolved before 1943, as Wikidata describes it.
#[derive(Default)]
struct FormerCommune {
    name: String,
    start: Option<String>,
    end: String,
    coordinates: Option<Coordinates>,
    /// Commune codes it was merged into or lay within, best first.
    codes: Vec<String>,
    departments: Vec<String>,
}

struct FormerCommunes(Vec<FormerCommune>);

impl FormerCommunes {
    async fn load(fetcher: &Fetcher) -> Result<Self> {
        let items = fetcher.sparql(WIKIDATA_FORMER).await?;
        let parents = fetcher.sparql(WIKIDATA_FORMER_PARENTS).await?;

        let mut communes: HashMap<String, FormerCommune> = HashMap::new();
        let (item, label, start, end, coord, successor) = (
            items.column("item")?,
            items.column("label")?,
            items.column("start")?,
            items.column("end")?,
            items.column("coord")?,
            items.column("successor")?,
        );
        for row in &items.rows {
            let Some(ended) = wikidata_date(&row[end]) else {
                continue;
            };
            let commune = communes.entry(row[item].clone()).or_default();
            commune.name.clone_from(&row[label]);
            commune.start = commune.start.take().or_else(|| wikidata_date(&row[start]));
            commune.end = ended;
            commune.coordinates = commune
                .coordinates
                .or_else(|| Coordinates::from_wkt(&row[coord]));
            push_unique(&mut commune.codes, &row[successor]);
        }
        let columns = [
            ("code", false),
            ("code2", false),
            ("department", true),
            ("department2", true),
        ];
        let columns = columns
            .iter()
            .map(|(name, department)| Ok((parents.column(name)?, *department)))
            .collect::<Result<Vec<_>>>()?;
        let item = parents.column("item")?;
        for row in &parents.rows {
            let Some(commune) = communes.get_mut(&row[item]) else {
                continue;
            };
            for &(column, department) in &columns {
                let list = if department {
                    &mut commune.departments
                } else {
                    &mut commune.codes
                };
                push_unique(list, &row[column]);
            }
        }
        let mut communes: Vec<_> = communes.into_values().collect();
        // A total order: homonyms dissolved the same day come out the same
        // way on every run.
        communes.sort_by(|a, b| {
            (&a.name, &a.end, &a.start, &a.codes, &a.departments).cmp(&(
                &b.name,
                &b.end,
                &b.start,
                &b.codes,
                &b.departments,
            ))
        });
        Ok(Self(communes))
    }

    fn places(&self, cog: &Cog, centres: &HashMap<String, Coordinates>) -> Vec<Place> {
        let mut by_department: HashMap<&str, Vec<(&str, Coordinates)>> = HashMap::new();
        for (code, centre) in centres {
            by_department
                .entry(department_of(code))
                .or_default()
                .push((code, *centre));
        }
        let mut places = Vec::new();
        for commune in &self.0 {
            let successor = commune.codes.iter().find_map(|code| {
                let index = cog
                    .record_at(code, &commune.end, false)
                    .or_else(|| cog.by_code.get(code)?.first().copied())?;
                cog.fate(index).live.map(|i| cog.records[i].code.clone())
            });
            // Without a successor, the nearest current commune in the same
            // département says which département holds the land today. It
            // is not claimed as the successor: a neighbour is not always the
            // commune that absorbed it.
            let neighbour = successor.clone().or_else(|| {
                let origin = commune.coordinates?;
                let departments: Vec<&str> = commune
                    .departments
                    .iter()
                    .flat_map(|d| split_department(d))
                    .collect();
                departments
                    .iter()
                    .flat_map(|d| by_department.get(d).into_iter().flatten())
                    .min_by(|a, b| origin.distance2(a.1).total_cmp(&origin.distance2(b.1)))
                    .map(|(code, _)| (*code).to_string())
            });
            let Some(neighbour) = neighbour else { continue };
            let today = department_of(&neighbour);
            let then = cog.department_at(&neighbour, &commune.end);
            let Some((department, region)) = cog.departments.get(today) else {
                continue;
            };
            let base = Place {
                name: commune.name.clone(),
                code: String::new(),
                subdivision: String::new(),
                region: Region::Named(String::new()),
                country: Country::France,
                kind: Kind::FormerCommune,
                valid_from: commune.start.clone(),
                valid_until: Some(commune.end.clone()),
                successor: successor.clone(),
                coordinates: commune.coordinates,
                current: false,
            };
            let old_region = region_before_2016(today).unwrap_or(region);
            let mut filed = Vec::new();
            file(&mut filed, &base, department, region, true);
            file(&mut filed, &base, department, old_region, false);
            for (_, former, until) in FORMER_DEPARTMENT_NAMES.iter().filter(|d| d.0 == then) {
                if commune.start.as_deref().is_none_or(|s| s < *until) {
                    file(&mut filed, &base, former, old_region, false);
                }
            }
            places.extend(filed);
        }
        places
    }
}

/// The current départements that took over a renumbered one's territory.
fn split_department(code: &str) -> Vec<&str> {
    match code {
        "20" => vec!["2A", "2B"],
        "75" => vec!["75", "92", "93", "94"],
        "78" => vec!["78", "91", "92", "93", "94", "95"],
        _ => vec![code],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fictitious history: commune 22901 renamed in 1950, commune 22902
    /// merged into it in 1960, and commune 78903 renumbered 91903 in 1968.
    fn cog() -> Cog {
        let record = |code: &str, name: &str, start: Option<&str>, end: Option<&str>| Record {
            arrondissement: false,
            code: code.to_string(),
            name: name.to_string(),
            start: start.map(str::to_string),
            end: end.map(str::to_string),
        };
        let records = vec![
            record("22901", "Bourg-Ancien", None, Some("1950-01-01")),
            record("22901", "Bourg-Neuf", Some("1950-01-01"), None),
            record("22902", "Hameau-Clos", None, Some("1960-01-01")),
            record("78903", "Village-Double", None, Some("1968-01-01")),
            record("91903", "Village-Double", Some("1968-01-01"), None),
        ];
        let mut by_code: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, r) in records.iter().enumerate() {
            by_code.entry(r.code.clone()).or_default().push(i);
        }
        let event = |kind: &str, code_after: &str| Event {
            kind: kind.to_string(),
            code_after: code_after.to_string(),
        };
        let events = HashMap::from([
            (
                ("22901".to_string(), "1950-01-01".to_string(), false),
                vec![event("10", "22901")],
            ),
            (
                ("22902".to_string(), "1960-01-01".to_string(), false),
                vec![event("31", "22901")],
            ),
            (
                ("78903".to_string(), "1968-01-01".to_string(), false),
                vec![event("41", "91903")],
            ),
        ]);
        let renumbered_from = HashMap::from([(
            "91903".to_string(),
            vec![("1968-01-01".to_string(), "78903".to_string())],
        )]);
        let departments = [
            ("22", "Côtes-d'Armor", "Bretagne"),
            ("78", "Yvelines", "Île-de-France"),
            ("91", "Essonne", "Île-de-France"),
        ]
        .into_iter()
        .map(|(c, n, r)| (c.to_string(), (n.to_string(), r.to_string())))
        .collect();
        Cog {
            records,
            by_code,
            events,
            renumbered_from,
            departments,
        }
    }

    fn rows(cog: &Cog, index: usize) -> Vec<(Kind, String, String, Option<String>)> {
        let wikidata = WikidataCodes {
            coordinates: HashMap::new(),
            labels: HashMap::new(),
        };
        cog.places_of(index, &HashMap::new(), &wikidata)
            .into_iter()
            .map(|p| {
                let Region::Named(region) = p.region else {
                    unreachable!()
                };
                (p.kind, p.subdivision, region, p.successor)
            })
            .collect()
    }

    #[test]
    fn a_live_commune_is_filed_under_every_name_of_its_departement() {
        assert_eq!(
            rows(&cog(), 1),
            vec![
                (
                    Kind::Commune,
                    "Côtes-d'Armor".into(),
                    "Bretagne".into(),
                    None
                ),
                (
                    Kind::Commune,
                    "Côtes-du-Nord".into(),
                    "Bretagne".into(),
                    None
                ),
            ]
        );
    }

    #[test]
    fn only_todays_filing_is_marked_current() {
        let wikidata = WikidataCodes {
            coordinates: HashMap::new(),
            labels: HashMap::new(),
        };
        let cog = cog();
        let current = |index| {
            cog.places_of(index, &HashMap::new(), &wikidata)
                .into_iter()
                .map(|p| (p.subdivision, p.current))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            current(1),
            vec![
                ("Côtes-d'Armor".to_string(), true),
                ("Côtes-du-Nord".to_string(), false)
            ]
        );
        // A renumbered code was never filed under today's département.
        assert_eq!(current(3), vec![("Seine-et-Oise".to_string(), false)]);
    }

    #[test]
    fn a_new_commune_is_not_filed_under_names_that_ended_before_it() {
        let mut cog = cog();
        cog.records[1].start = Some("2019-01-01".to_string());
        assert_eq!(
            rows(&cog, 1),
            vec![(
                Kind::Commune,
                "Côtes-d'Armor".into(),
                "Bretagne".into(),
                None
            )]
        );
    }

    #[test]
    fn a_renamed_commune_keeps_its_old_name_as_a_former_name() {
        let rows = rows(&cog(), 0);
        assert!(rows.iter().all(|r| r.0 == Kind::FormerName));
        assert_eq!(rows[0].3.as_deref(), Some("22901"));
    }

    #[test]
    fn a_merged_commune_points_at_the_commune_that_absorbed_it() {
        let rows = rows(&cog(), 2);
        assert!(rows.iter().all(|r| r.0 == Kind::FormerCommune));
        assert!(rows.iter().all(|r| r.3.as_deref() == Some("22901")));
        assert!(rows.iter().any(|r| r.1 == "Côtes-du-Nord"));
    }

    #[test]
    fn a_renumbered_code_is_filed_only_under_its_old_departement() {
        assert_eq!(
            rows(&cog(), 3),
            vec![(
                Kind::FormerName,
                "Seine-et-Oise".into(),
                "Île-de-France".into(),
                Some("91903".into())
            )]
        );
        let live = rows(&cog(), 4);
        assert!(live.iter().all(|r| r.1 == "Essonne"));
    }

    #[test]
    fn the_departement_of_a_date_follows_renumberings_backwards() {
        let cog = cog();
        assert_eq!(cog.department_at("91903", "1900-01-01"), "78");
        assert_eq!(cog.department_at("91903", "1990-01-01"), "91");
    }

    #[test]
    fn every_current_metropolitan_departement_has_a_former_region() {
        let departments = (1..=95)
            .filter(|n| *n != 20)
            .map(|n| format!("{n:02}"))
            .chain(["2A".to_string(), "2B".to_string()]);
        for department in departments {
            assert!(
                region_before_2016(&department).is_some(),
                "{department} has no region"
            );
        }
    }
}
