//! Decompresses and indexes the embedded reference-data JSON (see
//! `build.rs`), and resolves free-text GEDCOM values (occupation labels,
//! given names) to the matching content entry.

use std::collections::HashMap;
use std::sync::OnceLock;

use oxidgene_core::search::fold_words;
use serde::Deserialize;

use crate::embedded;

/// Reference-content language. Deliberately independent of `oxidgene-ui`'s
/// `Language` type — this crate has no UI dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceLang {
    Fr,
    En,
    De,
    Es,
    It,
    Nl,
    Pl,
    Pt,
}

impl ReferenceLang {
    pub const ALL: [Self; 8] = [
        Self::Fr,
        Self::En,
        Self::De,
        Self::Es,
        Self::It,
        Self::Nl,
        Self::Pl,
        Self::Pt,
    ];

    /// Parse a BCP-47-ish path segment (`"fr"`, `"en"`, `"de"`…).
    pub fn from_code(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|lang| lang.code() == s)
    }

    /// Position in [`Self::ALL`], for per-language tables.
    pub(super) fn slot(self) -> usize {
        Self::ALL
            .iter()
            .position(|lang| *lang == self)
            .expect("every language is in ALL")
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Fr => "fr",
            Self::En => "en",
            Self::De => "de",
            Self::Es => "es",
            Self::It => "it",
            Self::Nl => "nl",
            Self::Pl => "pl",
            Self::Pt => "pt",
        }
    }
}

/// The error a caller gets for a language code it cannot use.
pub const UNSUPPORTED_LANGUAGE: &str =
    "language must be one of `fr`, `en`, `de`, `es`, `it`, `nl`, `pl`, `pt`";

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

macro_rules! embed_br {
    ($name:literal) => {
        include_bytes!(concat!(env!("OUT_DIR"), "/", $name, ".br"))
    };
}

/// One file per language, in [`ReferenceLang::ALL`] order.
static OCCUPATIONS: [&[u8]; 8] = [
    embed_br!("occupations.fr.json"),
    embed_br!("occupations.en.json"),
    embed_br!("occupations.de.json"),
    embed_br!("occupations.es.json"),
    embed_br!("occupations.it.json"),
    embed_br!("occupations.nl.json"),
    embed_br!("occupations.pl.json"),
    embed_br!("occupations.pt.json"),
];
static GIVEN_NAMES: [&[u8]; 8] = [
    embed_br!("given_names.fr.json"),
    embed_br!("given_names.en.json"),
    embed_br!("given_names.de.json"),
    embed_br!("given_names.es.json"),
    embed_br!("given_names.it.json"),
    embed_br!("given_names.nl.json"),
    embed_br!("given_names.pl.json"),
    embed_br!("given_names.pt.json"),
];

fn decompress_json<T: serde::de::DeserializeOwned>(compressed: &'static [u8]) -> T {
    serde_json::from_slice(&embedded::decompress(compressed))
        .expect("embedded reference data must be valid JSON")
}

/// One kind of reference content in every language.
///
/// Each language's file holds the entries written in that language under
/// shared keys, and English holds them all: a key missing from a language
/// reads the English entry. A record's term may be in any language — a
/// Polish register's "Kmieć" read by a French interface — so every
/// language's index resolves the aliases of all the files. Keys come first,
/// then the language's own aliases, then the other files' in
/// [`ReferenceLang::ALL`] order; the first to claim a term keeps it.
struct Reference<T> {
    tables: Vec<HashMap<String, T>>,
    /// Per language: each normalized key or alias, and the key it names.
    indexes: Vec<HashMap<String, String>>,
    /// Every label and alias as some file writes it, for suggestions.
    terms: Vec<Term>,
}

/// A term a sheet answers to, spelled as one of the files spells it.
struct Term {
    folded: String,
    written: String,
    /// The file it comes from, as a [`ReferenceLang::slot`].
    file: usize,
}

