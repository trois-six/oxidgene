use std::sync::{Arc, LazyLock, RwLock};
use std::{borrow::Cow, collections::HashMap};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomLanguage {
    pub code: String,
    pub name: String,
    pub flag: String,
    #[serde(default = "english_code")]
    pub reference_language: String,
    pub plurals: Plurals,
    pub dates: Dates,
    pub translations: HashMap<String, String>,
}

fn english_code() -> String {
    "en".to_owned()
}

impl CustomLanguage {
    pub fn code(&self) -> &str {
        &self.code
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dates {
    pub months: [String; 12],
    pub months_with_day: [String; 12],
    pub long: [String; 3],
    pub short: [String; 3],
    pub display: [String; 3],
    pub numeric: [String; 3],
    pub years: Vec<String>,
    pub days: Vec<String>,
    pub reading: HashMap<String, ReadingPiece>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ReadingPiece {
    Add { value: u32 },
    Hundred,
    Thousand,
    And,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plurals {
    pub rules: Vec<PluralRule>,
    pub default: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluralRule {
    pub suffix: String,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(default)]
    pub modulo: Option<usize>,
    #[serde(default)]
    pub min: Option<usize>,
    #[serde(default)]
    pub max: Option<usize>,
    #[serde(default)]
    pub exclude: Vec<[usize; 2]>,
}

impl Condition {
    fn matches(&self, count: usize) -> bool {
        let value = self.modulo.map_or(count, |modulo| count % modulo);
        self.min.is_none_or(|min| value >= min)
            && self.max.is_none_or(|max| value <= max)
            && !self
                .exclude
                .iter()
                .any(|[min, max]| (*min..=*max).contains(&value))
    }
}

struct Entry {
    code: &'static str,
    locale: Arc<CustomLanguage>,
    builtin: bool,
    available: bool,
}

static REGISTRY: LazyLock<RwLock<Vec<Entry>>> = LazyLock::new(|| {
    let mut entries = Vec::new();
    for source in embedded_documents() {
        let locale = parse_locale(&source).expect("valid embedded locale");
        assert!(
            !entries
                .iter()
                .any(|entry: &Entry| entry.code == locale.code),
            "duplicate embedded locale"
        );
        entries.push(Entry {
            code: Box::leak(locale.code.clone().into_boxed_str()),
            locale: Arc::new(locale),
            builtin: true,
            available: true,
        });
    }
    assert!(
        entries.iter().any(|entry| entry.code == "en"),
        "English fallback locale"
    );
    RwLock::new(entries)
});

#[cfg(all(feature = "compressed-locales", not(target_arch = "wasm32")))]
fn embedded_documents() -> Vec<Cow<'static, str>> {
    use std::io::Read;
    let documents: &[&[u8]] = include!(concat!(env!("OUT_DIR"), "/locales.rs"));
    documents
        .iter()
        .map(|compressed| {
            let mut source = String::new();
            brotli_decompressor::Decompressor::new(*compressed, 1 << 16)
                .read_to_string(&mut source)
                .expect("valid embedded Brotli locale");
            Cow::Owned(source)
        })
        .collect()
}

#[cfg(not(all(feature = "compressed-locales", not(target_arch = "wasm32"))))]
fn embedded_documents() -> Vec<Cow<'static, str>> {
    let documents: &[&str] = include!(concat!(env!("OUT_DIR"), "/locales.rs"));
    documents
        .iter()
        .map(|source| Cow::Borrowed(*source))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language(usize);

impl Language {
    pub fn english() -> Self {
        Self::from_storage_id("en").expect("English fallback")
    }

    pub fn builtins() -> Vec<Self> {
        REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.builtin)
            .map(|(index, _)| Self(index))
            .collect()
    }

    pub fn available() -> Vec<Self> {
        REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.available)
            .map(|(index, _)| Self(index))
            .collect()
    }

    pub(crate) fn locale(self) -> Arc<CustomLanguage> {
        REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[self.0]
            .locale
            .clone()
    }

    pub fn code(self) -> &'static str {
        REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)[self.0]
            .code
    }

    pub fn reference_code(self) -> &'static str {
        let code = self.locale().reference_language.clone();
        Self::from_storage_id(&code)
            .unwrap_or_else(Self::english)
            .code()
    }

    pub fn id(self) -> String {
        let registry = REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = &registry[self.0];
        if entry.builtin {
            entry.code.to_owned()
        } else {
            format!("custom:{}", entry.code)
        }
    }

    pub fn label(self) -> String {
        self.code().to_ascii_uppercase()
    }
    pub fn native_name(self) -> String {
        self.locale().name.clone()
    }
    pub fn flag(self) -> String {
        self.locale().flag.clone()
    }

    pub fn plural_suffix(self, count: usize) -> String {
        let locale = self.locale();
        locale
            .plurals
            .rules
            .iter()
            .find(|rule| {
                rule.conditions
                    .iter()
                    .all(|condition| condition.matches(count))
            })
            .map_or_else(
                || locale.plurals.default.clone(),
                |rule| rule.suffix.clone(),
            )
    }

    pub fn try_from_code(code: &str) -> Option<Self> {
        let code = code.replace('_', "-").to_ascii_lowercase();
        let mut candidate = code.as_str();
        loop {
            if let Some(language) = Self::from_storage_id(candidate) {
                return Some(language);
            }
            candidate = candidate.rsplit_once('-')?.0;
        }
    }

    pub fn from_preferences<'a>(codes: impl IntoIterator<Item = &'a str>) -> Self {
        codes
            .into_iter()
            .find_map(Self::try_from_code)
            .unwrap_or_else(Self::english)
    }

    pub(super) fn from_storage_id(id: &str) -> Option<Self> {
        let code = id.strip_prefix("custom:").unwrap_or(id);
        REGISTRY
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .position(|entry| entry.available && entry.code == code)
            .map(Self)
    }

    pub(super) fn translation(self, key: &str) -> Option<String> {
        let locale = self.locale();
        let english = Self::english().locale();
        locale
            .translations
            .get(key)
            .or_else(|| english.translations.get(key))
            .or_else(|| {
                ["_zero", "_two", "_few", "_many"]
                    .iter()
                    .find_map(|suffix| {
                        let other = format!("{}_other", key.strip_suffix(suffix)?);
                        locale
                            .translations
                            .get(&other)
                            .or_else(|| english.translations.get(&other))
                    })
            })
            .cloned()
    }

    #[cfg(test)]
    pub(crate) fn translations(self) -> HashMap<String, String> {
        self.locale().translations.clone()
    }
}

