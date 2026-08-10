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
        // Each information type the picker offers now has its own variant.
        // They used to all collapse onto `AlsoKnownAs`, which made the user's
        // choice unrecoverable on reload — "Alias" and "Surnom" both filled
        // the surname piece, so nothing distinguished them once saved.
        "Prenom" => NameType::GivenName,
        "Alias" => NameType::Alias,
        "Surnom" => NameType::Byname,
        "Sobriquet" => NameType::Sobriquet,
        "Maiden" => NameType::Maiden,
        "Religious" => NameType::Religious,
        _ => NameType::Other,
    }
}

/// The picker value that round-trips back to `name_type`, so a saved entry
/// reopens on the type it was created with.
pub fn name_type_value(nt: NameType) -> &'static str {
    match nt {
        NameType::Birth => "Birth",
        NameType::Married => "Married",
        NameType::AlsoKnownAs => "AlsoKnownAs",
        NameType::GivenName => "Prenom",
        NameType::Alias => "Alias",
        NameType::Byname => "Surnom",
        NameType::Sobriquet => "Sobriquet",
        NameType::Maiden => "Maiden",
        NameType::Religious => "Religious",
        NameType::Other => "Other",
    }
}

/// The i18n key labelling a name type in lists and read-only views.
pub fn name_type_label_key(nt: NameType) -> &'static str {
    match nt {
        NameType::Birth => "name_type.birth",
        NameType::Married => "name_type.married",
        NameType::AlsoKnownAs => "name_type.also_known_as",
        NameType::GivenName => "name_type.prenom",
        NameType::Alias => "name_type.alias",
        NameType::Byname => "name_type.surnom",
        NameType::Sobriquet => "name_type.sobriquet",
        NameType::Maiden => "name_type.maiden",
        NameType::Religious => "name_type.religious",
        NameType::Other => "name_type.other",
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
        "CasteName" => EventType::CasteName,
        "PhysicalDescription" => EventType::PhysicalDescription,
        "Education" => EventType::Education,
        "NationalId" => EventType::NationalId,
        "NationalOrigin" => EventType::NationalOrigin,
        "ChildrenCount" => EventType::ChildrenCount,
        "MarriagesCount" => EventType::MarriagesCount,
        "Property" => EventType::Property,
        "Religion" => EventType::Religion,
        "SocialSecurityNumber" => EventType::SocialSecurityNumber,
        "NobilityTitle" => EventType::NobilityTitle,
        "Fact" => EventType::Fact,
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
        "Calculated" => DateQualifier::Calculated,
        "Estimated" => DateQualifier::Estimated,
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
