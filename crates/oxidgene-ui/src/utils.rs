//! Shared utility functions for parsing enums, formatting, and name resolution.

use std::collections::HashMap;

use uuid::Uuid;

use crate::i18n::I18n;

use oxidgene_core::{ChildType, Confidence, EventType, NameType, Privacy, Sex};

/// A signal following `value`, a prop: written when the props bring a new
/// one, so resources reading it re-run. The router reuses a page's instance
/// across navigations, which changes its props without remounting it.
pub fn use_synced<T: PartialEq + Clone + 'static>(value: T) -> dioxus::prelude::Signal<T> {
    use dioxus::prelude::*;
    let mut signal = use_signal(|| value.clone());
    if *signal.peek() != value {
        signal.set(value);
    }
    signal
}

pub async fn sleep_ms(milliseconds: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(milliseconds).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(milliseconds))).await;
}

// ── Enum parsers ────────────────────────────────────────────────────────

/// Parse a string value from a `<select>` into a [`Sex`] enum.
pub fn parse_sex(s: &str) -> Sex {
    match s {
        "Male" => Sex::Male,
        "Female" => Sex::Female,
        _ => Sex::Unknown,
    }
}

/// Declares the [`NameType`]s once, in the order the pickers list them: the
/// value its option carries and its label key. Parsing, labelling and the
/// pickers read this one table; the generated `match`es stay exhaustive,
/// so a new variant cannot be left out.
macro_rules! name_types {
    ($($variant:ident => $value:literal, $label:literal;)*) => {
        /// Every name type, in picker order.
        pub const NAME_TYPES: &[NameType] = &[$(NameType::$variant),*];

        /// Parse a string value from a `<select>` into a [`NameType`] enum.
        pub fn parse_name_type(s: &str) -> NameType {
            match s {
                $($value => NameType::$variant,)*
                _ => NameType::Other,
            }
        }

        /// The picker value that round-trips back to `name_type`, so a saved
        /// entry reopens on the type it was created with.
        pub fn name_type_value(nt: NameType) -> &'static str {
            match nt {
                $(NameType::$variant => $value,)*
            }
        }

        /// The i18n key labelling a name type in lists and read-only views.
        pub fn name_type_label_key(nt: NameType) -> &'static str {
            match nt {
                $(NameType::$variant => concat!("name_type.", $label),)*
            }
        }
    };
}

name_types! {
    Birth => "Birth", "birth";
    Married => "Married", "married";
    AlsoKnownAs => "AlsoKnownAs", "also_known_as";
    Maiden => "Maiden", "maiden";
    Religious => "Religious", "religious";
    GivenName => "Prenom", "prenom";
    Alias => "Alias", "alias";
    Byname => "Surnom", "surnom";
    Sobriquet => "Sobriquet", "sobriquet";
    Other => "Other", "other";
}

/// The i18n key labelling how a child is attached to their family.
pub fn child_type_label_key(ct: ChildType) -> &'static str {
    match ct {
        ChildType::Biological => "child_type.biological",
        ChildType::Adopted => "child_type.adopted",
        ChildType::Foster => "child_type.foster",
        ChildType::Step => "child_type.step",
        ChildType::Unknown => "child_type.unknown",
    }
}

/// Declares the [`EventType`]s once: the value of its picker option (the
/// variant's own name) and its label key. Parsing, labelling and the
/// pickers read this one table; the generated `match`es stay exhaustive,
/// so a new variant cannot be left out.
macro_rules! event_types {
    ($($variant:ident => $label:literal,)*) => {
        /// Every event type.
        pub const EVENT_TYPES: &[EventType] = &[$(EventType::$variant),*];

        /// i18n key naming an [`EventType`], for the badges and labels that
        /// show one.
        pub fn event_type_label_key(et: EventType) -> &'static str {
            match et {
                $(EventType::$variant => concat!("event.type.", $label),)*
            }
        }

        /// The value of an [`EventType`]'s picker option.
        pub fn event_type_value(et: EventType) -> &'static str {
            match et {
                $(EventType::$variant => stringify!($variant),)*
            }
        }

        /// Parse a string value from a `<select>` into an [`EventType`] enum.
        pub fn parse_event_type(s: &str) -> EventType {
            match s {
                $(stringify!($variant) => EventType::$variant,)*
                _ => EventType::Other,
            }
        }
    };
}

