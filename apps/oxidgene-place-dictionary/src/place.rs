//! One dictionary row, and how it is written.
//!
//! The first five columns are those of Geneanet's `dico_place_*.csv`
//! (place, code, subdivision, region, country), so a reader of that format
//! reads these files unchanged. The columns after them carry what that
//! format has no room for; see `docs/place-dictionary.md`.

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

use crate::table::quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Country {
    France,
    UnitedKingdom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Nation {
    England,
    Scotland,
    Wales,
    NorthernIreland,
}

impl Nation {
    /// The nation named by the ONS in English.
    pub fn from_english(name: &str) -> Option<Self> {
        match name {
            "England" => Some(Self::England),
            "Scotland" => Some(Self::Scotland),
            "Wales" => Some(Self::Wales),
            "Northern Ireland" => Some(Self::NorthernIreland),
            _ => None,
        }
    }
}

/// The fourth column: a French region, or a British nation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Region {
    Named(String),
    Nation(Nation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// A French commune that exists today under this name and code.
    Commune,
    /// One of the arrondissements of Paris, Lyon or Marseille.
    MunicipalArrondissement,
    /// A commune that still exists, under another name or code.
    FormerName,
    /// A commune merged into another or abolished.
    FormerCommune,
    /// A British town, village, hamlet or locality.
    Settlement,
    /// A British civil parish or Welsh community.
    Parish,
}

impl Kind {
    fn token(self) -> &'static str {
        match self {
            Self::Commune => "commune",
            Self::MunicipalArrondissement => "municipal_arrondissement",
            Self::FormerName => "former_name",
            Self::FormerCommune => "former_commune",
            Self::Settlement => "settlement",
            Self::Parish => "parish",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub code: String,
    pub subdivision: String,
    pub region: Region,
    pub country: Country,
    pub kind: Kind,
    /// First day the name was in use, when known and later than the source's
    /// horizon.
    pub valid_from: Option<String>,
    /// First day the name was no longer in use.
    pub valid_until: Option<String>,
    /// Code of the commune that holds the territory today.
    pub successor: Option<String>,
    pub coordinates: Option<Coordinates>,
    /// Filed under today's subdivision and region, rather than a former one.
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

impl Coordinates {
    /// Reads Wikidata's `Point(longitude latitude)` literal.
    pub fn from_wkt(text: &str) -> Option<Self> {
        let inner = text.strip_prefix("Point(")?.strip_suffix(')')?;
        let (longitude, latitude) = inner.split_once(' ')?;
        Some(Self {
            latitude: latitude.parse().ok()?,
            longitude: longitude.parse().ok()?,
        })
    }

    /// Squared distance on a plane stretched by the latitude, which ranks
    /// neighbours correctly at the scale of a département.
    pub fn distance2(self, other: Self) -> f64 {
        let dx = (self.longitude - other.longitude) * self.latitude.to_radians().cos();
        let dy = self.latitude - other.latitude;
        dx * dx + dy * dy
    }
}

/// The dictionary is written in French, like Geneanet's `dico_place_fr.csv`.
/// The application translates the few British names for its English
/// interface when it loads the file.
fn country_name(country: Country) -> &'static str {
    match country {
        Country::France => "France",
        Country::UnitedKingdom => "Royaume-Uni",
    }
}

fn nation_name(nation: Nation) -> &'static str {
    match nation {
        Nation::England => "Angleterre",
        Nation::Scotland => "Écosse",
        Nation::Wales => "Pays de Galles",
        Nation::NorthernIreland => "Irlande du Nord",
    }
}

/// Sorts the places, drops duplicate rows and renders the dictionary as CSV.
/// Returns the text and its number of rows.
///
/// Two rows are duplicates when they would read the same in the first five
/// columns and have the same kind: the ONS lists a place once per boundary it
/// straddles, which differs only in its coordinates.
pub fn render(places: &mut [Place]) -> (String, usize) {
    places.sort_by_cached_key(|p| {
        (
            p.country,
            p.region.clone(),
            p.subdivision.clone(),
            p.code.clone(),
            fold(&p.name),
            p.name.clone(),
            p.kind,
            // Of two rows that read the same, the current one is kept.
            !p.current,
        )
    });
    let mut seen = HashSet::new();
    let mut out = String::new();
    let mut rows = 0;
    for place in places.iter() {
        let region = match &place.region {
            Region::Named(name) => name.as_str(),
            Region::Nation(nation) => nation_name(*nation),
        };
        if !seen.insert((
            &place.name,
            &place.code,
            &place.subdivision,
            region,
            place.country,
            place.kind,
        )) {
            continue;
        }
        let (latitude, longitude) = place
            .coordinates
            .map_or((String::new(), String::new()), |c| {
                (format!("{:.4}", c.latitude), format!("{:.4}", c.longitude))
            });
        let fields = [
            place.name.as_str(),
            &place.code,
            &place.subdivision,
            region,
            country_name(place.country),
            place.kind.token(),
            place.valid_from.as_deref().unwrap_or_default(),
            place.valid_until.as_deref().unwrap_or_default(),
            place.successor.as_deref().unwrap_or_default(),
            &latitude,
            &longitude,
            if place.current { "1" } else { "" },
        ];
        let line = fields
            .iter()
            .map(|f| quote(f))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
        rows += 1;
    }
    (out, rows)
}

/// Lowercase, without accents or punctuation: what two sources spelling the
/// same name differently still agree on.
pub fn fold(name: &str) -> String {
    name.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(name: &str, kind: Kind, latitude: f64) -> Place {
        Place {
            name: name.to_string(),
            code: String::new(),
            subdivision: "Shire A".to_string(),
            region: Region::Nation(Nation::Wales),
            country: Country::UnitedKingdom,
            kind,
            valid_from: None,
            valid_until: None,
            successor: None,
            coordinates: Some(Coordinates {
                latitude,
                longitude: -3.0,
            }),
            current: false,
        }
    }

    #[test]
    fn folding_ignores_accents_case_and_punctuation() {
        assert_eq!(fold("Saint-Étienne-d'Été"), "saint etienne d ete");
    }

    #[test]
    fn reads_a_wikidata_point() {
        let c = Coordinates::from_wkt("Point(-3.25 48.5)").unwrap();
        assert_eq!((c.latitude, c.longitude), (48.5, -3.25));
        assert!(Coordinates::from_wkt("48.5 -3.25").is_none());
    }

    #[test]
    fn renders_each_row_once() {
        let mut places = vec![
            place("Village B", Kind::Settlement, 52.0),
            place("Village A", Kind::Settlement, 52.0),
            // The same place on the other side of a boundary.
            place("Village A", Kind::Settlement, 52.1),
            place("Village A", Kind::Parish, 52.0),
        ];

        let (text, rows) = render(&mut places);
        assert_eq!(rows, 3);

        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            "\"Village A\",\"\",\"Shire A\",\"Pays de Galles\",\"Royaume-Uni\",\"settlement\",\"\",\"\",\"\",\"52.0000\",\"-3.0000\",\"\""
        );
        assert!(text.lines().nth(1).unwrap().contains("\"parish\""));
        assert!(text.lines().nth(2).unwrap().starts_with("\"Village B\""));
    }
}
