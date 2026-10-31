//! Decompresses and indexes the embedded reference-data JSON (see
//! `build.rs`), and resolves free-text GEDCOM values (occupation labels,
//! given names) to the matching content entry.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::OnceLock;

use oxidgene_core::search::fold_words;
use serde::Deserialize;
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};

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
    /// Who bears the name: one sex, or both (the default) when the name or
    /// one of its aliases is given to men and women alike somewhere.
    #[serde(default, skip_serializing)]
    pub sex: SheetSex,
}

/// The sex a given-name sheet is borne by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SheetSex {
    Male,
    Female,
    #[default]
    Both,
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

fn decompress_json<T: serde::de::DeserializeOwned>(compressed: &[u8]) -> T {
    serde_json::from_slice(&embedded::decompress(compressed))
        .expect("embedded reference data must be valid JSON")
}

/// What the index is built from: a sheet's label, aliases and sex. Its texts
/// are skipped unread.
#[derive(Deserialize)]
struct Outline {
    label: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    sex: SheetSex,
}

/// No key claims a term in that place.
const UNCLAIMED: u32 = u32::MAX;

/// The keys a normalized term names, as positions in [`Reference::keys`].
struct Claims {
    /// The key folding to the term, which names it in every language.
    key: u32,
    /// Per file, in [`ReferenceLang::ALL`] order, the key that first lists
    /// the term among its aliases.
    aliases: [u32; 8],
}

impl Claims {
    fn new() -> Self {
        Self {
            key: UNCLAIMED,
            aliases: [UNCLAIMED; 8],
        }
    }

    /// The key the term names in `lang`: the key folding to it, else the
    /// language's own alias, else the other files' in [`ReferenceLang::ALL`]
    /// order.
    fn key_for(&self, lang: ReferenceLang) -> Option<u32> {
        let own = lang.slot();
        let others = self
            .aliases
            .iter()
            .enumerate()
            .filter(move |(file, _)| *file != own)
            .map(|(_, key)| *key);
        [self.key, self.aliases[own]]
            .into_iter()
            .chain(others)
            .find(|key| *key != UNCLAIMED)
    }
}

/// One kind of reference content in every language.
///
/// Each language's file holds the entries written in that language under
/// shared keys, and English holds them all: a key missing from a language
/// reads the English entry. A record's term may be in any language — a
/// Polish register's "Kmieć" read by a French interface — so every
/// language resolves the aliases of all the files. Keys come first, then the
/// language's own aliases, then the other files' in [`ReferenceLang::ALL`]
/// order; the first to claim a term keeps it.
///
/// Only what resolves a term stays in memory: the keys, one index of the
/// normalized terms for every language, the spellings suggestions offer and
/// each sheet's sex, some 10 MB. The sheets themselves — their texts are two
/// thirds of the 21 MB the files decompress to — are read from the embedded
/// file of the language a lookup asks for, for that lookup only (see
/// [`Self::sheets`]), a few milliseconds. Kept whole, with an index per
/// language, the tables held some 75 MB in every process from startup on,
/// whether a sheet was ever shown or not.
struct Reference<T> {
    files: &'static [&'static [u8]; 8],
    /// Every key of every file, sorted.
    keys: Vec<Box<str>>,
    /// Each key's sheet's sex, as English gives it.
    sexes: Vec<SheetSex>,
    /// Each normalized key or alias, and the keys it names.
    index: HashMap<Box<str>, Claims>,
    /// Every label and alias as some file writes it, for suggestions.
    terms: Vec<Term>,
    sheet: PhantomData<fn() -> T>,
}

/// A term a sheet answers to, spelled as one of the files spells it.
struct Term {
    folded: Box<str>,
    written: Box<str>,
    /// The file it comes from, as a [`ReferenceLang::slot`].
    file: usize,
}

