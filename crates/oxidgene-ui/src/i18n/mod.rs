//! Internationalization (i18n) module.
//!
//! Provides runtime language switching between the languages of the
//! countries the place dictionary covers: English, French, German, Spanish,
//! Italian, Dutch, Polish and Portuguese. Uses a Dioxus context signal for
//! reactive updates across all components.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod nl;
mod pl;
mod pt;

use std::collections::HashMap;

use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateDisplayFormat};
use oxidgene_core::types::Tree;

/// Supported languages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    En,
    Fr,
    De,
    Es,
    It,
    Nl,
    Pl,
    Pt,
}

impl Language {
    /// Every language, in the order the settings page offers them.
    pub const ALL: [Self; 8] = [
        Self::En,
        Self::Fr,
        Self::De,
        Self::Es,
        Self::It,
        Self::Nl,
        Self::Pl,
        Self::Pt,
    ];

    /// BCP-47 language code.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Fr => "fr",
            Self::De => "de",
            Self::Es => "es",
            Self::It => "it",
            Self::Nl => "nl",
            Self::Pl => "pl",
            Self::Pt => "pt",
        }
    }

    /// Native display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::En => "EN",
            Self::Fr => "FR",
            Self::De => "DE",
            Self::Es => "ES",
            Self::It => "IT",
            Self::Nl => "NL",
            Self::Pl => "PL",
            Self::Pt => "PT",
        }
    }

    /// The language's own name for itself.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Fr => "Français",
            Self::De => "Deutsch",
            Self::Es => "Español",
            Self::It => "Italiano",
            Self::Nl => "Nederlands",
            Self::Pl => "Polski",
            Self::Pt => "Português",
        }
    }

    /// The flag shown beside the language in the settings.
    pub fn flag(self) -> &'static str {
        match self {
            Self::En => "\u{1F1EC}\u{1F1E7}",
            Self::Fr => "\u{1F1EB}\u{1F1F7}",
            Self::De => "\u{1F1E9}\u{1F1EA}",
            Self::Es => "\u{1F1EA}\u{1F1F8}",
            Self::It => "\u{1F1EE}\u{1F1F9}",
            Self::Nl => "\u{1F1F3}\u{1F1F1}",
            Self::Pl => "\u{1F1F5}\u{1F1F1}",
            Self::Pt => "\u{1F1F5}\u{1F1F9}",
        }
    }

    /// The suffix of the plural form for `count`: `_one` or `_other`, and in
    /// Polish `_one`, `_few` or `_many`.
    pub fn plural_suffix(self, count: usize) -> &'static str {
        match self {
            // French treats zero as singular: "0 personne".
            Self::Fr if count <= 1 => "_one",
            Self::Pl if count == 1 => "_one",
            Self::Pl if (2..=4).contains(&(count % 10)) && !(12..=14).contains(&(count % 100)) => {
                "_few"
            }
            Self::Pl => "_many",
            _ if count == 1 => "_one",
            _ => "_other",
        }
    }

    /// Parse a BCP-47 code or prefix (e.g. "fr-FR" → Fr).
    ///
    /// Returns `None` for a language the UI has no translation for, so a
    /// caller walking a preference list can keep looking instead of settling
    /// on English at the first unknown entry.
    pub fn try_from_code(s: &str) -> Option<Self> {
        // Only the primary subtag matters: "fr", "fr-FR", "fr_CA" all map to Fr.
        let primary = s.split(['-', '_']).next().unwrap_or_default();
        Self::ALL
            .into_iter()
            .find(|language| language.code() == primary.to_ascii_lowercase())
    }

    /// Pick the best supported language from an ordered preference list.
    ///
    /// Mirrors how the platform exposes its preferences (`navigator.languages`
    /// is ordered most-preferred first): the first entry we have a translation
    /// for wins, so a user whose OS lists German then French gets French rather
    /// than English. English is the fallback when nothing matches — including
    /// when detection produced no list at all.
    pub fn from_preferences<'a>(codes: impl IntoIterator<Item = &'a str>) -> Self {
        codes
            .into_iter()
            .find_map(Self::try_from_code)
            .unwrap_or(Self::En)
    }

    /// The raw table for this language, with no fallback. `I18n::t` is what
    /// callers want; this exists so a test can assert a locale really carries
    /// a key, which `t` would hide behind its fallback to English.
    pub(crate) fn translations(self) -> &'static HashMap<String, String> {
        match self {
            Self::En => en::translations(),
            Self::Fr => fr::translations(),
            Self::De => de::translations(),
            Self::Es => es::translations(),
            Self::It => it::translations(),
            Self::Nl => nl::translations(),
            Self::Pl => pl::translations(),
            Self::Pt => pt::translations(),
        }
    }
}

