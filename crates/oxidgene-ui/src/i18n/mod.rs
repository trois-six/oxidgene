//! JSON-driven runtime internationalization.

use std::sync::Arc;

pub(crate) mod locale;
use locale::replace_custom_languages;
pub use locale::{CustomLanguage, Language, is_valid_custom_language_code, parse_custom_language};

use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateDisplayFormat};
use oxidgene_core::types::Tree;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CustomLanguages {
    pub languages: Vec<CustomLanguage>,
    pub errors: Vec<CustomLanguageError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomLanguageError {
    pub file: String,
    pub message: String,
}

pub trait CustomLanguageSource: Send + Sync {
    fn location(&self) -> String;
    fn load(&self) -> CustomLanguages;
    fn load_code(&self, code: &str) -> Result<Option<CustomLanguage>, CustomLanguageError>;
}

#[derive(Clone)]
pub struct CustomLanguageLoader(Arc<dyn CustomLanguageSource>);

impl CustomLanguageLoader {
    #[must_use]
    pub fn new(source: Arc<dyn CustomLanguageSource>) -> Self {
        Self(source)
    }

    #[must_use]
    pub fn location(&self) -> String {
        self.0.location()
    }

    #[must_use]
    pub fn load(&self) -> CustomLanguages {
        self.0.load()
    }

    pub fn load_code(&self, code: &str) -> Result<Option<CustomLanguage>, CustomLanguageError> {
        self.0.load_code(code)
    }
}

impl PartialEq for CustomLanguageLoader {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for CustomLanguageLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CustomLanguageLoader")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageCatalog {
    pub languages: Vec<Language>,
    pub location: Option<String>,
    pub errors: Vec<CustomLanguageError>,
    pub revision: u64,
}

impl Default for LanguageCatalog {
    fn default() -> Self {
        Self {
            languages: Language::builtins(),
            location: None,
            errors: Vec::new(),
            revision: 0,
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
        self.0.translation(key)
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
    if let Some(catalog) = try_use_context::<Signal<LanguageCatalog>>() {
        let _ = catalog.read().revision;
    }
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
    let loader = try_use_context::<CustomLanguageLoader>();
    let mut lang = use_context_provider(|| Signal::new(Language::english()));
    let mut catalog = use_context_provider(|| Signal::new(LanguageCatalog::default()));

    // On mount: read persisted language or detect browser/system language.
    use_effect(move || {
        let loader = loader.clone();
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
                let preferences: Vec<String> = val
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let stored = preferences.first().map(String::as_str).unwrap_or_default();
                if let Some(code) = stored.strip_prefix("custom:") {
                    match loader.as_ref().map(|loader| loader.load_code(code)) {
                        Some(Ok(Some(custom))) if custom.code == code => {
                            let options = replace_custom_languages(vec![custom]);
                            let selected = options.first().copied().unwrap_or(Language::english());
                            let mut languages = Language::builtins();
                            languages.extend(options);
                            catalog.set(LanguageCatalog {
                                languages,
                                location: loader.as_ref().map(CustomLanguageLoader::location),
                                errors: Vec::new(),
                                revision: 1,
                            });
                            lang.set(selected);
                        }
                        Some(Err(error)) => {
                            replace_custom_languages(Vec::new());
                            let mut next = LanguageCatalog {
                                location: loader.as_ref().map(CustomLanguageLoader::location),
                                ..LanguageCatalog::default()
                            };
                            next.errors.push(error);
                            catalog.set(next);
                            lang.set(Language::english());
                            persist_language(Language::english());
                        }
                        _ => {
                            replace_custom_languages(Vec::new());
                            lang.set(Language::english());
                            persist_language(Language::english());
                        }
                    }
                } else {
                    replace_custom_languages(Vec::new());
                    lang.set(Language::from_preferences(
                        preferences.iter().map(String::as_str),
                    ));
                }
            }
        });
    });

    lang
}

/// Persist the language choice to localStorage and update the signal.
pub fn set_language(mut lang: Signal<Language>, new_lang: Language) {
    lang.set(new_lang);
    persist_language(new_lang);
}

fn persist_language(language: Language) {
    let id = language.id();
    document::eval(&format!("localStorage.setItem('oxidgene-lang', '{id}');"));
}

pub fn reload_custom_languages(
    mut language: Signal<Language>,
    mut catalog: Signal<LanguageCatalog>,
    loader: &CustomLanguageLoader,
) {
    let previous_id = language.peek().id();
    let loaded = loader.load();
    let custom = replace_custom_languages(loaded.languages);
    let mut languages = Language::builtins();
    languages.extend(custom);
    let revision = catalog.peek().revision.wrapping_add(1);
    catalog.set(LanguageCatalog {
        languages,
        location: Some(loader.location()),
        errors: loaded.errors,
        revision,
    });

    let selected = Language::from_storage_id(&previous_id).unwrap_or(Language::english());
    language.set(selected);
    if selected.id() != previous_id {
        persist_language(selected);
    }
}

