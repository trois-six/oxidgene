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

    /// Parse from a BCP-47 code or prefix (e.g. "fr-FR" → Fr).
    pub fn from_code(s: &str) -> Self {
        if s.starts_with("fr") {
            Self::Fr
        } else {
            Self::En
        }
    }

    fn translations(self) -> &'static HashMap<String, String> {
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
#[derive(Clone, Copy)]
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
/// Reads the persisted language from `localStorage` (web) or defaults to
/// English. Provides a `Signal<Language>` in the Dioxus context.
pub fn use_init_language() -> Signal<Language> {
    let mut lang = use_context_provider(|| Signal::new(Language::En));

    // On mount: read persisted language or detect browser/system language.
    use_effect(move || {
        spawn(async move {
            let result = document::eval(
                r#"
                let stored = localStorage.getItem('oxidgene-lang');
                if (stored) return stored;
                return (navigator.language || navigator.userLanguage || 'en').substring(0, 2);
                "#,
            );
            if let Ok(val) = result.await
                && let Some(code) = val.as_str()
            {
                lang.set(Language::from_code(code));
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