impl<T: serde::de::DeserializeOwned> Reference<T> {
    fn load(
        files: &[&'static [u8]; 8],
        label_of: impl Fn(&T) -> &str,
        aliases_of: impl Fn(&T) -> &[String],
    ) -> Self {
        let tables: Vec<HashMap<String, T>> =
            files.iter().map(|file| decompress_json(file)).collect();
        let mut keys: Vec<&String> = tables.iter().flat_map(HashMap::keys).collect();
        keys.sort();
        keys.dedup();
        // Sorted, so that an alias two entries of a file share always goes
        // to the same one.
        let sorted: Vec<Vec<(&String, &T)>> = tables
            .iter()
            .map(|table| {
                let mut entries: Vec<_> = table.iter().collect();
                entries.sort_by_key(|(key, _)| *key);
                entries
            })
            .collect();
        let indexes = (0..tables.len())
            .map(|own| {
                let mut index: HashMap<String, String> = keys
                    .iter()
                    .map(|key| (fold_words(key), (*key).clone()))
                    .collect();
                let others = (0..tables.len()).filter(|&other| other != own);
                for file in std::iter::once(own).chain(others) {
                    for (key, entry) in &sorted[file] {
                        for alias in aliases_of(entry) {
                            index
                                .entry(fold_words(alias))
                                .or_insert_with(|| (*key).clone());
                        }
                    }
                }
                index
            })
            .collect();
        let mut terms = Vec::new();
        for (file, entries) in sorted.iter().enumerate() {
            for (_, entry) in entries {
                let written = std::iter::once(without_gloss(label_of(entry)))
                    .chain(aliases_of(entry).iter().map(String::as_str));
                for written in written {
                    let folded = fold_words(written);
                    if !folded.is_empty() {
                        terms.push(Term {
                            folded,
                            written: written.trim().to_string(),
                            file,
                        });
                    }
                }
            }
        }
        terms.sort_by(|a, b| (&a.folded, a.file, &a.written).cmp(&(&b.folded, b.file, &b.written)));
        terms.dedup_by(|a, b| a.folded == b.folded && a.file == b.file);
        Self {
            tables,
            indexes,
            terms,
        }
    }

    fn index(&self, lang: ReferenceLang) -> &HashMap<String, String> {
        &self.indexes[lang.slot()]
    }

    /// The entry for `key` in `lang`, or in English when `lang` has none.
    fn entry(&self, lang: ReferenceLang, key: &str) -> Option<&T> {
        self.tables[lang.slot()]
            .get(key)
            .or_else(|| self.tables[ReferenceLang::En.slot()].get(key))
    }

    /// The entry a normalized term names.
    fn get(&self, lang: ReferenceLang, term: &str) -> Option<&T> {
        self.entry(lang, self.index(lang).get(term)?)
    }

    /// Up to `limit` terms with a word starting with the normalized `query`,
    /// each spelled once: terms starting with it first, then the
    /// language's own spellings, then the shortest.
    fn suggest(&self, lang: ReferenceLang, query: &str, limit: usize) -> Vec<String> {
        let index = self.index(lang);
        let own = lang.slot();
        let mut found: Vec<&Term> = self
            .terms
            .iter()
            .filter(|term| starts_a_word(&term.folded, query))
            // A label cut of its gloss may name no sheet in this language.
            .filter(|term| index.contains_key(&term.folded))
            .collect();
        found.sort_by_key(|term| {
            (
                !term.folded.starts_with(query),
                term.file != own,
                term.folded.len(),
                &term.folded,
                term.file,
            )
        });
        let mut seen = std::collections::HashSet::new();
        found
            .into_iter()
            .filter(|term| seen.insert(term.folded.as_str()))
            .take(limit)
            .map(|term| term.written.clone())
            .collect()
    }
}

/// A label without the gloss some languages add in parentheses: the sheet
/// "Kmieć (paysan tenancier)" answers to "Kmieć".
fn without_gloss(label: &str) -> &str {
    match label
        .trim()
        .strip_suffix(')')
        .and_then(|l| l.rsplit_once(" ("))
    {
        Some((head, _)) => head,
        None => label,
    }
}

/// Whether `query` appears in `text` at the start of a word, both
/// normalized.
pub fn starts_a_word(text: &str, query: &str) -> bool {
    text.match_indices(query)
        .any(|(at, _)| at == 0 || text.as_bytes()[at - 1] == b' ')
}

fn occupations() -> &'static Reference<OccupationEntry> {
    static OCCUPATION_TABLES: OnceLock<Reference<OccupationEntry>> = OnceLock::new();
    OCCUPATION_TABLES.get_or_init(|| {
        Reference::load(
            &OCCUPATIONS,
            |e: &OccupationEntry| &e.label,
            |e: &OccupationEntry| &e.aliases,
        )
    })
}