/// How the tree being read writes its dates (`docs/ui-settings.md` §9): set
/// on the tree, and carried by [`I18n`] beside the language so that every
/// date the interface writes follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateStyle {
    /// How much of a date is written, and how.
    pub format: DateDisplayFormat,
    /// Whether a lifespan writes `* 1842 + 1907` rather than `1842-1907`.
    pub symbols: bool,
    /// Whether an approximate date reads « c. 1842 » rather than « about
    /// 1842 ».
    pub circa: bool,
    /// The calendar a date recorded in another one is also given in.
    pub calendar: Calendar,
}

impl DateStyle {
    /// A new tree's: the day, the month's name and the year, a lifespan
    /// joined by a dash, and the Gregorian equivalent of any other calendar.
    pub const DEFAULT: Self = Self {
        format: DateDisplayFormat::DayMonthYear,
        symbols: false,
        circa: false,
        calendar: Calendar::Gregorian,
    };

    /// The style `tree` sets.
    pub fn of(tree: &Tree) -> Self {
        Self {
            format: tree.date_format,
            symbols: tree.date_symbols,
            circa: tree.date_circa,
            calendar: tree.date_calendar,
        }
    }
}

/// Translation helper returned by [`use_i18n`]: how the interface writes for
/// its reader — the language, and the date style of the tree being read.
///
/// Because it reads from reactive signals, any component using it re-renders
/// when the language or the tree's date style changes.
#[derive(Clone, Copy, PartialEq)]
pub struct I18n(pub Language, pub DateStyle);

impl I18n {
    /// Text in `language`, dates in the default style.
    pub const fn new(language: Language) -> Self {
        Self(language, DateStyle::DEFAULT)
    }

    /// How dates are written.
    pub const fn dates(&self) -> DateStyle {
        self.1
    }

    /// The same language, writing dates in `style`.
    pub const fn with_dates(self, style: DateStyle) -> Self {
        Self(self.0, style)
    }

    /// Look up a translation key. Falls back to English, then to the key itself.
    pub fn t(&self, key: &str) -> String {
        self.try_t(key).unwrap_or_else(|| key.to_string())
    }

    /// Look a key up, reporting a miss instead of echoing the key back.
    ///
    /// For callers that have a better fallback than the key itself — a theme
    /// named after a place or a product, say, which has no translation and
    /// should be shown as its author wrote it.
    pub fn try_t(&self, key: &str) -> Option<String> {
        self.0.translations().get(key).cloned().or_else(|| {
            if self.0 == Language::En {
                None
            } else {
                Language::En.translations().get(key).cloned()
            }
        })
    }

    /// Look up a translation key with interpolation.
    ///
    /// Replaces `{variable}` placeholders with the supplied values.
    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.t(key);
        for (k, v) in args {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }

    /// Look up a pluralised key.
    ///
    /// Appends the language's plural suffix for `count` (see
    /// [`Language::plural_suffix`]) to the key.
    pub fn t_plural(&self, key: &str, count: usize) -> String {
        self.t_args(
            &self.plural_key(key, count),
            &[("count", &count.to_string())],
        )
    }

    /// The key of the plural form of `key` for `count`, for a caller that
    /// interpolates more than the count.
    pub fn plural_key(&self, key: &str, count: usize) -> String {
        format!("{key}{}", self.0.plural_suffix(count))
    }
}

/// Hook: obtain the [`I18n`] helper for the current language.
///
/// Must be called inside a component whose ancestor called [`use_init_language`].
///
/// The date style is the one of the tree held by the
/// [tree cache](crate::components::tree_cache), the default outside a tree.
pub fn use_i18n() -> I18n {
    let lang: Signal<Language> = use_context();
    let dates = try_use_context::<Signal<DateStyle>>().map_or(DateStyle::DEFAULT, |style| style());
    I18n(lang(), dates)
}

