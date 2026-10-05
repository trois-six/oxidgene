//! The words citations are written with, one embedded JSON document per
//! language under `assets/citations/` (Archive Portals §5.1).
//!
//! The recognizer of [`crate::recognize`] knows no language: every word it
//! reads — the kinds of archive, the acts and series, the views, numbers,
//! folios and call numbers, the months and the words joining a period — comes
//! from these documents, and a country's conventions (the year its civil
//! registration began, the Republican calendar) from the documents naming
//! that country. Adding a language is a data change.

use std::collections::BTreeMap;
use std::fmt;

use oxidgene_core::search::fold_words;
use serde::Deserialize;

use crate::catalog::Level;
use crate::citation::{Act, ActKind, Series};

/// The embedded documents.
pub(crate) const EMBEDDED: &[&str] = include!(concat!(env!("OUT_DIR"), "/vocabularies.rs"));

/// The registers a word names without naming the act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// Parish registers: baptisms, marriages, burials.
    Parish,
    /// Civil status: births, marriages, deaths.
    Civil,
    /// A register, of either kind.
    Either,
}

/// What a phrase of a vocabulary means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Meaning {
    /// A kind of archive service, of one level or of any (`None`).
    Archive(Option<Level>),
    Act(ActKind),
    Family(Family),
    /// A table or a series.
    Document(Act),
    View,
    ViewCount,
    Number,
    /// A word whose number is not the act's: `ménage n° 56`.
    OtherNumber,
    Folio,
    Page,
    Recto,
    Verso,
    CallNumber,
    Parish,
    /// A preposition introducing a locality: `de`, `à`.
    Place,
    Bureau,
    Range,
    RepublicanYear,
    /// A month, zero-based.
    Month(u8),
    Ignored,
}

/// The sides a view number may carry as a suffix.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sides {
    #[serde(default)]
    right: Vec<String>,
    #[serde(default)]
    left: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registers {
    #[serde(default)]
    any: Vec<String>,
    #[serde(default)]
    parish: Vec<String>,
    #[serde(default)]
    civil: Vec<String>,
}

/// One vocabulary document, as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    language: String,
    #[serde(default)]
    countries: Vec<String>,
    #[serde(default)]
    civil_registration_from: Option<u16>,
    /// Phrases naming a kind of archive, by level, or `any`.
    #[serde(default)]
    archives: BTreeMap<String, Vec<String>>,
    /// Phrases naming one act kind, by its letter.
    #[serde(default)]
    acts: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    registers: Registers,
    /// Phrases naming a table, by its code.
    #[serde(default)]
    tables: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    series: BTreeMap<Series, Vec<String>>,
    #[serde(default)]
    views: Vec<String>,
    #[serde(default)]
    view_counts: Vec<String>,
    #[serde(default)]
    sides: Sides,
    #[serde(default)]
    numbers: Vec<String>,
    #[serde(default)]
    other_numbers: Vec<String>,
    #[serde(default)]
    folios: Vec<String>,
    #[serde(default)]
    pages: Vec<String>,
    #[serde(default)]
    recto: Vec<String>,
    #[serde(default)]
    verso: Vec<String>,
    #[serde(default)]
    call_numbers: Vec<String>,
    #[serde(default)]
    parishes: Vec<String>,
    #[serde(default)]
    places: Vec<String>,
    #[serde(default)]
    particles: Vec<String>,
    #[serde(default)]
    bureaus: Vec<String>,
    /// Twelve lists, January first, or none.
    #[serde(default)]
    months: Vec<Vec<String>>,
    #[serde(default)]
    ranges: Vec<String>,
    #[serde(default)]
    republican_years: Vec<String>,
    #[serde(default)]
    ignored: Vec<String>,
}

/// Phrases with their meaning, and the key under which the first is the
/// one written back (§6.5 of Archive Portals), if any.
type Entry<'d> = (&'d [String], Meaning, Option<String>);

