//! Wikidata queries and helpers shared by the countries.

use std::collections::HashMap;

use anyhow::Result;

use crate::fetch::Fetcher;
use crate::place::Coordinates;

/// The day part of a Wikidata timestamp, when it is an ordinary AD date.
pub fn wikidata_date(field: &str) -> Option<String> {
    let day = field.get(..10)?;
    (day.len() == 10 && day.as_bytes()[4] == b'-' && !field.starts_with('-'))
        .then(|| day.to_string())
}

pub fn push_unique(list: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !list.iter().any(|v| v == value) {
        list.push(value.to_string());
    }
}

/// The coordinates of the items bearing a municipality code `property`, by
/// code.
pub async fn coordinates(
    fetcher: &Fetcher,
    property: &str,
) -> Result<HashMap<String, Coordinates>> {
    let query =
        format!("SELECT ?code ?coord WHERE {{ ?item wdt:{property} ?code ; wdt:P625 ?coord . }}");
    let table = fetcher.sparql(&query).await?;
    let (code, coord) = (table.column("code")?, table.column("coord")?);
    let mut found = HashMap::new();
    for row in &table.rows {
        if let Some(point) = Coordinates::from_wkt(&row[coord]) {
            found.entry(row[code].clone()).or_insert(point);
        }
    }
    Ok(found)
}

/// Which Wikidata items are a country's former municipalities.
pub struct FormerQuery<'a> {
    /// The country's item, such as `Q183`.
    pub country: &'a str,
    /// A pattern binding `?statement`, the instance-of statement that makes
    /// `?item` a municipality, so its end date can be read.
    pub instance: &'a str,
    /// The property holding the country's municipality code, such as `P439`.
    pub code: &'a str,
    /// Label languages, preferred first.
    pub languages: &'a [&'a str],
    /// Only municipalities that ended before this year.
    pub before_year: Option<u16>,
    /// Whether an item with no end date counts: when the class itself says
    /// the municipality is gone, a missing date is only a missing date.
    pub undated: bool,
}

/// A former municipality, as Wikidata describes it.
#[derive(Debug, Default)]
pub struct Former {
    pub name: String,
    /// Its own municipality code, when it had one.
    pub code: String,
    pub start: Option<String>,
    pub end: Option<String>,
    pub coordinates: Option<Coordinates>,
    /// Codes of the municipalities it was merged into or lies within, best
    /// first.
    pub codes: Vec<String>,
}

pub async fn former_municipalities(
    fetcher: &Fetcher,
    query: &FormerQuery<'_>,
) -> Result<Vec<Former>> {
    let labels: String = query
        .languages
        .iter()
        .enumerate()
        .map(|(i, lang)| {
            format!("OPTIONAL {{ ?item rdfs:label ?l{i} . FILTER(LANG(?l{i}) = \"{lang}\") }}\n")
        })
        .collect();
    let coalesce = (0..query.languages.len())
        .map(|i| format!("?l{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let dated = match query.before_year {
        Some(year) => format!("BOUND(?end) && YEAR(?end) < {year}"),
        None => "BOUND(?end)".to_string(),
    };
    let filter = if query.undated {
        format!("!BOUND(?end) || ({dated})")
    } else {
        dated
    };
    let sparql = format!(
        r#"SELECT ?item ?label ?own ?start ?end ?coord ?successor ?parent WHERE {{
  {instance}
  ?item wdt:P17 wd:{country} .
  OPTIONAL {{ ?item wdt:P576 ?dissolved }}
  OPTIONAL {{ ?statement pq:P582 ?ended }}
  BIND(COALESCE(?dissolved, ?ended) AS ?end)
  FILTER({filter})
  {labels}  BIND(COALESCE({coalesce}) AS ?label)
  FILTER(BOUND(?label))
  OPTIONAL {{ ?item wdt:{code} ?own }}
  OPTIONAL {{ ?item wdt:P571 ?start }}
  OPTIONAL {{ ?item wdt:P625 ?coord }}
  OPTIONAL {{ ?item wdt:P1366 ?next . ?next wdt:{code} ?successor }}
  OPTIONAL {{ ?item wdt:P131 ?within . ?within wdt:{code} ?parent }}
}}"#,
        instance = query.instance,
        country = query.country,
        code = query.code,
    );
    let table = fetcher.sparql(&sparql).await?;
    let (item, label, own, start, end, coord, successor, parent) = (
        table.column("item")?,
        table.column("label")?,
        table.column("own")?,
        table.column("start")?,
        table.column("end")?,
        table.column("coord")?,
        table.column("successor")?,
        table.column("parent")?,
    );
    let mut by_item: HashMap<String, Former> = HashMap::new();
    for row in &table.rows {
        let ended = wikidata_date(&row[end]);
        if ended.is_none() && !query.undated {
            continue;
        }
        let former = by_item.entry(row[item].clone()).or_default();
        former.name.clone_from(&row[label]);
        if former.code.is_empty() {
            former.code.clone_from(&row[own]);
        }
        former.start = former.start.take().or_else(|| wikidata_date(&row[start]));
        former.end = former.end.take().or(ended);
        former.coordinates = former
            .coordinates
            .or_else(|| Coordinates::from_wkt(&row[coord]));
        push_unique(&mut former.codes, &row[successor]);
        push_unique(&mut former.codes, &row[parent]);
    }
    let mut former: Vec<Former> = by_item.into_values().collect();
    former.sort_by(|a, b| (&a.name, &a.end).cmp(&(&b.name, &b.end)));
    Ok(former)
}
