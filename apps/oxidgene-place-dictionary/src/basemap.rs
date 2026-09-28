//! The basemap of the Statistics page's heat map: every country's outline
//! from Natural Earth (1:50m, public domain), coarsened to a tenth of a
//! degree, which is all a heat map of regions needs, and its populated
//! places (Natural Earth's 1:10m layer), named on the map from the zoom
//! Natural Earth gives each, so a reader knows where they are.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::fetch::Fetcher;

const NATURAL_EARTH_URL: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson";
const POPULATED_PLACES_URL: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_10m_populated_places.geojson";
/// Coordinates are stored as integers of this many units per degree.
pub const UNITS_PER_DEGREE: f64 = 10.0;
/// The interface languages, whose names of a city are kept when they differ
/// from its Natural Earth name.
const LANGUAGES: [&str; 8] = ["en", "fr", "de", "es", "it", "nl", "pl", "pt"];

/// The basemap as JSON: `[{"iso", "name", "rings": [[lon, lat, lon, lat…]],
/// "cities": [{"name", "names": [{"lang", "name"}], "lon", "lat", "zoom",
/// "population"}]}]`, coordinates in tenths of a degree, outer rings only,
/// cities by the zoom they appear from, then the most populated first.
pub async fn basemap(fetcher: &Fetcher) -> Result<String> {
    let mut cities = cities(fetcher).await?;
    let bytes = fetcher
        .bytes("ne_50m_admin_0_countries.geojson", NATURAL_EARTH_URL)
        .await?;
    let geojson: Value = serde_json::from_slice(&bytes).context("Natural Earth is not JSON")?;
    let features = geojson["features"]
        .as_array()
        .context("Natural Earth has no features")?;
    let mut countries = Vec::new();
    for feature in features {
        let properties = &feature["properties"];
        let geometry = &feature["geometry"];
        let polygons: Vec<&Value> = match geometry["type"].as_str() {
            Some("Polygon") => vec![&geometry["coordinates"]],
            Some("MultiPolygon") => geometry["coordinates"]
                .as_array()
                .map(|p| p.iter().collect())
                .unwrap_or_default(),
            _ => continue,
        };
        let rings: Vec<Vec<i64>> = polygons
            .iter()
            .filter_map(|polygon| polygon.get(0))
            .map(coarse_ring)
            .filter(|ring| ring.len() >= 6)
            .collect();
        if rings.is_empty() {
            continue;
        }
        let code = properties["ADM0_A3"].as_str().unwrap_or_default();
        countries.push(json!({
            "iso": properties["ISO_A2_EH"].as_str().unwrap_or_default(),
            "name": properties["NAME"].as_str().unwrap_or_default(),
            "rings": rings,
            "cities": cities.remove(code).unwrap_or_default(),
        }));
    }
    eprintln!("basemap: {} countries", countries.len());
    Ok(serde_json::to_string(&countries)?)
}

/// Each country's populated places, by the country's Natural Earth
/// `ADM0_A3` code, which both layers carry.
async fn cities(fetcher: &Fetcher) -> Result<HashMap<String, Vec<Value>>> {
    let bytes = fetcher
        .bytes("ne_10m_populated_places.geojson", POPULATED_PLACES_URL)
        .await?;
    let geojson: Value =
        serde_json::from_slice(&bytes).context("Natural Earth's populated places are not JSON")?;
    let features = geojson["features"]
        .as_array()
        .context("Natural Earth's populated places have no features")?;
    let mut by_country: HashMap<String, Vec<&Value>> = HashMap::new();
    for feature in features {
        let properties = &feature["properties"];
        if let Some(code) = properties["ADM0_A3"].as_str() {
            by_country
                .entry(code.to_string())
                .or_default()
                .push(properties);
        }
    }
    Ok(by_country
        .into_iter()
        .map(|(code, places)| {
            let mut kept: Vec<Value> = places.into_iter().filter_map(city).collect();
            let key = |c: &Value| {
                (
                    c["zoom"].as_i64().unwrap_or_default(),
                    -c["population"].as_i64().unwrap_or_default(),
                )
            };
            kept.sort_by_key(key);
            (code, kept)
        })
        .collect())
}

/// A populated place as the basemap keeps it: its name, its names in the
/// interface languages where they differ, its position, the zoom from
/// which Natural Earth labels it (in tenths of a web map zoom level) and
/// its population in thousands.
fn city(properties: &Value) -> Option<Value> {
    let name = properties["NAME"].as_str().filter(|n| !n.is_empty())?;
    let (lon, lat) = (
        properties["LONGITUDE"].as_f64()?,
        properties["LATITUDE"].as_f64()?,
    );
    let names: Vec<Value> = LANGUAGES
        .iter()
        .filter_map(|lang| {
            let local = properties[format!("NAME_{}", lang.to_uppercase())].as_str()?;
            (!local.is_empty() && local != name).then(|| json!({"lang": lang, "name": local}))
        })
        .collect();
    Some(json!({
        "name": name,
        "names": names,
        "lon": (lon * UNITS_PER_DEGREE).round() as i64,
        "lat": (lat * UNITS_PER_DEGREE).round() as i64,
        "zoom": (properties["MIN_ZOOM"].as_f64()? * 10.0).round() as i64,
        "population": (properties["POP_MAX"].as_f64().unwrap_or_default() / 1000.0).round() as i64,
    }))
}

/// A ring's points on the coarse grid, without the repeats it creates.
fn coarse_ring(ring: &Value) -> Vec<i64> {
    let mut out = Vec::new();
    let mut last = None;
    for point in ring.as_array().into_iter().flatten() {
        let (Some(lon), Some(lat)) = (point[0].as_f64(), point[1].as_f64()) else {
            continue;
        };
        let snapped = (
            (lon * UNITS_PER_DEGREE).round() as i64,
            (lat * UNITS_PER_DEGREE).round() as i64,
        );
        if last != Some(snapped) {
            out.extend([snapped.0, snapped.1]);
            last = Some(snapped);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{city, coarse_ring};
    use serde_json::json;

    #[test]
    fn a_city_keeps_only_the_names_that_differ() {
        let properties = json!({
            "NAME": "City A", "NAME_EN": "City A", "NAME_FR": "Ville A",
            "NAME_DE": "", "LONGITUDE": 2.349, "LATITUDE": 48.864,
            "MIN_ZOOM": 4.7, "POP_MAX": 1_423_000,
        });
        assert_eq!(
            city(&properties),
            Some(json!({
                "name": "City A", "names": [{"lang": "fr", "name": "Ville A"}],
                "lon": 23, "lat": 489, "zoom": 47, "population": 1423,
            }))
        );
        assert_eq!(city(&json!({"NAME": "City B"})), None);
    }

    #[test]
    fn rings_snap_to_a_tenth_of_a_degree_without_repeats() {
        let ring = json!([[2.01, 48.02], [2.04, 48.03], [2.5, 48.9], [2.51, 48.94]]);
        assert_eq!(coarse_ring(&ring), vec![20, 480, 25, 489]);
    }
}