impl Document {
    /// The phrases whose meaning a code or a level gives: kinds of archive,
    /// acts, tables and series.
    fn coded(&self) -> Result<Vec<Entry<'_>>, String> {
        let mut entries = Vec::new();
        for (level, texts) in &self.archives {
            let level = match level.as_str() {
                "any" => None,
                other => Some(
                    serde_json::from_value::<Level>(serde_json::Value::String(other.to_owned()))
                        .map_err(|_| format!("`{other}` is no archive level"))?,
                ),
            };
            entries.push((texts.as_slice(), Meaning::Archive(level), None));
        }
        for (letter, texts) in &self.acts {
            let kind = letter
                .chars()
                .next()
                .filter(|_| letter.chars().count() == 1)
                .and_then(ActKind::from_letter)
                .ok_or_else(|| format!("`{letter}` is no act letter"))?;
            entries.push((texts.as_slice(), Meaning::Act(kind), Some(letter.clone())));
        }
        for (code, texts) in &self.tables {
            let act = Act::from_code(code)
                .filter(|act| matches!(act, Act::Table(_)))
                .ok_or_else(|| format!("`{code}` is no table code"))?;
            entries.push((texts.as_slice(), Meaning::Document(act), Some(code.clone())));
        }
        for (series, texts) in &self.series {
            entries.push((
                texts.as_slice(),
                Meaning::Document(Act::Series(*series)),
                Some(series.code().to_owned()),
            ));
        }
        Ok(entries)
    }

    /// Every other phrase: register words, keywords and months.
    fn plain(&self) -> Vec<Entry<'_>> {
        let mut entries: Vec<Entry<'_>> = vec![
            (&self.registers.any, Meaning::Family(Family::Either), None),
            (
                &self.registers.parish,
                Meaning::Family(Family::Parish),
                None,
            ),
            (&self.registers.civil, Meaning::Family(Family::Civil), None),
            (&self.views, Meaning::View, Some("view".to_owned())),
            (&self.view_counts, Meaning::ViewCount, None),
            (&self.numbers, Meaning::Number, None),
            (&self.other_numbers, Meaning::OtherNumber, None),
            (&self.folios, Meaning::Folio, None),
            (&self.pages, Meaning::Page, None),
            (&self.recto, Meaning::Recto, None),
            (&self.verso, Meaning::Verso, None),
            (&self.call_numbers, Meaning::CallNumber, None),
            (&self.parishes, Meaning::Parish, None),
            (&self.places, Meaning::Place, None),
            (&self.bureaus, Meaning::Bureau, None),
            (&self.ranges, Meaning::Range, None),
            (&self.republican_years, Meaning::RepublicanYear, None),
            (&self.ignored, Meaning::Ignored, None),
        ];
        for (month, texts) in self.months.iter().enumerate() {
            let month = u8::try_from(month).expect("twelve months");
            entries.push((texts, Meaning::Month(month), None));
        }
        entries
    }
}

/// A vocabulary document the loader refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyError(String);

impl fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VocabularyError {}

/// A phrase as folded words: `arch. dép.` is `["arch", "dep"]`.
pub(crate) type Phrase = Vec<String>;

/// Folds a word or phrase as the recognizer compares them: case, accents
/// and punctuation set aside, and a degree sign after a letter read as the
/// `o` it abbreviates (`n°` is `no`, `f°` is `fo`, `v°` is `vo`).
pub(crate) fn fold(text: &str) -> Phrase {
    let mut spelled = String::with_capacity(text.len());
    let mut after_letter = false;
    for c in text.chars() {
        if matches!(c, '°' | 'º') && after_letter {
            spelled.push('o');
        } else {
            spelled.push(c);
        }
        after_letter = c.is_alphabetic();
    }
    fold_words(&spelled)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// One language's words, folded for matching.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    language: String,
    countries: Vec<String>,
    civil_registration_from: Option<u16>,
    phrases: Vec<(Phrase, Meaning)>,
    particles: Vec<Phrase>,
    right: Vec<String>,
    left: Vec<String>,
    /// The words written back into a citation (Archive Portals §6.5): the
    /// first phrase of each act, table and series, and the first view word.
    written: BTreeMap<String, String>,
}

impl Vocabulary {
    /// Reads one vocabulary document.
    pub fn parse(document: &str) -> Result<Self, VocabularyError> {
        let document: Document =
            serde_json::from_str(document).map_err(|error| VocabularyError(error.to_string()))?;
        Self::compile(document)
    }