impl<T: serde::de::DeserializeOwned> Reference<T> {
    fn load(files: &'static [&'static [u8]; 8]) -> Self {
        let outlines: Vec<HashMap<String, Outline>> =
            files.iter().map(|file| decompress_json(file)).collect();
        let mut keys: Vec<&String> = outlines.iter().flat_map(HashMap::keys).collect();
        keys.sort();
        keys.dedup();
        let position: HashMap<&str, u32> = keys
            .iter()
            .zip(0u32..)
            .map(|(key, at)| (key.as_str(), at))
            .collect();
        let mut index: HashMap<Box<str>, Claims> = HashMap::new();
        // Sorted, so that of two keys folding alike the last names the term,
        // in every language.
        for (key, at) in keys.iter().zip(0u32..) {
            index
                .entry(fold_words(key).into())
                .or_insert_with(Claims::new)
                .key = at;
        }
        // Sorted, so that an alias two entries of a file share always goes
        // to the same one.
        let sorted: Vec<Vec<(&String, &Outline)>> = outlines
            .iter()
            .map(|outline| {
                let mut entries: Vec<_> = outline.iter().collect();
                entries.sort_by_key(|(key, _)| *key);
                entries
            })
            .collect();
        for (file, entries) in sorted.iter().enumerate() {
            for (key, entry) in entries {
                for alias in &entry.aliases {
                    let claims = index
                        .entry(fold_words(alias).into())
                        .or_insert_with(Claims::new);
                    if claims.aliases[file] == UNCLAIMED {
                        claims.aliases[file] = position[key.as_str()];
                    }
                }
            }
        }
        let mut terms = Vec::new();
        for (file, entries) in sorted.iter().enumerate() {
            for (_, entry) in entries {
                let written = std::iter::once(without_gloss(&entry.label))
                    .chain(entry.aliases.iter().map(String::as_str));
                for written in written {
                    let folded = fold_words(written);
                    if !folded.is_empty() {
                        terms.push(Term {
                            folded: folded.into(),
                            written: written.trim().into(),
                            file,
                        });
                    }
                }
            }
        }
        terms.sort_by(|a, b| (&a.folded, a.file, &a.written).cmp(&(&b.folded, b.file, &b.written)));
        terms.dedup_by(|a, b| a.folded == b.folded && a.file == b.file);
        terms.shrink_to_fit();
        let english = &outlines[ReferenceLang::En.slot()];
        let sexes = keys
            .iter()
            .map(|key| {
                english
                    .get(*key)
                    .map(|outline| outline.sex)
                    .unwrap_or_default()
            })
            .collect();
        let keys = keys.into_iter().map(|key| key.as_str().into()).collect();
        index.shrink_to_fit();
        Self {
            files,
            keys,
            sexes,
            index,
            terms,
            sheet: PhantomData,
        }
    }

    /// The key a normalized term names in `lang`.
    fn key(&self, lang: ReferenceLang, term: &str) -> Option<u32> {
        self.index.get(term)?.key_for(lang)
    }

    /// The key of the longest normalized term of the index occurring as a
    /// whole-word run inside `haystack`, as `lang` resolves it. Longest wins
    /// so a more specific multi-word entry ("barbier perruquier") is
    /// preferred over a shorter one it contains ("barbier").
    fn longest_word_run(&self, lang: ReferenceLang, haystack: &str) -> Option<u32> {
        let haystack_words: Vec<&str> = haystack.split(' ').filter(|w| !w.is_empty()).collect();
        self.longest_word_runs(lang, std::slice::from_ref(&haystack_words))[0]
    }

    /// [`Self::longest_word_run`] for each of `haystacks`, already split in
    /// words, in one scan of the index.
    fn longest_word_runs(&self, lang: ReferenceLang, haystacks: &[Vec<&str>]) -> Vec<Option<u32>> {
        let mut best: Vec<Option<(&str, u32)>> = vec![None; haystacks.len()];
        for (term, claims) in &self.index {
            if term.is_empty() {
                continue;
            }
            let Some(key) = claims.key_for(lang) else {
                continue;
            };
            let term_words: Vec<&str> = term.split(' ').collect();
            for (slot, haystack_words) in best.iter_mut().zip(haystacks) {
                if contains_word_run(haystack_words, &term_words)
                    && slot.is_none_or(|(best_term, _)| term.len() > best_term.len())
                {
                    *slot = Some((term, key));
                }
            }
        }
        best.into_iter()
            .map(|slot| slot.map(|(_, key)| key))
            .collect()
    }

    /// The sheets of `wanted` in `lang`, each in English when `lang` has
    /// none: the language's file is decompressed for this lookup, the
    /// sheets asked for are deserialized, and nothing is kept.
    fn sheets(&self, lang: ReferenceLang, wanted: &[u32]) -> HashMap<u32, T> {
        let mut found = HashMap::new();
        let mut names: HashMap<&str, u32> = wanted
            .iter()
            .map(|&key| (&*self.keys[key as usize], key))
            .collect();
        for file in [lang, ReferenceLang::En] {
            if names.is_empty() {
                break;
            }
            pick(self.files[file.slot()], &names, &mut found);
            names.retain(|_, key| !found.contains_key(key));
        }
        found
    }

    /// Up to `limit` terms with a word starting with the normalized `query`,
    /// each spelled once: terms starting with it first, then the
    /// language's own spellings, then the shortest.
    fn suggest(&self, lang: ReferenceLang, query: &str, limit: usize) -> Vec<String> {
        let own = lang.slot();
        let mut found: Vec<&Term> = self
            .terms
            .iter()
            .filter(|term| starts_a_word(&term.folded, query))
            // A label cut of its gloss may name no sheet.
            .filter(|term| self.index.contains_key(&term.folded))
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
            .filter(|term| seen.insert(&*term.folded))
            .take(limit)
            .map(|term| term.written.to_string())
            .collect()
    }
}

