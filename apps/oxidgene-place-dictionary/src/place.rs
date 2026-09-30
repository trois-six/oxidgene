//! One dictionary row, and how it is written.
//!
//! The first five columns are those of Geneanet's `dico_place_*.csv`
//! (place, code, subdivision, region, country), so a reader of that format
//! reads these files unchanged. The columns after them carry what that
//! format has no room for; see `docs/place-dictionary.md`.

use oxidgene_core::search::fold_words;
use std::collections::HashSet;

use crate::table::quote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Country {
    France,
    UnitedKingdom,
    Germany,
    Italy,
    Spain,
    Switzerland,
    Poland,
    UnitedStates,
    Portugal,
    Belgium,
    Luxembourg,
    Netherlands,
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

impl Place {
    /// The region column, when it is a named region.
    pub fn region_name(&self) -> &str {
        match &self.region {
            Region::Named(name) => name,
            Region::Nation(_) => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

impl Coordinates {
    /// Reads decimal degrees, surrounding spaces aside.
    pub fn parse(latitude: &str, longitude: &str) -> Option<Self> {
        Some(Self {
            latitude: latitude.trim().parse().ok()?,
            longitude: longitude.trim().parse().ok()?,
        })
    }

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
/// Subdivisions and regions keep their official local names; only the
/// countries and the British nations are French, and the application
/// translates those for its English interface when it loads the file.
fn country_name(country: Country) -> &'static str {
    match country {
        Country::France => "France",
        Country::UnitedKingdom => "Royaume-Uni",
        Country::Germany => "Allemagne",
        Country::Italy => "Italie",
        Country::Spain => "Espagne",
        Country::Switzerland => "Suisse",
        Country::Poland => "Pologne",
        // Geneanet's spelling, so a place imported from it matches.
        Country::UnitedStates => "Etats-Unis d'Amérique",
        Country::Portugal => "Portugal",
        Country::Belgium => "Belgique",
        Country::Luxembourg => "Luxembourg",
        Country::Netherlands => "Pays-Bas",
    }
}

/// Adds `base` filed under a subdivision and region, unless it already is.
/// `current` marks today's filing.
pub fn file(places: &mut Vec<Place>, base: &Place, subdivision: &str, region: &str, current: bool) {
    let region = Region::Named(region.to_string());
    // The filings of one place are pushed one after the other, so only the
    // tail of the list can already hold this one.
    if !places
        .iter()
        .rev()
        .take_while(|p| p.name == base.name && p.code == base.code && p.kind == base.kind)
        .any(|p| p.subdivision == subdivision && p.region == region)
    {
        places.push(Place {
            subdivision: subdivision.to_string(),
            region,
            current,
            ..base.clone()
        });
    }
}

/// A place of `country` with no filing yet, to hand to [`file`].
pub fn unfiled(country: Country, name: &str, code: &str, kind: Kind) -> Place {
    Place {
        name: name.to_string(),
        code: code.to_string(),
        subdivision: String::new(),
        region: Region::Named(String::new()),
        country,
        kind,
        valid_from: None,
        valid_until: None,
        successor: None,
        coordinates: None,
        current: false,
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
/// Returns the text, its number of rows and the number of rows dropped.
///
/// Two rows are duplicates when they would read the same once folded (case,
/// accents and punctuation aside) in the first five columns, with the same
/// kind, dates and successor: the ONS lists a place once per boundary it
/// straddles, which differs only in its coordinates, and spells some names
/// two ways ("St George", "St. George"). Rows that differ in their dates are
/// two eras of a name and are both kept.
pub fn render(places: &mut [Place]) -> (String, usize, usize) {
    places.sort_by_cached_key(|p| {
        (
            p.country,
            p.region.clone(),
            p.subdivision.clone(),
            p.code.clone(),
            fold_words(&p.name),
            p.kind,
            p.valid_from.clone(),
            p.valid_until.clone(),
            p.successor.clone(),
            // Of two rows that read the same, the current one is kept, then
            // the first spelling.
            !p.current,
            p.name.clone(),
            // The rest only makes the order total, so that two runs over the
            // same sources write the same file whatever order the sources
            // listed their rows in.
            p.coordinates
                .map(|c| (c.latitude.to_bits(), c.longitude.to_bits())),
        )
    });
    let mut seen = HashSet::new();
    let mut out = String::new();
    let mut rows = 0;
    let mut dropped = 0;
    for place in places.iter() {
        let region = match &place.region {
            Region::Named(name) => name.as_str(),
            Region::Nation(nation) => nation_name(*nation),
        };
        if !seen.insert((
            fold_words(&place.name),
            &place.code,
            &place.subdivision,
            region,
            place.country,
            place.kind,
            &place.valid_from,
            &place.valid_until,
            &place.successor,
        )) {
            dropped += 1;
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
    (out, rows, dropped)
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
        assert_eq!(fold_words("Saint-Étienne-d'Été"), "saint etienne d ete");
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

        let (text, rows, dropped) = render(&mut places);
        assert_eq!((rows, dropped), (3, 1));

        let first = text.lines().next().unwrap();
        assert_eq!(
            first,
            "\"Village A\",\"\",\"Shire A\",\"Pays de Galles\",\"Royaume-Uni\",\"settlement\",\"\",\"\",\"\",\"52.0000\",\"-3.0000\",\"\""
        );
        assert!(text.lines().nth(1).unwrap().contains("\"parish\""));
        assert!(text.lines().nth(2).unwrap().starts_with("\"Village B\""));
    }

    #[test]
    fn a_name_spelled_two_ways_is_one_row() {
        let mut places = vec![
            place("St. Village", Kind::Settlement, 52.0),
            place("St Village", Kind::Settlement, 52.0),
            place("Village-on-Sea", Kind::Settlement, 52.0),
            place("Village on sea", Kind::Settlement, 52.0),
        ];
        let (text, rows, dropped) = render(&mut places);
        assert_eq!((rows, dropped), (2, 2));
        assert!(text.contains("\"St Village\""));
        assert!(text.contains("\"Village on sea\""));
    }

    #[test]
    fn two_eras_of_a_name_are_two_rows() {
        let era = |until: &str| Place {
            valid_until: Some(until.to_string()),
            ..place("Village A", Kind::Settlement, 52.0)
        };
        let mut places = vec![era("1900-01-01"), era("1950-01-01"), era("1950-01-01")];
        let (_, rows, dropped) = render(&mut places);
        assert_eq!((rows, dropped), (2, 1));
    }
}