    fn compile(document: Document) -> Result<Self, VocabularyError> {
        if document.language.trim().is_empty() {
            return Err(VocabularyError(
                "a vocabulary needs its language".to_owned(),
            ));
        }
        let error = |message: String| VocabularyError(format!("{}: {message}", document.language));
        if !document.months.is_empty() && document.months.len() != 12 {
            return Err(error("months are twelve lists, January first".to_owned()));
        }
        let mut entries = document.coded().map_err(error)?;
        entries.extend(document.plain());
        let mut phrases = Vec::new();
        let mut written = BTreeMap::new();
        for (texts, meaning, key) in entries {
            for text in texts {
                let phrase = fold(text);
                if phrase.is_empty() {
                    return Err(error(format!("`{text}` has no word")));
                }
                phrases.push((phrase, meaning.clone()));
            }
            if let (Some(key), Some(first)) = (key, texts.first()) {
                written.insert(key, first.clone());
            }
        }
        let single = |texts: &[String]| -> Vec<String> {
            texts.iter().map(|text| fold(text).join(" ")).collect()
        };
        Ok(Self {
            right: single(&document.sides.right),
            left: single(&document.sides.left),
            particles: document.particles.iter().map(|text| fold(text)).collect(),
            language: document.language,
            countries: document.countries,
            civil_registration_from: document.civil_registration_from,
            phrases,
            written,
        })
    }

    /// The language code the document names, such as `fr`.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Whether this language's conventions apply to a country's archives.
    pub fn serves(&self, country: &str) -> bool {
        self.countries.iter().any(|served| served == country)
    }

    /// The year a country's civil registration began, when a vocabulary
    /// serving it says.
    pub(crate) fn civil_registration_from(&self) -> Option<u16> {
        self.civil_registration_from
    }

    pub(crate) fn phrases(&self) -> &[(Phrase, Meaning)] {
        &self.phrases
    }

    pub(crate) fn is_particle(&self, word: &str) -> bool {
        self.particles
            .iter()
            .any(|particle| particle.len() == 1 && particle[0] == word)
    }

    /// The side a folded suffix names.
    pub(crate) fn side(&self, suffix: &str) -> Option<crate::citation::Side> {
        if self.right.iter().any(|right| right == suffix) {
            Some(crate::citation::Side::Right)
        } else if self.left.iter().any(|left| left == suffix) {
            Some(crate::citation::Side::Left)
        } else {
            None
        }
    }

    /// The words this language writes a document kind with, or a view with
    /// (`view`), for writing found parts back into a citation.
    pub(crate) fn written(&self, key: &str) -> Option<&str> {
        self.written.get(key).map(String::as_str)
    }
}

/// The embedded vocabularies. Their validity is a test of this crate.
pub(crate) fn embedded() -> Vec<Vocabulary> {
    EMBEDDED
        .iter()
        .map(|document| {
            Vocabulary::parse(document)
                .unwrap_or_else(|error| panic!("an embedded citation vocabulary: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_vocabularies_load() {
        let vocabularies = embedded();
        assert!(
            vocabularies
                .iter()
                .any(|vocabulary| vocabulary.language() == "fr")
        );
        let french = vocabularies
            .iter()
            .find(|vocabulary| vocabulary.language() == "fr")
            .unwrap();
        assert!(french.serves("FR"));
        assert_eq!(french.civil_registration_from(), Some(1793));
        assert_eq!(french.written("N"), Some("naissance"));
        assert_eq!(french.written("view"), Some("vue"));
    }

    #[test]
    fn folding_reads_a_degree_sign_as_its_letter() {
        assert_eq!(fold("n° 312"), ["no", "312"]);
        assert_eq!(fold("f° 23 v°"), ["fo", "23", "vo"]);
        assert_eq!(fold("Arch. dép."), ["arch", "dep"]);
        assert_eq!(fold("°"), Vec::<String>::new());
    }

    #[test]
    fn a_malformed_vocabulary_is_refused() {
        for document in [
            r#"{"language": ""}"#,
            r#"{"language": "xx", "acts": {"X": ["x"]}}"#,
            r#"{"language": "xx", "acts": {"NM": ["x"]}}"#,
            r#"{"language": "xx", "tables": {"N": ["x"]}}"#,
            r#"{"language": "xx", "archives": {"galactic": ["x"]}}"#,
            r#"{"language": "xx", "views": ["--"]}"#,
            r#"{"language": "xx", "months": [["a"]]}"#,
            r#"{"language": "xx", "unknown": []}"#,
        ] {
            assert!(Vocabulary::parse(document).is_err(), "{document}");
        }
    }
}