event_types! {
    Birth => "birth",
    Death => "death",
    Baptism => "baptism",
    Confirmation => "confirmation",
    FirstCommunion => "first_communion",
    BarBatMitzvah => "bar_bat_mitzvah",
    Burial => "burial",
    Cremation => "cremation",
    Graduation => "graduation",
    Immigration => "immigration",
    Emigration => "emigration",
    Naturalization => "naturalization",
    Census => "census",
    Occupation => "occupation",
    Residence => "residence",
    Retirement => "retirement",
    MilitaryService => "military_service",
    Will => "will",
    Probate => "probate",
    Adoption => "adoption",
    CasteName => "caste_name",
    PhysicalDescription => "physical_description",
    Education => "education",
    NationalId => "national_id",
    NationalOrigin => "national_origin",
    ChildrenCount => "children_count",
    MarriagesCount => "marriages_count",
    Property => "property",
    Religion => "religion",
    SocialSecurityNumber => "social_security_number",
    NobilityTitle => "nobility_title",
    Fact => "fact",
    LdsBaptism => "lds_baptism",
    LdsConfirmation => "lds_confirmation",
    Blessing => "blessing",
    Ordination => "ordination",
    Christening => "christening",
    AdultChristening => "adult_christening",
    Accomplishment => "accomplishment",
    Acquisition => "acquisition",
    Membership => "membership",
    ChangeName => "change_name",
    Circumcision => "circumcision",
    Award => "award",
    MilitaryDischarge => "military_discharge",
    Degree => "degree",
    Distinction => "distinction",
    Election => "election",
    Excommunication => "excommunication",
    Funeral => "funeral",
    Hospitalization => "hospitalization",
    Illness => "illness",
    PassengerList => "passenger_list",
    MilitaryDistinction => "military_distinction",
    MilitaryPromotion => "military_promotion",
    MilitaryMobilization => "military_mobilization",
    PropertySale => "property_sale",
    Endowment => "endowment",
    LdsDotation => "lds_dotation",
    SealingChild => "sealing_child",
    SealingSpouse => "sealing_spouse",
    SealingParent => "sealing_parent",
    FamilyLinkLds => "family_link_lds",
    NoMarriage => "no_marriage",
    NoMention => "no_mention",
    Marriage => "marriage",
    Divorce => "divorce",
    Annulment => "annulment",
    Engagement => "engagement",
    MarriageBann => "marriage_bann",
    MarriageContract => "marriage_contract",
    MarriageLicense => "marriage_license",
    MarriageSettlement => "marriage_settlement",
    CivilUnion => "civil_union",
    Separation => "separation",
    DivorceFiled => "divorce_filed",
    Other => "other",
}

/// Parse a string value from a `<select>` into a [`Privacy`] enum.
pub fn parse_privacy(s: &str) -> Privacy {
    match s {
        "Public" => Privacy::Public,
        "Private" => Privacy::Private,
        _ => Privacy::Default,
    }
}

// ── String helpers ──────────────────────────────────────────────────────

/// Convert a form input string to `Option<String>`, returning `None` for empty strings.
/// How a recorded age reads ("aged 34 years", "aged under 1 year",
/// "infant"), or `None` for text that is not an age.
pub fn age_label(i18n: &I18n, age: &str) -> Option<String> {
    use oxidgene_core::types::age::{AgeAtEvent, AgeModifier};
    let (modifier, units) = match age.parse::<AgeAtEvent>().ok()? {
        AgeAtEvent::Child => return Some(i18n.t("person.age.child")),
        AgeAtEvent::Infant => return Some(i18n.t("person.age.infant")),
        AgeAtEvent::Stillborn => return Some(i18n.t("person.age.stillborn")),
        AgeAtEvent::Duration {
            modifier,
            years,
            months,
            weeks,
            days,
        } => (
            modifier,
            [
                ("person.age.years", years),
                ("person.age.months", months),
                ("person.age.weeks", weeks),
                ("person.age.days", days),
            ],
        ),
    };
    let duration = units
        .into_iter()
        .filter_map(|(key, count)| Some(i18n.t_plural(key, usize::from(count?))))
        .collect::<Vec<_>>()
        .join(" ");
    let key = match modifier {
        AgeModifier::Exact => "person.age.exact",
        AgeModifier::LessThan => "person.age.under",
        AgeModifier::GreaterThan => "person.age.over",
    };
    Some(i18n.t_args(key, &[("age", &duration)]))
}

/// Every citation confidence level, least reliable first.
pub const CONFIDENCE_LEVELS: [Confidence; 5] = [
    Confidence::VeryLow,
    Confidence::Low,
    Confidence::Medium,
    Confidence::High,
    Confidence::VeryHigh,
];

/// The translation key naming a confidence level.
pub fn confidence_key(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::VeryLow => "confidence.very_low",
        Confidence::Low => "confidence.low",
        Confidence::Medium => "confidence.medium",
        Confidence::High => "confidence.high",
        Confidence::VeryHigh => "confidence.very_high",
    }
}

