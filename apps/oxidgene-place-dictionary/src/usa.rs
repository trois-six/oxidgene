//! United States: the incorporated places and census-designated places of
//! the Census Bureau's gazetteer, and the towns of New England, which are
//! county subdivisions rather than places. Each is filed under its county
//! and state; a place spanning several counties under each of them.

use std::collections::{HashMap, HashSet};
use std::io::Read;

use anyhow::{Context, Result};

use crate::fetch::Fetcher;
use crate::place::{Coordinates, Country, Kind, Place, file, unfiled};
use crate::table::{Table, decode_mixed};

const GAZETTEER_URL: &str = "https://www2.census.gov/geo/docs/maps-data/data/gazetteer";
/// Which counties each place lies in, as of the 2020 census.
const PLACE_BY_COUNTY_URL: &str =
    "https://www2.census.gov/geo/docs/reference/codes2020/national_place_by_county2020.txt";

/// New England governs by town, and its towns are county subdivisions.
const NEW_ENGLAND: &[&str] = &["09", "23", "25", "33", "44", "50"];

/// The legal descriptions the Census Bureau appends to names, longest first:
/// "Abbeville city" is Abbeville.
const DESCRIPTIONS: &[&str] = &[
    " consolidated government (balance)",
    " metropolitan government (balance)",
    " metro government (balance)",
    " unified government (balance)",
    " city and borough",
    " charter township",
    " (balance)",
    " urban county",
    " municipality",
    " zona urbana",
    " comunidad",
    " plantation",
    " township",
    " borough",
    " village",
    " city",
    " town",
    " CDP",
];

const STATES: &[(&str, &str)] = &[
    ("01", "Alabama"),
    ("02", "Alaska"),
    ("04", "Arizona"),
    ("05", "Arkansas"),
    ("06", "California"),
    ("08", "Colorado"),
    ("09", "Connecticut"),
    ("10", "Delaware"),
    ("11", "District of Columbia"),
    ("12", "Florida"),
    ("13", "Georgia"),
    ("15", "Hawaii"),
    ("16", "Idaho"),
    ("17", "Illinois"),
    ("18", "Indiana"),
    ("19", "Iowa"),
    ("20", "Kansas"),
    ("21", "Kentucky"),
    ("22", "Louisiana"),
    ("23", "Maine"),
    ("24", "Maryland"),
    ("25", "Massachusetts"),
    ("26", "Michigan"),
    ("27", "Minnesota"),
    ("28", "Mississippi"),
    ("29", "Missouri"),
    ("30", "Montana"),
    ("31", "Nebraska"),
    ("32", "Nevada"),
    ("33", "New Hampshire"),
    ("34", "New Jersey"),
    ("35", "New Mexico"),
    ("36", "New York"),
    ("37", "North Carolina"),
    ("38", "North Dakota"),
    ("39", "Ohio"),
    ("40", "Oklahoma"),
    ("41", "Oregon"),
    ("42", "Pennsylvania"),
    ("44", "Rhode Island"),
    ("45", "South Carolina"),
    ("46", "South Dakota"),
    ("47", "Tennessee"),
    ("48", "Texas"),
    ("49", "Utah"),
    ("50", "Vermont"),
    ("51", "Virginia"),
    ("53", "Washington"),
    ("54", "West Virginia"),
    ("55", "Wisconsin"),
    ("56", "Wyoming"),
    ("72", "Puerto Rico"),
];

pub async fn places(fetcher: &Fetcher) -> Result<Vec<Place>> {
    let year = latest_year(fetcher).await?;
    eprintln!("United States: {year} gazetteer");
    let counties = gazetteer(fetcher, year, "counties").await?;
    let [geoid, name] = counties.columns(["GEOID", "NAME"])?;
    let filing = Filing {
        states: STATES.iter().copied().collect(),
        county_names: counties
            .rows
            .iter()
            .map(|r| (r[geoid].as_str(), r[name].as_str()))
            .collect(),
        place_counties: place_counties(fetcher).await?,
    };

    let mut places = Vec::new();
    // New England towns first: a census-designated place of the same name
    // in the same county is the town's centre, not another place.
    let towns = gazetteer(fetcher, year, "cousubs").await?;
    let mut town_names: HashSet<(String, String)> = HashSet::new();
    for row in rows(&towns)? {
        let state = row.geoid.get(..2).unwrap_or_default();
        if NEW_ENGLAND.contains(&state) && row.name.ends_with(" town") {
            town_names.insert(filing.town(&row, &mut places)?);
        }
    }
    let gazetteer_places = gazetteer(fetcher, year, "place").await?;
    for row in rows(&gazetteer_places)? {
        filing.place(&row, &town_names, &mut places);
    }
    eprintln!("United States: {} rows", places.len());
    Ok(places)
}

