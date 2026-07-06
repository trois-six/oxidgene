//! Shared utility functions for parsing enums, formatting, and name resolution.

use std::collections::HashMap;

use uuid::Uuid;

use oxidgene_core::{Calendar, Confidence, DateQualifier, EventType, NameType, Privacy, Sex};

// ── Enum parsers ────────────────────────────────────────────────────────

/// Parse a string value from a `<select>` into a [`Sex`] enum.
pub fn parse_sex(s: &str) -> Sex {
    match s {
        "Male" => Sex::Male,
        "Female" => Sex::Female,
        _ => Sex::Unknown,
    }
}

/// Parse a string value from a `<select>` into a [`NameType`] enum.
pub fn parse_name_type(s: &str) -> NameType {
    match s {
        "Birth" => NameType::Birth,
        "Married" => NameType::Married,
        "AlsoKnownAs" => NameType::AlsoKnownAs,
        "Maiden" => NameType::Maiden,
        "Religious" => NameType::Religious,
        _ => NameType::Other,
    }
}

/// Parse a string value from a `<select>` into an [`EventType`] enum.
pub fn parse_event_type(s: &str) -> EventType {
    match s {
        "Birth" => EventType::Birth,
        "Death" => EventType::Death,
        "Baptism" => EventType::Baptism,
        "Confirmation" => EventType::Confirmation,
        "FirstCommunion" => EventType::FirstCommunion,
        "BarBatMitzvah" => EventType::BarBatMitzvah,
        "Burial" => EventType::Burial,
        "Cremation" => EventType::Cremation,
        "Graduation" => EventType::Graduation,
        "Immigration" => EventType::Immigration,
        "Emigration" => EventType::Emigration,
        "Naturalization" => EventType::Naturalization,
        "Census" => EventType::Census,
        "Occupation" => EventType::Occupation,
        "Residence" => EventType::Residence,
        "Retirement" => EventType::Retirement,
        "MilitaryService" => EventType::MilitaryService,
        "Will" => EventType::Will,
        "Probate" => EventType::Probate,
        "Adoption" => EventType::Adoption,
        "Marriage" => EventType::Marriage,
        "Divorce" => EventType::Divorce,
        "Annulment" => EventType::Annulment,
        "Engagement" => EventType::Engagement,
        "MarriageBann" => EventType::MarriageBann,
        "MarriageContract" => EventType::MarriageContract,
        "MarriageLicense" => EventType::MarriageLicense,
        "MarriageSettlement" => EventType::MarriageSettlement,
        "CivilUnion" => EventType::CivilUnion,
        "Separation" => EventType::Separation,
        "DivorceFiled" => EventType::DivorceFiled,
        _ => EventType::Other,
    }
}

/// Parse a string value from a `<select>` into a [`DateQualifier`] enum.
pub fn parse_date_qualifier(s: &str) -> DateQualifier {
    match s {
        "About" => DateQualifier::About,
        "Perhaps" => DateQualifier::Perhaps,
        "Before" => DateQualifier::Before,
        "After" => DateQualifier::After,
        "Or" => DateQualifier::Or,
        "Between" => DateQualifier::Between,
        "FromAge" => DateQualifier::FromAge,
        _ => DateQualifier::Exact,
    }
}

/// Parse a string value from a `<select>` into a [`Calendar`] enum.
pub fn parse_calendar(s: &str) -> Calendar {
    match s {
        "Julian" => Calendar::Julian,
        "Hebrew" => Calendar::Hebrew,
        "FrenchRepublican" => Calendar::FrenchRepublican,
        _ => Calendar::Gregorian,
    }
}

/// Parse a string value from a `<select>` into a [`Privacy`] enum.
pub fn parse_privacy(s: &str) -> Privacy {
    match s {
        "Public" => Privacy::Public,
        "Private" => Privacy::Private,
        _ => Privacy::Default,
    }
}

/// Parse a string value from a `<select>` into a [`Confidence`] enum.
pub fn parse_confidence(s: &str) -> Confidence {
    match s {
        "VeryLow" => Confidence::VeryLow,
        "Low" => Confidence::Low,
        "High" => Confidence::High,
        "VeryHigh" => Confidence::VeryHigh,
        _ => Confidence::Medium,
    }
}

// ── String helpers ──────────────────────────────────────────────────────

/// Convert a form input string to `Option<String>`, returning `None` for empty strings.
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
) -> String {
    match name_map.get(&person_id) {
        Some(names) => {
            let primary = names.iter().find(|n| n.is_primary).or(names.first());
            match primary {
                Some(name) => {
                    let dn = name.display_name();
                    if dn.is_empty() {
                        "Unnamed".to_string()
                    } else {
                        dn
                    }
                }
                None => "Unnamed".to_string(),
            }
        }
        None => "Unnamed".to_string(),
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

/// Estimate rendered text width in pixels for Lato-like sans fonts.
fn estimate_text_width_px(text: &str, font_size_px: f32) -> f32 {
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