/// Deserialize from the embedded file `compressed` the sheets of the keys of
/// `wanted` into `found`, under their key's position; every other sheet is
/// skipped unread.
fn pick<T: serde::de::DeserializeOwned>(
    compressed: &[u8],
    wanted: &HashMap<&str, u32>,
    found: &mut HashMap<u32, T>,
) {
    let bytes = embedded::decompress(compressed);
    let mut deserializer = serde_json::Deserializer::from_slice(&bytes);
    Pick { wanted, found }
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end())
        .expect("embedded reference data must be valid JSON");
}

/// A visitor of a file's map of sheets keeping those it is asked for.
struct Pick<'a, T> {
    wanted: &'a HashMap<&'a str, u32>,
    found: &'a mut HashMap<u32, T>,
}

impl<'de, T: serde::de::DeserializeOwned> DeserializeSeed<'de> for Pick<'_, T> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de, T: serde::de::DeserializeOwned> Visitor<'de> for Pick<'_, T> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a map of reference sheets")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match self.wanted.get(key.as_str()) {
                Some(&at) => {
                    let sheet = map.next_value::<T>()?;
                    self.found.insert(at, sheet);
                }
                None => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
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
    OCCUPATION_TABLES.get_or_init(|| Reference::load(&OCCUPATIONS))
}

fn given_names() -> &'static Reference<GivenNameEntry> {
    static GIVEN_NAME_TABLES: OnceLock<Reference<GivenNameEntry>> = OnceLock::new();
    GIVEN_NAME_TABLES.get_or_init(|| Reference::load(&GIVEN_NAMES))
}

/// Builds the index of every reference table up front.
///
/// Each table is Brotli-compressed JSON, decompressed and indexed on first
/// lookup. Left lazy, that cost lands on whichever async worker happens to
/// serve the first tooltip request and blocks it for tens of milliseconds,
/// with any concurrent lookup queued behind the same `OnceLock`. Call this
/// from a blocking context at startup so no request ever pays for it. What
/// reading the files allocated and the index does not keep is handed back to
/// the system afterwards.
#[tracing::instrument(name = "reference.preheat")]
pub fn preheat() {
    occupations();
    given_names();
    crate::memory::release_free_memory();
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
    let key = occupations
        .key(lang, &normalized)
        .or_else(|| occupations.longest_word_run(lang, &normalized))?;
    occupations.sheets(lang, &[key]).remove(&key)
}