pub fn is_valid_custom_language_code(code: &str) -> bool {
    (2..=35).contains(&code.len())
        && code.split('-').all(|part| {
            !part.is_empty()
                && part.len() <= 8
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
        && code
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
}

fn parse_locale(source: &str) -> Result<CustomLanguage, String> {
    let locale: CustomLanguage = serde_json::from_str(source).map_err(|error| error.to_string())?;
    if !is_valid_custom_language_code(&locale.code) || locale.name.trim().is_empty() {
        return Err("invalid locale code or empty name".to_owned());
    }
    if locale.dates.years.len() != 4000
        || locale.dates.days.len() != 32
        || locale.dates.years[1..]
            .iter()
            .chain(&locale.dates.days[1..])
            .any(|word| word.trim().is_empty())
        || locale
            .dates
            .months
            .iter()
            .chain(&locale.dates.months_with_day)
            .any(|word| word.trim().is_empty())
    {
        return Err(
            "dates require 12 month names, 32 day forms and 4000 year forms (index zero is unused)"
                .to_owned(),
        );
    }
    validate_date_templates(&locale.dates)?;
    for rule in &locale.plurals.rules {
        if rule.conditions.iter().any(|condition| {
            condition.modulo == Some(0)
                || condition
                    .min
                    .zip(condition.max)
                    .is_some_and(|(min, max)| min > max)
                || condition.exclude.iter().any(|[min, max]| min > max)
        }) {
            return Err("invalid plural condition".to_owned());
        }
    }
    if std::iter::once(&locale.plurals.default)
        .chain(locale.plurals.rules.iter().map(|rule| &rule.suffix))
        .any(|suffix| {
            !["_zero", "_one", "_two", "_few", "_many", "_other"].contains(&suffix.as_str())
        })
    {
        return Err("unknown plural category".to_owned());
    }
    Ok(locale)
}

fn validate_date_templates(dates: &Dates) -> Result<(), String> {
    for templates in [&dates.long, &dates.short, &dates.display, &dates.numeric] {
        for (index, template) in templates.iter().enumerate() {
            let allowed = [
                &["year"][..],
                &["year", "month"][..],
                &["year", "month", "day"][..],
            ][index];
            let placeholders = placeholders(template);
            if placeholders.len() != allowed.len()
                || allowed.iter().any(|name| !placeholders.contains(*name))
            {
                return Err(
                    "date templates must contain the corresponding year/month/day placeholders"
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

pub fn parse_custom_language(source: &str) -> Result<CustomLanguage, String> {
    let locale = parse_locale(source)?;
    if Language::builtins()
        .iter()
        .any(|language| language.code() == locale.code)
    {
        return Err("custom locale cannot replace an embedded locale".to_owned());
    }
    let english = Language::english().locale();
    for (key, text) in &locale.translations {
        let reference = english
            .translations
            .get(key)
            .or_else(|| {
                ["_zero", "_two", "_few", "_many"]
                    .iter()
                    .find_map(|suffix| {
                        english
                            .translations
                            .get(&format!("{}_other", key.strip_suffix(suffix)?))
                    })
            })
            .ok_or_else(|| format!("unknown translation key: {key}"))?;
        if placeholders(reference) != placeholders(text) {
            return Err(format!("different interpolation placeholders: {key}"));
        }
    }
    if !Language::builtins()
        .iter()
        .any(|language| language.code() == locale.reference_language)
    {
        return Err("unknown reference dictionary language".to_owned());
    }
    Ok(locale)
}

pub(super) fn replace_custom_languages(locales: Vec<CustomLanguage>) -> Vec<Language> {
    let mut registry = REGISTRY
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for entry in registry.iter_mut().filter(|entry| !entry.builtin) {
        entry.available = false;
    }
    let mut languages = Vec::new();
    for locale in locales {
        let index = if let Some(index) = registry.iter().position(|entry| entry.code == locale.code)
        {
            if registry[index].builtin {
                continue;
            }
            registry[index].locale = Arc::new(locale);
            registry[index].available = true;
            index
        } else {
            let index = registry.len();
            registry.push(Entry {
                code: Box::leak(locale.code.clone().into_boxed_str()),
                locale: Arc::new(locale),
                builtin: false,
                available: true,
            });
            index
        };
        if !languages.contains(&Language(index)) {
            languages.push(Language(index));
        }
    }
    drop(registry);
    crate::date_words::refresh_lexicon();
    languages
}

fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        found.insert(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    found
}

pub(crate) fn available_locales() -> Vec<Arc<CustomLanguage>> {
    REGISTRY
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|entry| entry.available)
        .map(|entry| entry.locale.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        date_words::{self, Form, Ymd},
        i18n::I18n,
    };

    #[cfg(all(feature = "compressed-locales", not(target_arch = "wasm32")))]
    #[test]
    fn compressed_documents_preserve_every_source_byte() {
        let documents: &[&[u8]] = include!(concat!(env!("OUT_DIR"), "/locales.rs"));
        let source_directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/i18n");
        let mut files: Vec<_> = std::fs::read_dir(source_directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|file| {
                file.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        files.sort();
        assert_eq!(files.len(), documents.len());
        for ((file, compressed), decoded) in files.iter().zip(documents).zip(embedded_documents()) {
            let source = std::fs::read_to_string(file).unwrap();
            assert_eq!(decoded, source);
            assert!(compressed.len() < source.len(), "{}", file.display());
        }
    }

    #[test]
    fn one_document_defines_a_new_locale_and_stable_handles() {
        let mut document: serde_json::Value =
            serde_json::from_str(include_str!("../../../../assets/i18n/en.json")).unwrap();
        document["code"] = "zz-test".into();
        document["name"] = "Example locale".into();
        document["reference_language"] = "en".into();
        document["translations"] = serde_json::json!({"common.save":"Example save"});
        document["dates"]["short"][2] = "{year} / {month} / {day}".into();
        document["dates"]["display"][2] = "{month} {day}, {year}".into();
        document["dates"]["numeric"][2] = "{year}-{month}-{day}".into();
        document["dates"]["months"][1] = "examplemonth".into();
        document["dates"]["months_with_day"][1] = "examplemonth".into();
        document["dates"]["reading"]["exampletwo"] = serde_json::json!({"kind":"add","value":2});
        document["plurals"]["rules"][0]["conditions"][0]["max"] = 2.into();
        let locale = parse_custom_language(&document.to_string()).unwrap();
        let language = replace_custom_languages(vec![locale.clone()])[0];
        assert_eq!(Language::try_from_code("zz-TEST"), Some(language));
        assert_eq!(language.code(), "zz-test");
        assert_eq!(language.reference_code(), "en");
        assert_eq!(language.plural_suffix(2), "_one");
        assert_eq!(I18n::new(language).t("common.save"), "Example save");
        assert_eq!(I18n::new(language).t("common.cancel"), "Cancel");
        let date = Ymd {
            year: 1650,
            month: Some(2),
            day: Some(2),
        };
        assert_eq!(
            date_words::written(language, date, Form::Short).unwrap(),
            "1650 / examplemonth / 2"
        );
        assert_eq!(
            date_words::read("exampletwo examplemonth 1650").unwrap(),
            date
        );
        use crate::components::date_input::format_date;
        use oxidgene_core::enums::{Calendar, DateDisplayFormat, DateQualifier};
        let i18n = I18n::new(language);
        assert_eq!(
            format_date(
                &i18n,
                Calendar::Gregorian,
                DateQualifier::Exact,
                Some("2 FEB 1650"),
                None
            ),
            "Feb 2, 1650"
        );
        let numeric = i18n.with_dates(crate::i18n::DateStyle {
            format: DateDisplayFormat::Numeric,
            ..crate::i18n::DateStyle::DEFAULT
        });
        assert_eq!(
            format_date(
                &numeric,
                Calendar::Gregorian,
                DateQualifier::Exact,
                Some("2 FEB 1650"),
                None
            ),
            "1650-02-02"
        );
        assert_eq!(replace_custom_languages(vec![locale])[0], language);
        replace_custom_languages(Vec::new());
        assert_eq!(Language::from_storage_id("custom:zz-test"), None);
        assert_eq!(
            Language::from_storage_id("custom:zz-test").unwrap_or_else(Language::english),
            Language::english()
        );
    }

    #[test]
    fn rejects_invalid_data_before_registering_it() {
        let source = include_str!("../../../../assets/i18n/en.json");
        let mut document: serde_json::Value = serde_json::from_str(source).unwrap();
        document["code"] = "zz-invalid".into();
        document["plurals"]["rules"][0]["conditions"][0]["modulo"] = 0.into();
        assert!(parse_custom_language(&document.to_string()).is_err());
        document["plurals"]["rules"][0]["conditions"][0]
            .as_object_mut()
            .unwrap()
            .remove("modulo");
        document["dates"]["short"][2] = "{year} {unknown}".into();
        assert!(parse_custom_language(&document.to_string()).is_err());
    }
}