/// Hook: initialise the language context (call once in `AppShell`).
///
/// On first use — no persisted choice yet — the language follows the
/// languages configured in the browser or OS, which the webview reports
/// most-preferred first. English is used when none of them is translated, and
/// when detection yields nothing at all. Provides a `Signal<Language>` in the
/// Dioxus context.
pub fn use_init_language() -> Signal<Language> {
    let mut lang = use_context_provider(|| Signal::new(Language::En));

    // On mount: read persisted language or detect browser/system language.
    use_effect(move || {
        spawn(async move {
            // One ordered list: the explicit choice (if any) first, then what
            // the platform reports. An unreadable stored value therefore falls
            // through to detection instead of pinning English. Each accessor is
            // guarded — storage can be blocked, and `navigator.languages` is
            // missing on some embedded webviews — so a failure just leaves the
            // list shorter rather than aborting detection.
            let result = document::eval(
                r#"
                const prefs = [];
                try {
                    const stored = localStorage.getItem('oxidgene-lang');
                    if (stored) prefs.push(stored);
                } catch (e) {}
                try {
                    if (navigator.languages && navigator.languages.length) {
                        prefs.push(...navigator.languages);
                    } else if (navigator.language || navigator.userLanguage) {
                        prefs.push(navigator.language || navigator.userLanguage);
                    }
                } catch (e) {}
                return prefs;
                "#,
            );
            if let Ok(val) = result.await {
                let prefs: Vec<&str> = val
                    .as_array()
                    .map(|items| items.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                lang.set(Language::from_preferences(prefs));
            }
        });
    });

    lang
}

/// Persist the language choice to localStorage and update the signal.
pub fn set_language(mut lang: Signal<Language>, new_lang: Language) {
    lang.set(new_lang);
    let code = new_lang.code();
    document::eval(&format!("localStorage.setItem('oxidgene-lang', '{code}');"));
}

#[cfg(test)]
mod language_detection_tests {
    use super::Language;

    #[test]
    fn matches_on_the_primary_subtag_only() {
        assert_eq!(Language::try_from_code("fr"), Some(Language::Fr));
        assert_eq!(Language::try_from_code("fr-FR"), Some(Language::Fr));
        assert_eq!(Language::try_from_code("fr_CA"), Some(Language::Fr));
        assert_eq!(Language::try_from_code("EN-gb"), Some(Language::En));
    }

    #[test]
    fn reports_untranslated_languages_as_unsupported() {
        assert_eq!(Language::try_from_code("sv"), None);
        assert_eq!(Language::try_from_code("frr"), None); // North Frisian, not French
        assert_eq!(Language::try_from_code(""), None);
    }

    #[test]
    fn picks_the_first_translated_entry_not_the_first_entry() {
        assert_eq!(
            Language::from_preferences(["sv-SE", "fr-FR", "en"]),
            Language::Fr
        );
        assert_eq!(Language::from_preferences(["pl-PL", "en"]), Language::Pl);
    }

    #[test]
    fn falls_back_to_english_without_a_usable_preference() {
        assert_eq!(Language::from_preferences(["sv", "ja"]), Language::En);
        assert_eq!(Language::from_preferences([]), Language::En);
    }

    #[test]
    fn an_explicit_choice_leading_the_list_wins_over_the_os() {
        assert_eq!(Language::from_preferences(["en", "fr-FR"]), Language::En);
        // A corrupted stored value defers to the OS rather than pinning English.
        assert_eq!(Language::from_preferences(["xx", "fr-FR"]), Language::Fr);
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;

    /// Polish plurals have three forms: each `_one`/`_other` pair of the
    /// other languages also has a `_few` and a `_many` form there. A lone
    /// `_one` key is chosen by the code, not by a count.
    fn polish_plural_forms() -> std::collections::HashSet<String> {
        let en = en::translations();
        en.keys()
            .filter_map(|key| key.strip_suffix("_one"))
            .filter(|stem| en.contains_key(&format!("{stem}_other")))
            .flat_map(|stem| [format!("{stem}_few"), format!("{stem}_many")])
            .collect()
    }

    /// Every key must exist in every table.
    ///
    /// A missing key does not fail to compile and does not fail to render — it
    /// renders as the key itself, in the middle of a sentence, only for users
    /// of the other language. Adding a screenful of strings to one file and
    /// forgetting the others is exactly how that happens.
    #[test]
    fn every_table_carries_the_same_keys() {
        let en = en::translations();
        let polish = polish_plural_forms();
        for language in Language::ALL {
            let table = language.translations();
            let missing: Vec<_> = en.keys().filter(|key| !table.contains_key(*key)).collect();
            let extra: Vec<_> = table
                .keys()
                .filter(|key| !en.contains_key(*key))
                .filter(|key| !(language == Language::Pl && polish.contains(*key)))
                .collect();
            assert!(missing.is_empty(), "{language:?} lacks {missing:?}");
            assert!(
                extra.is_empty(),
                "{language:?} has keys English lacks: {extra:?}"
            );
            if language == Language::Pl {
                let absent: Vec<_> = polish
                    .iter()
                    .filter(|key| !table.contains_key(*key))
                    .collect();
                assert!(
                    absent.is_empty(),
                    "Polish lacks the plural forms {absent:?}"
                );
            }
        }
    }

    /// Every key the code looks up by a literal must exist in English.
    ///
    /// The parity test above only compares the tables with each other; a key
    /// missing from all eight passes it, and renders as the key itself in
    /// every language. This reads the crate's sources for `t`, `t_args`,
    /// `t_plural` and `try_t` calls whose first argument is a string literal;
    /// keys built at run time are left to the code that builds them.
    #[test]
    fn every_literal_key_in_the_code_exists() {
        let en = en::translations();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        for path in rust_sources(&root) {
            let source = std::fs::read_to_string(&path).unwrap();
            for (call, key) in literal_keys(&source) {
                let present = match call {
                    "t_plural" => {
                        en.contains_key(&format!("{key}_one"))
                            && en.contains_key(&format!("{key}_other"))
                    }
                    _ => en.contains_key(key),
                };
                if !present {
                    missing.push(format!("{}: {key}", path.display()));
                }
            }
        }
        assert!(missing.is_empty(), "keys missing from en.rs: {missing:#?}");
    }

    /// Key families the code builds at run time in a way the scan below
    /// cannot read from a single token, with where.
    const RUNTIME_FAMILIES: &[&str] = &[
        // `concat!("name_type.", $label)` in utils.rs's `name_types!` table.
        "name_type.{}",
        // `concat!("event.type.", $label)` in utils.rs's `event_types!` table.
        "event.type.{}",
    ];

    /// Every key of the tables is used somewhere.
    ///
    /// Drift it prevents: a screen removed or reworded leaves its strings
    /// behind in eight tables, translated and maintained for nothing. A key
    /// counts as used when the crate's sources (the tables aside) hold it as a
    /// token — its plural stem for a `_one`/`_other` form — or when a format
    /// string builds it: `"tools.tab.{}"` covers every
    /// `tools.tab.<word>`. Fixing a failure: delete the key from all eight
    /// tables; a family built some other way goes in `RUNTIME_FAMILIES`.
    #[test]
    fn every_key_is_used() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let tables = root.join("i18n");
        let mut literals = std::collections::HashSet::new();
        let mut families: Vec<String> = RUNTIME_FAMILIES.iter().map(|f| f.to_string()).collect();
        for path in rust_sources(&root) {
            if path.parent() == Some(tables.as_path()) && !path.ends_with("mod.rs") {
                continue;
            }
            let full = std::fs::read_to_string(&path).unwrap();
            // Production code only: a key a test mentions is not used.
            let source = full.split("#[cfg(test)]").next().unwrap_or_default();
            // Tokens rather than string literals: a key quoted inside an
            // `rsx!` string (`"{i18n.t(\"…\")}"`) is still a key.
            let tokens = source.split(|c: char| {
                !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '{' | '}'))
            });
            for literal in tokens.filter(|token| token.contains('.')) {
                if literal.contains('{') {
                    let stem = &literal[..literal.find('{').unwrap()];
                    if stem.contains('.')
                        && stem.chars().all(|c| {
                            c.is_ascii_lowercase() || c.is_ascii_digit() || "._".contains(c)
                        })
                    {
                        families.push(literal.to_string());
                    }
                } else {
                    literals.insert(literal.to_string());
                }
            }
        }
        let stem = |key: &str| {
            ["_one", "_other", "_few", "_many"]
                .iter()
                .find_map(|suffix| key.strip_suffix(suffix))
                .map(str::to_string)
        };
        let mut unused: Vec<_> = en::translations()
            .keys()
            .filter(|key| {
                let stem = stem(key);
                let candidates = std::iter::once(key.as_str()).chain(stem.as_deref());
                !candidates.clone().any(|k| literals.contains(k))
                    && !families
                        .iter()
                        .any(|family| candidates.clone().any(|k| family_matches(family, k)))
            })
            .cloned()
            .collect();
        unused.sort();
        assert!(unused.is_empty(), "keys no code uses: {unused:#?}");
    }

    /// Whether `key` is one of the keys format string `family` builds: each
    /// `{…}` stands for one or more of `[a-z0-9_]`.
    fn family_matches(family: &str, key: &str) -> bool {
        let mut parts = Vec::new();
        let mut rest = family;
        while let Some(open) = rest.find('{') {
            parts.push(&rest[..open]);
            let Some(close) = rest[open..].find('}') else {
                return false;
            };
            rest = &rest[open + close + 1..];
        }
        parts.push(rest);
        matches_parts(&parts, key)
    }

    fn matches_parts(parts: &[&str], key: &str) -> bool {
        let Some((first, others)) = parts.split_first() else {
            return key.is_empty();
        };
        let Some(after) = key.strip_prefix(first) else {
            return false;
        };
        if others.is_empty() {
            return after.is_empty();
        }
        // A placeholder: one or more word characters, then the next part.
        let word = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        (1..=word).any(|n| matches_parts(others, &after[n..]))
    }

    #[test]
    fn a_family_covers_the_keys_it_builds() {
        assert!(family_matches("tools.tab.{}", "tools.tab.anomalies"));
        assert!(family_matches(
            "tools.{key}.intro",
            "tools.duplicates.intro"
        ));
        assert!(family_matches(
            "kinship.rel.{key}_{}",
            "kinship.rel.cousin_2"
        ));
        assert!(!family_matches("tools.tab.{}", "tools.tab."));
        assert!(!family_matches("tools.tab.{}", "tools.title"));
    }

    /// Every `.rs` file under `dir`.
    fn rust_sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
        files
    }

    /// The `(call, key)` of every lookup in `source` whose key is a literal.
    fn literal_keys(source: &str) -> Vec<(&'static str, &str)> {
        const CALLS: [&str; 4] = ["t_plural", "t_args", "try_t", "t"];
        let mut keys = Vec::new();
        for (dot, _) in source.match_indices('.') {
            let rest = &source[dot + 1..];
            let Some(call) = CALLS.iter().find(|call| {
                rest.strip_prefix(**call)
                    .is_some_and(|after| after.starts_with('('))
            }) else {
                continue;
            };
            let argument = rest[call.len() + 1..].trim_start();
            let Some(literal) = argument.strip_prefix('"') else {
                continue;
            };
            let Some(end) = literal.find('"') else {
                continue;
            };
            let key = &literal[..end];
            // Only what looks like a key: `section.name`, no format braces.
            if key.contains('.') && !key.contains(['{', ' ']) {
                keys.push((*call, key));
            }
        }
        keys
    }

    /// A `{placeholder}` in one language must exist in every other.
    ///
    /// `t_args` substitutes by name and leaves anything it was not given
    /// alone, so a translation that renamed `{count}` to `{nombre}` shows the
    /// literal braces to the user rather than a number.
    #[test]
    fn matching_keys_interpolate_the_same_names() {
        let en = en::translations();
        for language in Language::ALL {
            for (key, text) in language.translations() {
                // Polish `_few`/`_many` forms follow their `_other` sibling.
                let reference = en.get(key).or_else(|| {
                    let stem = key
                        .strip_suffix("_few")
                        .or_else(|| key.strip_suffix("_many"))?;
                    en.get(&format!("{stem}_other"))
                });
                let Some(english) = reference else { continue };
                let (english, translated) = (placeholders(english), placeholders(text));
                let mismatched: Vec<_> = english.symmetric_difference(&translated).collect();
                assert!(
                    mismatched.is_empty(),
                    "{language:?} {key} interpolates different names: {mismatched:?}"
                );
            }
        }
    }

    #[test]
    fn polish_counts_take_their_three_forms() {
        let forms: Vec<_> = [1, 2, 4, 5, 12, 14, 21, 22, 25, 112]
            .into_iter()
            .map(|n| Language::Pl.plural_suffix(n))
            .collect();
        assert_eq!(
            forms,
            [
                "_one", "_few", "_few", "_many", "_many", "_many", "_many", "_few", "_many",
                "_many"
            ]
        );
        assert_eq!(Language::Fr.plural_suffix(0), "_one");
        assert_eq!(Language::En.plural_suffix(0), "_other");
    }

    /// The search results counted "1 results": the count is a plural.
    #[test]
    fn a_single_search_result_is_counted_in_the_singular() {
        assert_eq!(
            I18n::new(Language::En).t_plural("search.results_count", 1),
            "1 result"
        );
        assert_eq!(
            I18n::new(Language::En).t_plural("search.results_count", 5),
            "5 results"
        );
        assert_eq!(
            I18n::new(Language::Pl).t_plural("search.results_count", 3),
            "3 wyniki"
        );
        assert_eq!(
            I18n::new(Language::Pl).t_plural("search.results_count", 5),
            "5 wyników"
        );
    }

    fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
        let mut found = std::collections::BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            match after.find('}') {
                Some(end) => {
                    found.insert(after[..end].to_string());
                    rest = &after[end + 1..];
                }
                None => break,
            }
        }
        found
    }
}