fn given_names() -> &'static Reference<GivenNameEntry> {
    static GIVEN_NAME_TABLES: OnceLock<Reference<GivenNameEntry>> = OnceLock::new();
    GIVEN_NAME_TABLES.get_or_init(|| {
        Reference::load(
            &GIVEN_NAMES,
            |e: &GivenNameEntry| &e.label,
            |e: &GivenNameEntry| &e.aliases,
        )
    })
}

/// Builds every reference table up front.
///
/// Each table is Brotli-compressed JSON, decompressed and indexed on first
/// lookup. Left lazy, that cost lands on whichever async worker happens to
/// serve the first tooltip request and blocks it for tens of milliseconds,
/// with any concurrent lookup queued behind the same `OnceLock`. Call this
/// from a blocking context at startup so no request ever pays for it.
#[tracing::instrument(name = "reference.preheat")]
pub fn preheat() {
    occupations();
    given_names();
}

/// Looks up an occupation fiche by raw GEDCOM label (any case/accent/alias
/// variant listed in the data file). Free-text occupation fields (e.g. "CTO
/// chez Entreprise Exemple") rarely match a full entry verbatim, so on exact
/// miss this falls back to the longest dictionary key/alias that occurs as a
/// whole-word run inside the term — long enough to still tell "Barbier
/// Perruquier" apart from plain "Barbier" when both are present.
pub fn lookup_occupation(lang: ReferenceLang, term: &str) -> Option<OccupationEntry> {
    let occupations = occupations();
    let normalized = fold_words(term);
    if let Some(entry) = occupations.get(lang, &normalized) {
        return Some(entry.clone());
    }
    let key = longest_word_run_match(occupations.index(lang), &normalized)?;
    occupations.entry(lang, key).cloned()
}

/// Resolves several occupation terms in one pass over the table.
///
/// Returns one slot per input term, in order, so the caller can pair results
/// back with the terms it asked for. The word-run fallback scans every
/// key/alias in the table, so running it per term costs one full scan per
/// term; here the unresolved terms share a single scan.
pub fn lookup_occupations(lang: ReferenceLang, terms: &[String]) -> Vec<Option<OccupationEntry>> {
    let occupations = occupations();
    let index = occupations.index(lang);
    let normalized = terms
        .iter()
        .map(|term| fold_words(term))
        .collect::<Vec<_>>();
    let mut resolved: Vec<Option<&String>> = normalized
        .iter()
        .map(|term| index.get(term))
        .collect::<Vec<_>>();

    // Only the exact misses need the fallback, and they all share one scan.
    let pending = resolved
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.is_none())
        .map(|(index, _)| {
            let words: Vec<&str> = normalized[index]
                .split(' ')
                .filter(|word| !word.is_empty())
                .collect();
            (index, words)
        })
        .collect::<Vec<_>>();
    if !pending.is_empty() {
        let mut best: Vec<Option<(&str, &String)>> = vec![None; pending.len()];
        for (term, key) in index {
            if term.is_empty() {
                continue;
            }
            let term_words: Vec<&str> = term.split(' ').collect();
            for (slot, (_, haystack_words)) in best.iter_mut().zip(pending.iter()) {
                if contains_word_run(haystack_words, &term_words)
                    && slot.is_none_or(|(best_term, _)| term.len() > best_term.len())
                {
                    *slot = Some((term.as_str(), key));
                }
            }
        }
        for ((index, _), slot) in pending.into_iter().zip(best) {
            resolved[index] = slot.map(|(_, key)| key);
        }
    }
    resolved
        .into_iter()
        .map(|key| occupations.entry(lang, key?).cloned())
        .collect()
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
    let given_names = given_names();
    let full = fold_words(term);
    if let Some(entry) = given_names.get(lang, &full) {
        return Some(entry.clone());
    }
    let first_token = full.split(' ').next()?;
    given_names.get(lang, first_token).cloned()
}