pub fn opt_str(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

// ── Name resolution ─────────────────────────────────────────────────────

/// Resolve a display name for a person from a name map.
///
/// Looks up the person in the map, picks the primary name (or first available),
/// and returns its `display_name()`. Falls back to `"Unnamed"`.
pub fn resolve_name(
    person_id: Uuid,
    name_map: &HashMap<Uuid, Vec<oxidgene_core::types::PersonName>>,
    i18n: &I18n,
) -> String {
    let unnamed = || i18n.t("common.unnamed");
    let Some(names) = name_map.get(&person_id) else {
        return unnamed();
    };
    let Some(primary) = oxidgene_core::types::PersonName::primary(names) else {
        return unnamed();
    };
    let display = primary.display_name();
    if display.is_empty() {
        unnamed()
    } else {
        display
    }
}

/// ── Text truncation ─────────────────────────────────────────────────────
///
/// Estimate rendered text width in pixels for Lato-like sans fonts.
fn estimate_char_width_px(ch: char, font_size_px: f32) -> f32 {
    let ratio = match ch {
        // Extra narrow glyphs
        'i' | 'l' | 'I' | 'j' | 't' | 'f' | 'r' => 0.35,
        // Narrow punctuation and symbols
        '.' | ',' | ':' | ';' | '!' | '|' | '\'' => 0.25,
        // Space-like characters
        ' ' | '\t' => 0.30,
        // Wide uppercase glyphs
        'M' | 'W' => 0.92,
        // Wide lowercase glyphs
        'm' | 'w' => 0.80,
        // Digits
        '0'..='9' => 0.56,
        // Generic uppercase letters
        'A'..='Z' => 0.64,
        // Generic lowercase letters
        'a'..='z' => 0.54,
        // Fallback for non-latin glyphs and symbols
        _ => 0.62,
    };
    ratio * font_size_px
}

/// Turn a stored note body into the HTML actually handed to the DOM.
///
/// Note bodies are stored with their line breaks canonicalised to `\n`, which
/// keeps them useful as plain text — GEDCOM export writes real `CONT` lines,
/// the edit textarea shows text rather than tags — but means nothing to an HTML
/// renderer, which collapses a newline into a space. Restoring `<br>` here is
/// the other half of that trade; see `oxidgene_db::html` for the write side.
///
/// The input is already sanitized, and `<br>` is on its allowlist, so this adds
/// nothing the sanitizer would have removed.
#[must_use]
pub fn note_html_for_display(html: &str) -> String {
    html.replace('\n', "<br>")
}

/// Flatten a sanitized note body into a one-line plain-text preview.
///
/// Note bodies are HTML (see `oxidgene_db::html`), which is fine where they are
/// rendered but not in a list label: raw tags are noise, and truncating markup
/// mid-tag produces broken output. This drops tags, collapses whitespace and
/// cuts on a character boundary — `&text[..n]` would panic the moment an
/// accented letter straddles the byte index.
///
/// Entity handling covers only the few `ammonia` emits; anything else is left
/// as written, which is acceptable for a preview.
#[must_use]
pub fn html_to_preview(html: &str, max_chars: usize) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut entity: Option<String> = None;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if in_tag => {}
            '&' => entity = Some(String::new()),
            ';' if entity.is_some() => {
                let name = entity.take().unwrap_or_default();
                text.push_str(match name.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "#39" | "apos" => "'",
                    "nbsp" => " ",
                    _ => "",
                });
            }
            _ => match entity.as_mut() {
                // An unterminated `&…` is literal text, not an entity.
                Some(buf) if buf.chars().count() < 8 => buf.push(ch),
                Some(_) => {
                    let buf = entity.take().unwrap_or_default();
                    text.push('&');
                    text.push_str(&buf);
                    text.push(ch);
                }
                None => text.push(ch),
            },
        }
    }

    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let kept: String = collapsed.chars().take(max_chars).collect();
    format!("{}…", kept.trim_end())
}

/// Escape the five XML metacharacters, for the rare place that has to build
/// markup as a string rather than as rsx.
///
/// Needed because a date's precision mark is literally `<` or `>` — see
/// `pedigree_chart`'s SVG `<title>` tooltip.
pub fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Estimate rendered text width in pixels for Lato-like sans fonts.
pub fn estimate_text_width_px(text: &str, font_size_px: f32) -> f32 {
    text.chars()
        .map(|ch| estimate_char_width_px(ch, font_size_px))
        .sum()
}