/// Resolves several occupation terms in one pass over the table.
///
/// Returns one slot per input term, in order, so the caller can pair results
/// back with the terms it asked for. The word-run fallback scans every
/// key/alias in the table, so running it per term costs one full scan per
/// term; here the unresolved terms share a single scan, and the sheets one
/// reading of the language's file.
pub fn lookup_occupations(lang: ReferenceLang, terms: &[String]) -> Vec<Option<OccupationEntry>> {
    let occupations = occupations();
    let normalized = terms
        .iter()
        .map(|term| fold_words(term))
        .collect::<Vec<_>>();
    let mut resolved: Vec<Option<u32>> = normalized
        .iter()
        .map(|term| occupations.key(lang, term))
        .collect::<Vec<_>>();

    // Only the exact misses need the fallback, and they all share one scan.
    let (pending, haystacks): (Vec<usize>, Vec<Vec<&str>>) = resolved
        .iter()
        .enumerate()
        .filter(|(_, key)| key.is_none())
        .map(|(index, _)| {
            let words: Vec<&str> = normalized[index]
                .split(' ')
                .filter(|word| !word.is_empty())
                .collect();
            (index, words)
        })
        .unzip();
    if !pending.is_empty() {
        let found = occupations.longest_word_runs(lang, &haystacks);
        for (index, key) in pending.into_iter().zip(found) {
            resolved[index] = key;
        }
    }
    let wanted: Vec<u32> = resolved.iter().flatten().copied().collect();
    let sheets = occupations.sheets(lang, &wanted);
    resolved
        .into_iter()
        .map(|key| sheets.get(&key?).cloned())
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

/// Looks up a given-name fiche. Tries the full (possibly compound) term
/// first, then falls back to its first token — so "Marie-Claire" still
/// resolves via "Marie" if the compound itself has no dedicated entry.
pub fn lookup_given_name(lang: ReferenceLang, term: &str) -> Option<GivenNameEntry> {
    lookup_given_names(lang, std::slice::from_ref(&term.to_string()))
        .pop()
        .flatten()
}

/// [`lookup_given_name`] for each of `terms`, in order, reading the
/// language's file once for them all.
pub fn lookup_given_names(lang: ReferenceLang, terms: &[String]) -> Vec<Option<GivenNameEntry>> {
    let given_names = given_names();
    let resolved: Vec<Option<u32>> = terms
        .iter()
        .map(|term| {
            let full = fold_words(term);
            given_names
                .key(lang, &full)
                .or_else(|| given_names.key(lang, full.split(' ').next()?))
        })
        .collect();
    let wanted: Vec<u32> = resolved.iter().flatten().copied().collect();
    let sheets = given_names.sheets(lang, &wanted);
    resolved
        .into_iter()
        .map(|key| sheets.get(&key?).cloned())
        .collect()
}

/// The sex of the given-name sheet `term` itself names (not a word inside
/// it, unlike [`lookup_given_name`]): `Some` only for a name the sheets give
/// to one sex alone. The sex is the sheet's, whatever language reads it.
pub fn given_name_sex(term: &str) -> Option<oxidgene_core::Sex> {
    let given_names = given_names();
    let key = given_names.key(ReferenceLang::En, &fold_words(term))?;
    match given_names.sexes[key as usize] {
        SheetSex::Male => Some(oxidgene_core::Sex::Male),
        SheetSex::Female => Some(oxidgene_core::Sex::Female),
        SheetSex::Both => None,
    }
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

/// Whether `term` itself, not a word inside it, names a sheet. Every
/// language resolves the same terms, so the language does not change the
/// answer.
pub fn has_sheet(kind: ReferenceKind, term: &str) -> bool {
    let term = fold_words(term);
    match kind {
        ReferenceKind::Occupations => occupations().index.contains_key(term.as_str()),
        ReferenceKind::GivenNames => given_names().index.contains_key(term.as_str()),
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

    /// The embedded file of `sheets`, compressed as the build script does.
    fn embedded_file(sheets: &serde_json::Value) -> &'static [u8] {
        use std::io::Write;
        let mut compressed = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(&mut compressed, 1 << 16, 5, 22);
            encoder
                .write_all(sheets.to_string().as_bytes())
                .expect("compress");
        }
        Vec::leak(compressed)
    }

    #[test]
    fn a_sheet_missing_from_a_language_reads_in_english() {
        // Every embedded sheet is translated today, so the fallback is shown
        // on files of its own: a sheet only English holds.
        let sheet = |label: &str| serde_json::json!({ "label": label, "summary": "", "text": "" });
        let files: [&'static [u8]; 8] = std::array::from_fn(|slot| {
            let sheets = match ReferenceLang::ALL[slot] {
                ReferenceLang::En => serde_json::json!({
                    "sheet_a": sheet("Sheet A"),
                    "sheet_b": sheet("Sheet B"),
                }),
                ReferenceLang::Fr => serde_json::json!({ "sheet_b": sheet("Fiche B") }),
                _ => serde_json::json!({}),
            };
            embedded_file(&sheets)
        });
        let reference = Reference::<OccupationEntry>::load(Box::leak(Box::new(files)));
        let label = |lang, term: &str| {
            let key = reference.key(lang, term)?;
            reference.sheets(lang, &[key]).remove(&key).map(|e| e.label)
        };
        assert_eq!(
            label(ReferenceLang::It, "sheet a").as_deref(),
            Some("Sheet A")
        );
        assert_eq!(
            label(ReferenceLang::Fr, "sheet b").as_deref(),
            Some("Fiche B")
        );
        assert_eq!(label(ReferenceLang::It, "sheet c"), None);
    }

    #[test]
    fn a_lookup_reads_only_the_sheets_it_asks_for() {
        let occupations = occupations();
        let keys: Vec<u32> = ["laboureur", "forgeron"]
            .iter()
            .filter_map(|term| occupations.key(ReferenceLang::Fr, term))
            .collect();
        assert_eq!(keys.len(), 2);
        let sheets = occupations.sheets(ReferenceLang::Fr, &keys);
        assert_eq!(sheets.len(), 2);
        assert!(sheets.values().all(|sheet| !sheet.text.is_empty()));
    }

    /// The index is what stays in memory in every process: it must stay a
    /// fraction of the sheets, which are read on demand.
    #[test]
    fn the_resident_index_is_a_fraction_of_the_sheets() {
        fn held<T>(reference: &Reference<T>) -> usize {
            let strings: usize = reference.keys.iter().map(|key| key.len()).sum::<usize>()
                + reference.index.keys().map(|term| term.len()).sum::<usize>()
                + reference
                    .terms
                    .iter()
                    .map(|term| term.folded.len() + term.written.len())
                    .sum::<usize>();
            strings
                + reference.keys.len() * size_of::<Box<str>>()
                + reference.index.capacity() * (size_of::<Box<str>>() + size_of::<Claims>() + 1)
                + reference.terms.capacity() * size_of::<Term>()
        }
        let decompressed: usize = OCCUPATIONS
            .iter()
            .chain(GIVEN_NAMES.iter())
            .map(|file| embedded::decompress(file).len())
            .sum();
        let resident = held(occupations()) + held(given_names());
        assert!(
            resident * 2 < decompressed,
            "the index holds {resident} bytes for {decompressed} bytes of sheets"
        );
    }

    #[test]
    fn a_shared_term_goes_to_the_interface_language_first() {
        // "Schipper" is a Dutch boatman and a German shoveller.
        let key = |lang| {
            let occupations = occupations();
            let key = occupations.key(lang, "schipper")?;
            Some(occupations.keys[key as usize].to_string())
        };
        assert_eq!(key(ReferenceLang::Nl).as_deref(), Some("batelier"));
        assert_eq!(key(ReferenceLang::De).as_deref(), Some("terrasseur"));
    }

    /// Every file decompressed whole, as test material.
    fn tables<T: serde::de::DeserializeOwned>(
        files: &[&'static [u8]; 8],
    ) -> Vec<HashMap<String, T>> {
        files.iter().map(|file| decompress_json(file)).collect()
    }

    #[test]
    fn english_holds_every_entry_of_every_language() {
        fn check(tables: &[HashMap<String, serde_json::Value>]) {
            let english = &tables[ReferenceLang::En.slot()];
            for (lang, table) in ReferenceLang::ALL.iter().zip(tables) {
                assert!(!table.is_empty(), "{lang:?} is empty");
                let missing: Vec<_> = table.keys().filter(|k| !english.contains_key(*k)).collect();
                assert!(
                    missing.is_empty(),
                    "{lang:?} entries without English: {missing:?}"
                );
            }
        }
        check(&tables(&OCCUPATIONS));
        check(&tables(&GIVEN_NAMES));
    }

    #[test]
    fn a_sheet_says_which_sex_bears_its_name() {
        use oxidgene_core::Sex;
        assert_eq!(given_name_sex("Jean-Baptiste"), Some(Sex::Male));
        assert_eq!(given_name_sex("MARGUERITE"), Some(Sex::Female));
        // An alias reads its sheet's sex, in any language's spelling.
        assert_eq!(given_name_sex("Jehanne"), given_name_sex("Jeanne"));
        // A name borne by both sexes (Jean is a woman's name in English), a
        // word inside a name and an unknown name say nothing.
        assert_eq!(given_name_sex("Camille"), None);
        assert_eq!(given_name_sex("Jean"), None);
        assert_eq!(given_name_sex("Marguerite Zorglub"), None);
        assert_eq!(given_name_sex("Zorglub"), None);
    }

    #[test]
    fn every_language_gives_a_sheet_the_same_sex() {
        let sheets: Vec<HashMap<String, GivenNameEntry>> = tables(&GIVEN_NAMES);
        let english = &sheets[ReferenceLang::En.slot()];
        for (lang, table) in ReferenceLang::ALL.iter().zip(&sheets) {
            for (key, sheet) in table {
                assert_eq!(sheet.sex, english[key].sex, "{lang:?} {key}");
            }
        }
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
        assert!(has_sheet(ReferenceKind::Occupations, "laboureur/euse"));
        assert!(has_sheet(ReferenceKind::Occupations, "Kmieć"));
        // The tooltip finds "CTO" inside this, but it is not itself a term.
        assert!(!has_sheet(
            ReferenceKind::Occupations,
            "CTO chez Entreprise Exemple"
        ));
        assert!(has_sheet(ReferenceKind::GivenNames, "JEAN"));
        assert!(!has_sheet(ReferenceKind::GivenNames, "Zorglub"));
    }
}