#[cfg(test)]
mod language_detection_tests {
    use super::Language;

    #[test]
    fn matches_on_the_primary_subtag_only() {
        assert_eq!(
            Language::try_from_code("fr"),
            Some(Language::try_from_code("fr").unwrap())
        );
        assert_eq!(
            Language::try_from_code("fr-FR"),
            Some(Language::try_from_code("fr").unwrap())
        );
        assert_eq!(
            Language::try_from_code("fr_CA"),
            Some(Language::try_from_code("fr").unwrap())
        );
        assert_eq!(Language::try_from_code("EN-gb"), Some(Language::english()));
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
            Language::try_from_code("fr").unwrap()
        );
        assert_eq!(
            Language::from_preferences(["pl-PL", "en"]),
            Language::try_from_code("pl").unwrap()
        );
    }

    #[test]
    fn falls_back_to_english_without_a_usable_preference() {
        assert_eq!(
            Language::from_preferences(["sv", "ja"]),
            Language::english()
        );
        assert_eq!(Language::from_preferences([]), Language::english());
    }

    #[test]
    fn an_explicit_choice_leading_the_list_wins_over_the_os() {
        assert_eq!(
            Language::from_preferences(["en", "fr-FR"]),
            Language::english()
        );
        // A corrupted stored value defers to the OS rather than pinning English.
        assert_eq!(
            Language::from_preferences(["xx", "fr-FR"]),
            Language::try_from_code("fr").unwrap()
        );
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;

    fn additional_plural_forms(language: Language) -> std::collections::HashSet<String> {
        let en = Language::english().translations();
        let locale = language.locale();
        let suffixes: std::collections::HashSet<_> =
            std::iter::once(locale.plurals.default.as_str())
                .chain(locale.plurals.rules.iter().map(|rule| rule.suffix.as_str()))
                .filter(|suffix| !["_one", "_other"].contains(suffix))
                .collect();
        en.keys()
            .filter_map(|key| key.strip_suffix("_one"))
            .filter(|stem| en.contains_key(&format!("{stem}_other")))
            .flat_map(|stem| suffixes.iter().map(move |suffix| format!("{stem}{suffix}")))
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
        let en = Language::english().translations();
        for language in Language::builtins() {
            let additional = additional_plural_forms(language);
            let table = language.translations();
            let missing: Vec<_> = en.keys().filter(|key| !table.contains_key(*key)).collect();
            let extra: Vec<_> = table
                .keys()
                .filter(|key| !en.contains_key(*key))
                .filter(|key| !additional.contains(*key))
                .collect();
            assert!(missing.is_empty(), "{language:?} lacks {missing:?}");
            assert!(
                extra.is_empty(),
                "{language:?} has keys English lacks: {extra:?}"
            );
            {
                let absent: Vec<_> = additional
                    .iter()
                    .filter(|key| !table.contains_key(*key))
                    .collect();
                assert!(
                    absent.is_empty(),
                    "{language:?} lacks the declared plural forms {absent:?}"
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
        let en = Language::english().translations();
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
        assert!(
            missing.is_empty(),
            "keys missing from assets/i18n/en.json: {missing:#?}"
        );
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
            ["_zero", "_one", "_two", "_other", "_few", "_many"]
                .iter()
                .find_map(|suffix| key.strip_suffix(suffix))
                .map(str::to_string)
        };
        let mut unused: Vec<_> = Language::english()
            .translations()
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
        let en = Language::english().translations();
        for language in Language::builtins() {
            for (key, text) in &language.translations() {
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
            .map(|n| Language::try_from_code("pl").unwrap().plural_suffix(n))
            .collect();
        assert_eq!(
            forms,
            [
                "_one", "_few", "_few", "_many", "_many", "_many", "_many", "_few", "_many",
                "_many"
            ]
        );
        assert_eq!(
            Language::try_from_code("fr").unwrap().plural_suffix(0),
            "_one"
        );
        assert_eq!(Language::english().plural_suffix(0), "_other");
    }

    /// The search results counted "1 results": the count is a plural.
    #[test]
    fn a_single_search_result_is_counted_in_the_singular() {
        assert_eq!(
            I18n::new(Language::english()).t_plural("search.results_count", 1),
            "1 result"
        );
        assert_eq!(
            I18n::new(Language::english()).t_plural("search.results_count", 5),
            "5 results"
        );
        assert_eq!(
            I18n::new(Language::try_from_code("pl").unwrap()).t_plural("search.results_count", 3),
            "3 wyniki"
        );
        assert_eq!(
            I18n::new(Language::try_from_code("pl").unwrap()).t_plural("search.results_count", 5),
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
