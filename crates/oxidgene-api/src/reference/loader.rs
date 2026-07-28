//! Decompresses and indexes the embedded reference-data JSON (see
//! `build.rs`), and resolves free-text GEDCOM values (occupation labels,
//! given names) to the matching content entry.

use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;
use serde::Deserialize;

/// Reference-content language. Deliberately independent of `oxidgene-ui`'s
/// `Language` type — this crate has no UI dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceLang {
    Fr,
    En,
}

impl ReferenceLang {
    /// Parse a BCP-47-ish path segment (`"fr"` / `"en"`).
    pub fn from_code(s: &str) -> Option<Self> {
        match s {
            "fr" => Some(Self::Fr),
            "en" => Some(Self::En),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct OccupationEntry {
    pub label: String,
    pub summary: String,
    pub text: String,
    #[serde(default, skip_serializing)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct GivenNameEntry {
    pub label: String,
    pub origin: String,
    pub meaning: String,
    pub text: String,
    #[serde(default)]
    pub feast_day: Option<String>,
    #[serde(default, skip_serializing)]
    pub aliases: Vec<String>,
}

/// Normalizes a raw GEDCOM value for lookup: lowercase, accents stripped,
/// punctuation collapsed to single spaces. GEDCOM occupation/given-name
/// values are free text (accents, gendered variants, old spellings), so
/// entries also get indexed under each of their declared `aliases`.
pub fn normalize_key(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter_map(|c| match c.to_lowercase().next().unwrap_or(c) {
            'à' | 'â' | 'ä' | 'á' | 'ã' => Some('a'),
            'ç' => Some('c'),
            'é' | 'è' | 'ê' | 'ë' => Some('e'),
            'î' | 'ï' => Some('i'),
            'ô' | 'ö' | 'õ' => Some('o'),
            'ù' | 'û' | 'ü' => Some('u'),
            'ÿ' => Some('y'),
            '-' | '\'' | '_' | '/' => Some(' '),
            lower if lower.is_alphanumeric() || lower == ' ' => Some(lower),
            _ => None,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

macro_rules! embed_gz {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".gz"))
    };
}

static OCCUPATIONS_FR: &[u8] = embed_gz!("occupations.fr.json");
static OCCUPATIONS_EN: &[u8] = embed_gz!("occupations.en.json");
static GIVEN_NAMES_FR: &[u8] = embed_gz!("given_names.fr.json");
static GIVEN_NAMES_EN: &[u8] = embed_gz!("given_names.en.json");

fn decompress_json<T: serde::de::DeserializeOwned>(gz_bytes: &'static [u8]) -> T {
    let mut json = String::new();
    GzDecoder::new(gz_bytes)
        .read_to_string(&mut json)
        .expect("embedded reference data must be valid gzip");
    serde_json::from_str(&json).expect("embedded reference data must be valid JSON")
}

/// Builds the lookup table for one language: every entry indexed under its
/// own (normalized) key plus each of its (normalized) aliases.
fn build_table<T: Clone + serde::de::DeserializeOwned>(
    gz_bytes: &'static [u8],
    aliases_of: impl Fn(&T) -> &[String],
) -> HashMap<String, T> {
    let raw: HashMap<String, T> = decompress_json(gz_bytes);
    let mut table = HashMap::with_capacity(raw.len() * 2);
    for (key, entry) in raw {
        for alias in aliases_of(&entry) {
            table.insert(normalize_key(alias), entry.clone());
        }
        table.insert(normalize_key(&key), entry);
    }
    table
}

static OCCUPATIONS_FR_TABLE: OnceLock<HashMap<String, OccupationEntry>> = OnceLock::new();
static OCCUPATIONS_EN_TABLE: OnceLock<HashMap<String, OccupationEntry>> = OnceLock::new();
static GIVEN_NAMES_FR_TABLE: OnceLock<HashMap<String, GivenNameEntry>> = OnceLock::new();
static GIVEN_NAMES_EN_TABLE: OnceLock<HashMap<String, GivenNameEntry>> = OnceLock::new();

/// Looks up an occupation fiche by raw GEDCOM label (any case/accent/alias
/// variant listed in the data file). Free-text occupation fields (e.g. "CTO
/// chez Entreprise Exemple") rarely match a full entry verbatim, so on exact
/// miss this falls back to the longest dictionary key/alias that occurs as a
/// whole-word run inside the term — long enough to still tell "Barbier
/// Perruquier" apart from plain "Barbier" when both are present.
pub fn lookup_occupation(lang: ReferenceLang, term: &str) -> Option<OccupationEntry> {
    let table = match lang {
        ReferenceLang::Fr => OCCUPATIONS_FR_TABLE
            .get_or_init(|| build_table(OCCUPATIONS_FR, |e: &OccupationEntry| &e.aliases)),
        ReferenceLang::En => OCCUPATIONS_EN_TABLE
            .get_or_init(|| build_table(OCCUPATIONS_EN, |e: &OccupationEntry| &e.aliases)),
    };
    let normalized = normalize_key(term);
    if let Some(entry) = table.get(&normalized) {
        return Some(entry.clone());
    }
    longest_word_run_match(table, &normalized).cloned()
}

/// Returns `true` when `needle_words` occurs as a contiguous run inside
/// `haystack_words`, matching on whole words only (so "cto" never matches
/// inside e.g. "directeur").
fn contains_word_run(haystack_words: &[&str], needle_words: &[&str]) -> bool {
    !needle_words.is_empty()
        && needle_words.len() <= haystack_words.len()
        && haystack_words
            .windows(needle_words.len())
            .any(|w| w == needle_words)
}

/// Scans every (already-normalized) key/alias in `table` and returns the
/// entry for the longest one occurring as a whole-word run inside
/// `haystack`. Longest wins so a more specific multi-word entry ("barbier
/// perruquier") is preferred over a shorter one it contains ("barbier").
fn longest_word_run_match<'a, T>(table: &'a HashMap<String, T>, haystack: &str) -> Option<&'a T> {
    let haystack_words: Vec<&str> = haystack.split(' ').filter(|w| !w.is_empty()).collect();
    let mut best: Option<(&str, &T)> = None;
    for (key, entry) in table {
        if key.is_empty() {
            continue;
        }
        let key_words: Vec<&str> = key.split(' ').collect();
        if contains_word_run(&haystack_words, &key_words)
            && best
                .as_ref()
                .is_none_or(|(best_key, _)| key.len() > best_key.len())
        {
            best = Some((key, entry));
        }
    }
    best.map(|(_, entry)| entry)
}

/// Looks up a given-name fiche. Tries the full (possibly compound) term
/// first, then falls back to its first token — so "Marie-Claire" still
/// resolves via "Marie" if the compound itself has no dedicated entry.
pub fn lookup_given_name(lang: ReferenceLang, term: &str) -> Option<GivenNameEntry> {
    let table = match lang {
        ReferenceLang::Fr => GIVEN_NAMES_FR_TABLE
            .get_or_init(|| build_table(GIVEN_NAMES_FR, |e: &GivenNameEntry| &e.aliases)),
        ReferenceLang::En => GIVEN_NAMES_EN_TABLE
            .get_or_init(|| build_table(GIVEN_NAMES_EN, |e: &GivenNameEntry| &e.aliases)),
    };
    let full = normalize_key(term);
    if let Some(entry) = table.get(&full) {
        return Some(entry.clone());
    }
    let first_token = full.split(' ').next()?;
    table.get(first_token).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accents_and_punctuation() {
        assert_eq!(normalize_key("Laboureur/euse"), "laboureur euse");
        assert_eq!(normalize_key("  Forgeron  "), "forgeron");
        assert_eq!(normalize_key("Méunier"), "meunier");
    }

    #[test]
    fn looks_up_occupation_by_canonical_and_alias() {
        let entry = lookup_occupation(ReferenceLang::Fr, "Laboureur").expect("canonical match");
        assert_eq!(entry.label, "Laboureur");

        let alias = lookup_occupation(ReferenceLang::Fr, "laboureur/euse").expect("alias match");
        assert_eq!(alias.label, "Laboureur");

        assert!(lookup_occupation(ReferenceLang::Fr, "astronaute").is_none());
    }

    #[test]
    fn looks_up_occupation_in_english() {
        let entry = lookup_occupation(ReferenceLang::En, "laboureur").expect("english entry");
        assert!(entry.label.contains("ploughman"));
    }

    #[test]
    fn falls_back_to_longest_word_run_within_free_text() {
        let entry = lookup_occupation(ReferenceLang::Fr, "CTO chez Entreprise Exemple")
            .expect("substring match");
        assert_eq!(entry.label, "CTO");
    }

    #[test]
    fn prefers_longer_word_run_over_shorter_one_it_contains() {
        let entry =
            lookup_occupation(ReferenceLang::Fr, "Barbier Perruquier").expect("exact match");
        assert_eq!(entry.label, "Barbier Perruquier");

        // No entry has "Barbier Coiffeur" verbatim, so this only resolves via
        // the word-run fallback — which must prefer "Barbier Perruquier"
        // over the shorter "Barbier" it also contains, whichever iteration
        // order the underlying HashMap happens to produce.
        let fallback = lookup_occupation(ReferenceLang::Fr, "Ancien Barbier Perruquier Retraité")
            .expect("word-run fallback match");
        assert_eq!(fallback.label, "Barbier Perruquier");
    }

    #[test]
    fn does_not_match_a_word_partially() {
        // "cto" must not match inside a longer word that merely contains
        // those letters.
        assert!(lookup_occupation(ReferenceLang::Fr, "directoire").is_none());
    }

    #[test]
    fn looks_up_given_name_with_compound_fallback() {
        let entry = lookup_given_name(ReferenceLang::Fr, "Marie-Claire").expect("fallback match");
        assert_eq!(entry.label, "Marie");

        let direct = lookup_given_name(ReferenceLang::En, "JEAN").expect("case-insensitive match");
        assert_eq!(direct.label, "Jean");

        assert!(lookup_given_name(ReferenceLang::Fr, "Zorglub").is_none());
    }
}