/// Which sheets [`suggest_terms`] and [`has_sheet`] read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    Occupations,
    GivenNames,
}

/// Up to `limit` terms some sheet answers to, with a word starting with
/// `query`, in any language's spelling, `lang`'s own first. Each term is
/// spelled as a file writes it, so it can be entered as is.
pub fn suggest_terms(
    kind: ReferenceKind,
    lang: ReferenceLang,
    query: &str,
    limit: usize,
) -> Vec<String> {
    let query = fold_words(query);
    if query.is_empty() {
        return Vec::new();
    }
    match kind {
        ReferenceKind::Occupations => occupations().suggest(lang, &query, limit),
        ReferenceKind::GivenNames => given_names().suggest(lang, &query, limit),
    }
}

/// Whether `term` itself, not a word inside it, names a sheet.
pub fn has_sheet(kind: ReferenceKind, lang: ReferenceLang, term: &str) -> bool {
    let term = fold_words(term);
    match kind {
        ReferenceKind::Occupations => occupations().index(lang).contains_key(&term),
        ReferenceKind::GivenNames => given_names().index(lang).contains_key(&term),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The batch path resolves the same entries as the one-at-a-time path,
    /// exact hits and word-run fallbacks alike — it only shares the scan.
    #[test]
    fn batch_occupation_lookup_agrees_with_single_lookup() {
        let terms = [
            "Laboureur",
            "laboureur/euse",
            "CTO chez Entreprise Exemple",
            "Ancien Barbier Perruquier Retraité",
            "astronaute",
            "directoire",
        ]
        .map(str::to_string)
        .to_vec();

        let batch = lookup_occupations(ReferenceLang::Fr, &terms);

        assert_eq!(batch.len(), terms.len());
        for (term, entry) in terms.iter().zip(&batch) {
            assert_eq!(
                entry.as_ref().map(|e| e.label.as_str()),
                lookup_occupation(ReferenceLang::Fr, term)
                    .as_ref()
                    .map(|e| e.label.as_str()),
                "batch and single lookup disagree on {term:?}"
            );
        }
        assert_eq!(
            batch[0].as_ref().expect("canonical match").label,
            "Laboureur"
        );
        assert_eq!(
            batch[3].as_ref().expect("word-run fallback").label,
            "Barbier Perruquier"
        );
        assert!(batch[4].is_none());
    }

    #[test]
    fn a_term_from_any_language_reads_in_the_interface_language() {
        // A Polish register's term, read by a French interface.
        let entry = lookup_occupation(ReferenceLang::Fr, "Kmieć").expect("polish alias");
        assert_eq!(entry.label, "Kmieć (paysan tenancier)");
        // A French term, read by a German interface.
        let entry = lookup_occupation(ReferenceLang::De, "laboureur").expect("german sheet");
        assert_eq!(entry.label, "Ackermann");
        // A Polish given name, typed without its "ł", read by an English
        // interface.
        let entry = lookup_given_name(ReferenceLang::En, "Stanislaw").expect("polish name");
        assert_eq!(entry.label, "Stanisław");
        assert_eq!(
            lookup_given_name(ReferenceLang::Fr, "Jehan").map(|e| e.label),
            Some("Jean".to_string())
        );
    }

    #[test]
    fn a_sheet_missing_from_a_language_reads_in_english() {
        // Every embedded sheet is translated today, so the fallback is shown
        // on a table of its own: a sheet only English holds.
        let sheet = |label: &str| OccupationEntry {
            label: label.to_string(),
            summary: String::new(),
            text: String::new(),
            aliases: Vec::new(),
        };
        let mut tables: Vec<HashMap<String, OccupationEntry>> =
            ReferenceLang::ALL.iter().map(|_| HashMap::new()).collect();
        tables[ReferenceLang::En.slot()].insert("sheet_a".into(), sheet("Sheet A"));
        tables[ReferenceLang::Fr.slot()].insert("sheet_b".into(), sheet("Fiche B"));
        tables[ReferenceLang::En.slot()].insert("sheet_b".into(), sheet("Sheet B"));
        let reference = Reference {
            tables,
            indexes: Vec::new(),
            terms: Vec::new(),
        };
        let label = |lang, key| reference.entry(lang, key).map(|e| e.label.as_str());
        assert_eq!(label(ReferenceLang::It, "sheet_a"), Some("Sheet A"));
        assert_eq!(label(ReferenceLang::Fr, "sheet_b"), Some("Fiche B"));
        assert_eq!(label(ReferenceLang::It, "sheet_c"), None);
    }

    #[test]
    fn a_shared_term_goes_to_the_interface_language_first() {
        // "Schipper" is a Dutch boatman and a German shoveller.
        let key = |lang| occupations().index(lang).get("schipper").cloned();
        assert_eq!(key(ReferenceLang::Nl).as_deref(), Some("batelier"));
        assert_eq!(key(ReferenceLang::De).as_deref(), Some("terrasseur"));
    }

    #[test]
    fn english_holds_every_entry_of_every_language() {
        fn check<T>(reference: &Reference<T>) {
            let english = &reference.tables[ReferenceLang::En.slot()];
            for (lang, table) in ReferenceLang::ALL.iter().zip(&reference.tables) {
                assert!(!table.is_empty(), "{lang:?} is empty");
                let missing: Vec<_> = table.keys().filter(|k| !english.contains_key(*k)).collect();
                assert!(
                    missing.is_empty(),
                    "{lang:?} entries without English: {missing:?}"
                );
            }
        }
        check(occupations());
        check(given_names());
    }

    #[test]
    fn looks_up_given_name_with_compound_fallback() {
        let entry = lookup_given_name(ReferenceLang::Fr, "Marie-Claire").expect("fallback match");
        assert_eq!(entry.label, "Marie");

        let direct = lookup_given_name(ReferenceLang::En, "JEAN").expect("case-insensitive match");
        assert_eq!(direct.label, "Jean");

        assert!(lookup_given_name(ReferenceLang::Fr, "Zorglub").is_none());
    }

    #[test]
    fn suggests_terms_starting_with_the_query_in_the_language_first() {
        let terms = suggest_terms(ReferenceKind::Occupations, ReferenceLang::Fr, "Labou", 5);
        assert_eq!(terms.first().map(String::as_str), Some("Laboureur"));
        // Each spelling is offered once.
        let folded: Vec<String> = terms.iter().map(|t| fold_words(t)).collect();
        let mut unique = folded.clone();
        unique.dedup();
        assert_eq!(folded, unique);

        let given = suggest_terms(ReferenceKind::GivenNames, ReferenceLang::Fr, "jea", 3);
        assert_eq!(given.first().map(String::as_str), Some("Jean"));

        assert!(suggest_terms(ReferenceKind::GivenNames, ReferenceLang::Fr, " - ", 3).is_empty());
    }

    #[test]
    fn a_label_is_suggested_without_its_gloss() {
        assert_eq!(without_gloss("Kmieć (paysan tenancier)"), "Kmieć");
        assert_eq!(without_gloss("Laboureur"), "Laboureur");
        let terms = suggest_terms(ReferenceKind::Occupations, ReferenceLang::Fr, "kmie", 10);
        assert!(terms.iter().any(|t| t == "Kmieć"), "{terms:?}");
        assert!(terms.iter().all(|t| !t.contains('(')), "{terms:?}");
    }

    #[test]
    fn has_sheet_matches_the_whole_term_only() {
        assert!(has_sheet(
            ReferenceKind::Occupations,
            ReferenceLang::Fr,
            "laboureur/euse"
        ));
        assert!(has_sheet(
            ReferenceKind::Occupations,
            ReferenceLang::De,
            "Kmieć"
        ));
        // The tooltip finds "CTO" inside this, but it is not itself a term.
        assert!(!has_sheet(
            ReferenceKind::Occupations,
            ReferenceLang::Fr,
            "CTO chez Entreprise Exemple"
        ));
        assert!(has_sheet(
            ReferenceKind::GivenNames,
            ReferenceLang::Fr,
            "JEAN"
        ));
        assert!(!has_sheet(
            ReferenceKind::GivenNames,
            ReferenceLang::Fr,
            "Zorglub"
        ));
    }
}