/// Each place's counties, `STATEFP` + `COUNTYFP`, by `STATEFP` + `PLACEFP`.
async fn place_counties(fetcher: &Fetcher) -> Result<HashMap<String, Vec<String>>> {
    let by_county = fetcher
        .bytes("us-place-by-county.txt", PLACE_BY_COUNTY_URL)
        .await?;
    let by_county = Table::parse(&decode_mixed(&by_county), '|')?;
    let [state, county, place] = by_county.columns(["STATEFP", "COUNTYFP", "PLACEFP"])?;
    let mut place_counties: HashMap<String, Vec<String>> = HashMap::new();
    for row in &by_county.rows {
        place_counties
            .entry(format!("{}{}", row[state], row[place]))
            .or_default()
            .push(format!("{}{}", row[state], row[county]));
    }
    Ok(place_counties)
}

/// What files a gazetteer row under its county and state.
struct Filing<'a> {
    /// State name by FIPS code.
    states: HashMap<&'a str, &'a str>,
    /// County name by `GEOID`.
    county_names: HashMap<&'a str, &'a str>,
    place_counties: HashMap<String, Vec<String>>,
}

impl Filing<'_> {
    /// Files a New England town; its name and county. A `GEOID` too short
    /// for a county, or of an unknown state, is a download that changed.
    fn town(&self, row: &Row, places: &mut Vec<Place>) -> Result<(String, String)> {
        let county = row.geoid.get(..5).with_context(|| {
            format!("the county subdivision GEOID {:?} is too short", row.geoid)
        })?;
        let state_name = self
            .states
            .get(county.get(..2).unwrap_or_default())
            .copied()
            .with_context(|| {
                format!(
                    "the county subdivision GEOID {:?} names no state",
                    row.geoid
                )
            })?;
        let name = plain_name(&row.name);
        let mut base = unfiled(Country::UnitedStates, &name, &row.geoid, Kind::Commune);
        base.coordinates = row.coordinates;
        let county_name = self.county_names.get(county).copied().unwrap_or_default();
        file(places, &base, county_name, state_name, true);
        Ok((name, county.to_string()))
    }

    /// Files an incorporated or census-designated place under each of its
    /// counties, unless it is the centre of a New England town.
    fn place(&self, row: &Row, town_names: &HashSet<(String, String)>, places: &mut Vec<Place>) {
        let Some(state_name) = row.geoid.get(..2).and_then(|state| self.states.get(state)) else {
            return;
        };
        let name = plain_name(&row.name);
        let counties: &[String] = self
            .place_counties
            .get(&row.geoid)
            .map_or(&[], Vec::as_slice);
        let census_designated = row.name.ends_with(" CDP");
        let town_centre = || {
            counties
                .iter()
                .any(|c| town_names.contains(&(name.clone(), c.clone())))
        };
        if census_designated && town_centre() {
            return;
        }
        let kind = if census_designated {
            Kind::Settlement
        } else {
            Kind::Commune
        };
        let mut base = unfiled(Country::UnitedStates, &name, &row.geoid, kind);
        base.coordinates = row.coordinates;
        if counties.is_empty() {
            file(places, &base, "", state_name, true);
        }
        for county in counties {
            let county_name = self
                .county_names
                .get(county.as_str())
                .copied()
                .unwrap_or_default();
            // An independent city is its own county: "Lexington city".
            let county_name = if county_name == row.name {
                ""
            } else {
                county_name
            };
            file(places, &base, county_name, state_name, true);
        }
    }
}

