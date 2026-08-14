//! Internationalization (i18n) module.
//!
//! Provides runtime language switching with English and French translations.
//! Uses a Dioxus context signal for reactive updates across all components.

mod en;
mod fr;

use std::collections::HashMap;

use dioxus::prelude::*;

/// Supported languages.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    En,
    Fr,
}

impl Language {
    /// BCP-47 language code.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Fr => "fr",
        }
    }

    /// Native display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::En => "EN",
            Self::Fr => "FR",
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
        match primary.to_ascii_lowercase().as_str() {
            "en" => Some(Self::En),
            "fr" => Some(Self::Fr),
            _ => None,
        }
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
        }
    }
}

/// Translation helper returned by [`use_i18n`].
///
/// Holds the current language and provides lookup methods.
/// Because it reads from a reactive signal, any component using it
/// will re-render when the language changes.
#[derive(Clone, Copy, PartialEq)]
pub struct I18n(pub Language);

impl I18n {
    /// Look up a translation key. Falls back to English, then to the key itself.
    pub fn t(&self, key: &str) -> String {
        self.0
            .translations()
            .get(key)
            .cloned()
            .or_else(|| {
                if self.0 != Language::En {
                    Language::En.translations().get(key).cloned()
                } else {
                    None
                }
            })
            .unwrap_or_else(|| key.to_string())
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
    /// Appends `_one` (count ≤ 1) or `_other` (count > 1) to the key.
    pub fn t_plural(&self, key: &str, count: usize) -> String {
        let suffix = match self.0 {
            // French: 0 and 1 are singular
            Language::Fr => {
                if count <= 1 {
                    "_one"
                } else {
                    "_other"
                }
            }
            // English: only 1 is singular
            Language::En => {
                if count == 1 {
                    "_one"
                } else {
                    "_other"
                }
            }
        };
        self.t_args(&format!("{key}{suffix}"), &[("count", &count.to_string())])
    }
}

/// Hook: obtain the [`I18n`] helper for the current language.
///
/// Must be called inside a component whose ancestor called [`use_init_language`].
pub fn use_i18n() -> I18n {
    let lang: Signal<Language> = use_context();
    I18n(lang())
}

/// Hook: initialise the language context (call once in `Layout` or `App`).
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
        assert_eq!(Language::try_from_code("de"), None);
        assert_eq!(Language::try_from_code("frr"), None); // North Frisian, not French
        assert_eq!(Language::try_from_code(""), None);
    }

    #[test]
    fn picks_the_first_translated_entry_not_the_first_entry() {
        assert_eq!(
            Language::from_preferences(["de-DE", "fr-FR", "en"]),
            Language::Fr
        );
    }

    #[test]
    fn falls_back_to_english_without_a_usable_preference() {
        assert_eq!(Language::from_preferences(["de", "es"]), Language::En);
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

    /// Every key must exist in both tables.
    ///
    /// A missing key does not fail to compile and does not fail to render — it
    /// renders as the key itself, in the middle of a sentence, only for users
    /// of the other language. Adding a screenful of strings to one file and
    /// forgetting the other is exactly how that happens.
    #[test]
    fn the_two_tables_carry_the_same_keys() {
        let en = en::translations();
        let fr = fr::translations();

        let missing_from_fr: Vec<_> = en.keys().filter(|key| !fr.contains_key(*key)).collect();
        let missing_from_en: Vec<_> = fr.keys().filter(|key| !en.contains_key(*key)).collect();

        assert!(
            missing_from_fr.is_empty(),
            "keys present in English but not French: {missing_from_fr:?}"
        );
        assert!(
            missing_from_en.is_empty(),
            "keys present in French but not English: {missing_from_en:?}"
        );
    }

    /// A `{placeholder}` in one language must exist in the other.
    ///
    /// `t_args` substitutes by name and leaves anything it was not given
    /// alone, so a translation that renamed `{count}` to `{nombre}` shows the
    /// literal braces to the user rather than a number.
    #[test]
    fn matching_keys_interpolate_the_same_names() {
        let en = en::translations();
        let fr = fr::translations();

        for (key, english) in en {
            let Some(french) = fr.get(key) else { continue };
            let (english, french) = (placeholders(english), placeholders(french));
            let mismatched: Vec<_> = english.symmetric_difference(&french).collect();
            assert!(
                mismatched.is_empty(),
                "{key} interpolates different names in each language: {mismatched:?}"
            );
        }
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
