//! The basemap of the Statistics page's heat map: every country's outline
//! from Natural Earth (1:50m, public domain), coarsened to a tenth of a
//! degree, which is all a heat map of regions needs.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::fetch::Fetcher;

const NATURAL_EARTH_URL: &str = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_50m_admin_0_countries.geojson";
/// Coordinates are stored as integers of this many units per degree.
pub const UNITS_PER_DEGREE: f64 = 10.0;

/// The basemap as JSON: `[{"iso", "name", "rings": [[lon, lat, lon, lat…]]}]`,
/// coordinates in tenths of a degree, outer rings only.
pub async fn basemap(fetcher: &Fetcher) -> Result<String> {
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
        countries.push(json!({
            "iso": properties["ISO_A2_EH"].as_str().unwrap_or_default(),
            "name": properties["NAME"].as_str().unwrap_or_default(),
            "rings": rings,
        }));
    }
    eprintln!("basemap: {} countries", countries.len());
    Ok(serde_json::to_string(&countries)?)
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
    use super::coarse_ring;
    use serde_json::json;

    #[test]
    fn rings_snap_to_a_tenth_of_a_degree_without_repeats() {
        let ring = json!([[2.01, 48.02], [2.04, 48.03], [2.5, 48.9], [2.51, 48.94]]);
        assert_eq!(coarse_ring(&ring), vec![20, 480, 25, 489]);
    }
}