/// The newest year whose gazetteer the Census Bureau has published.
async fn latest_year(fetcher: &Fetcher) -> Result<i64> {
    let (year, _, _) = crate::calendar::today();
    for year in (year - 3..=year).rev() {
        let url = format!("{GAZETTEER_URL}/{year}_Gazetteer/{year}_Gaz_place_national.zip");
        if fetcher
            .optional_bytes(&format!("us-{year}-place.zip"), &url)
            .await?
            .is_some()
        {
            return Ok(year);
        }
    }
    anyhow::bail!("no gazetteer of the last four years was found")
}

async fn gazetteer(fetcher: &Fetcher, year: i64, layer: &str) -> Result<Table> {
    let url = format!("{GAZETTEER_URL}/{year}_Gazetteer/{year}_Gaz_{layer}_national.zip");
    let archive = fetcher
        .bytes(&format!("us-{year}-{layer}.zip"), &url)
        .await?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
    let mut bytes = Vec::new();
    archive.by_index(0)?.read_to_end(&mut bytes)?;
    let text = decode_mixed(&bytes);
    // Editions until 2024 separate columns with tabs, later ones with bars.
    let delimiter = if text.lines().next().unwrap_or_default().contains('|') {
        '|'
    } else {
        '\t'
    };
    Table::parse(&text, delimiter).with_context(|| format!("cannot read the {layer} gazetteer"))
}

struct Row {
    geoid: String,
    name: String,
    coordinates: Option<Coordinates>,
}

fn rows(table: &Table) -> Result<Vec<Row>> {
    let [geoid, name, latitude] = table.columns(["GEOID", "NAME", "INTPTLAT"])?;
    // The last header carries trailing spaces in some editions.
    let longitude = table
        .column_where(|h| h.trim() == "INTPTLONG")
        .context("no INTPTLONG column")?;
    Ok(table
        .rows
        .iter()
        .map(|r| Row {
            geoid: r[geoid].trim().to_string(),
            name: r[name].trim().to_string(),
            coordinates: Coordinates::parse(&r[latitude], &r[longitude]),
        })
        .collect())
}

fn plain_name(name: &str) -> String {
    DESCRIPTIONS
        .iter()
        .find_map(|d| name.strip_suffix(d))
        .unwrap_or(name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A town row whose GEOID cannot name a county, or names an unknown
    /// state, fails the run with a reason instead of panicking.
    #[test]
    fn a_malformed_town_geoid_is_an_error() {
        let filing = Filing {
            states: HashMap::from([("25", "Example State")]),
            county_names: HashMap::new(),
            place_counties: HashMap::new(),
        };
        let row = |geoid: &str| Row {
            geoid: geoid.to_owned(),
            name: "Exampleton town".to_owned(),
            coordinates: None,
        };
        let mut places = Vec::new();
        assert!(filing.town(&row("25"), &mut places).is_err());
        assert!(filing.town(&row("99001"), &mut places).is_err());
        assert!(filing.town(&row("2500\u{e9}"), &mut places).is_err());
        assert!(places.is_empty());
        let (name, county) = filing.town(&row("2500101234"), &mut places).unwrap();
        assert_eq!((name.as_str(), county.as_str()), ("Exampleton", "25001"));
    }

    /// A place row with a GEOID too short for a state is left out.
    #[test]
    fn a_short_place_geoid_is_left_out() {
        let filing = Filing {
            states: HashMap::from([("25", "Example State")]),
            county_names: HashMap::new(),
            place_counties: HashMap::new(),
        };
        let row = Row {
            geoid: "2".to_owned(),
            name: "Exampleton city".to_owned(),
            coordinates: None,
        };
        let mut places = Vec::new();
        filing.place(&row, &HashSet::new(), &mut places);
        assert!(places.is_empty());
    }

    #[test]
    fn legal_descriptions_are_not_part_of_the_name() {
        assert_eq!(plain_name("Town A city"), "Town A");
        assert_eq!(plain_name("Town A CDP"), "Town A");
        assert_eq!(plain_name("Town A city and borough"), "Town A");
        assert_eq!(
            plain_name("Town A-B consolidated government (balance)"),
            "Town A-B"
        );
        assert_eq!(plain_name("Town A"), "Town A");
    }
}