/// Truncate text so its rendered width fits in `max_width_px`, adding an ellipsis.
pub fn truncate_text_to_fit(text: &str, max_width_px: f32, font_size_px: f32) -> String {
    if text.is_empty() || max_width_px <= 0.0 || font_size_px <= 0.0 {
        return String::new();
    }

    if estimate_text_width_px(text, font_size_px) <= max_width_px {
        return text.to_string();
    }

    let ellipsis = '…';
    let ellipsis_width = estimate_text_width_px("…", font_size_px);
    if ellipsis_width >= max_width_px {
        return String::new();
    }

    let mut out = String::new();
    let mut width = 0.0;
    for ch in text.chars() {
        let ch_width = estimate_char_width_px(ch, font_size_px);
        if width + ch_width + ellipsis_width > max_width_px {
            break;
        }
        out.push(ch);
        width += ch_width;
    }

    if out.is_empty() {
        String::new()
    } else {
        out.push(ellipsis);
        out
    }
}

#[cfg(test)]
mod preview_tests {
    use super::html_to_preview;

    #[test]
    fn strips_tags_and_collapses_whitespace() {
        let out = html_to_preview("<p>Ne a <b>Paris</b></p>\n<p>en   1802</p>", 120);
        assert_eq!(out, "Ne a Paris en 1802");
    }

    #[test]
    fn decodes_the_entities_ammonia_emits() {
        assert_eq!(
            html_to_preview("Durand &amp; fils &lt;x&gt;", 120),
            "Durand & fils <x>"
        );
    }

    #[test]
    fn keeps_a_bare_ampersand_as_text() {
        assert_eq!(
            html_to_preview("vins & spiritueux", 120),
            "vins & spiritueux"
        );
    }

    #[test]
    fn truncates_on_a_char_boundary() {
        // Every char is multi-byte: slicing by byte index would panic here.
        let out = html_to_preview(&"é".repeat(50), 10);
        assert_eq!(out.chars().filter(|c| *c == 'é').count(), 10);
        assert!(out.ends_with('…'), "got: {out}");
    }

    #[test]
    fn leaves_short_text_untouched() {
        assert_eq!(html_to_preview("court", 120), "court");
    }
}

#[cfg(test)]
mod enum_table_tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn a_recorded_age_reads_in_words() {
        let en = I18n(Language::En);
        assert_eq!(age_label(&en, "34y").as_deref(), Some("aged 34 years"));
        assert_eq!(
            age_label(&en, "< 1y 6m").as_deref(),
            Some("aged under 1 year 6 months")
        );
        assert_eq!(
            age_label(&en, "> 80y").as_deref(),
            Some("aged over 80 years")
        );
        assert_eq!(age_label(&en, "INFANT").as_deref(), Some("infant"));
        assert_eq!(age_label(&en, "majeur"), None);
        let pl = I18n(Language::Pl);
        assert_eq!(age_label(&pl, "22y").as_deref(), Some("w wieku 22 lat"));
        assert_eq!(age_label(&pl, "1y").as_deref(), Some("w wieku 1 roku"));
    }

    /// Every confidence level names a key every locale translates.
    #[test]
    fn every_confidence_level_is_translated_in_every_locale() {
        for level in CONFIDENCE_LEVELS {
            for lang in Language::ALL {
                assert!(
                    lang.translations().contains_key(confidence_key(level)),
                    "{lang:?} {level:?}"
                );
            }
        }
    }

    /// Every event type must name a key that every locale translates.
    /// Types used to be rendered through `Display`/`Debug`, so a missing
    /// translation showed as "other" or "MarriageBann" in a French form.
    #[test]
    fn every_event_type_is_translated_in_every_locale() {
        for et in EVENT_TYPES {
            let key = event_type_label_key(*et);
            for lang in Language::ALL {
                let translated = lang.translations().get(key).cloned();
                assert!(
                    translated.is_some_and(|t| !t.is_empty()),
                    "{lang:?} has no translation for {key} ({et:?})"
                );
            }
        }
    }

    /// A picker value parses back to its own type: none falls through to
    /// `Other` and two types never share a value.
    #[test]
    fn every_picker_value_parses_back_to_its_type() {
        for et in EVENT_TYPES {
            assert_eq!(parse_event_type(event_type_value(*et)), *et);
        }
        for nt in NAME_TYPES {
            assert_eq!(parse_name_type(name_type_value(*nt)), *nt);
        }
    }

    #[test]
    fn every_name_type_is_translated_in_every_locale() {
        for nt in NAME_TYPES {
            let key = name_type_label_key(*nt);
            for lang in Language::ALL {
                assert!(
                    lang.translations().get(key).is_some_and(|t| !t.is_empty()),
                    "{lang:?} has no translation for {key}"
                );
            }
        }
    }
}
