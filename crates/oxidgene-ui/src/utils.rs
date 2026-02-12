//! Shared utility functions for parsing enums, formatting, and name resolution.

use std::collections::HashMap;

use uuid::Uuid;

use oxidgene_core::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};

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
        "Will" => EventType::Will,
        "Probate" => EventType::Probate,
        "Marriage" => EventType::Marriage,
        "Divorce" => EventType::Divorce,
        "Annulment" => EventType::Annulment,
        "Engagement" => EventType::Engagement,
        "MarriageBann" => EventType::MarriageBann,
        "MarriageContract" => EventType::MarriageContract,
        "MarriageLicense" => EventType::MarriageLicense,
        "MarriageSettlement" => EventType::MarriageSettlement,
        _ => EventType::Other,
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

/// Parse a string value from a `<select>` into a [`SpouseRole`] enum.
pub fn parse_spouse_role(s: &str) -> SpouseRole {
    match s {
        "Husband" => SpouseRole::Husband,
        "Wife" => SpouseRole::Wife,
        "Partner" => SpouseRole::Partner,
        _ => SpouseRole::Partner,
    }
}

/// Parse a string value from a `<select>` into a [`ChildType`] enum.
pub fn parse_child_type(s: &str) -> ChildType {
    match s {
        "Biological" => ChildType::Biological,
        "Adopted" => ChildType::Adopted,
        "Foster" => ChildType::Foster,
        "Step" => ChildType::Step,
        _ => ChildType::Unknown,
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

// ── Sex display helpers ─────────────────────────────────────────────────

/// Returns a short CSS class suffix for the sex icon (e.g. `"male"`, `"female"`, `""`).
pub fn sex_icon_class(sex: &Sex) -> &'static str {
    match sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "",
    }
}

/// Returns a single-character symbol for the sex (M / F / ?).
pub fn sex_symbol(sex: &Sex) -> &'static str {
    match sex {
        Sex::Male => "M",
        Sex::Female => "F",
        Sex::Unknown => "?",
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

// ── Generation labels ───────────────────────────────────────────────────

/// Returns a human-readable generation label for ancestry/descendant charts.
pub fn generation_label(depth: i32, is_ancestors: bool) -> String {
    if is_ancestors {
        match depth {
            1 => "Parents".to_string(),
            2 => "Grandparents".to_string(),
            3 => "Great-Grandparents".to_string(),
            n => format!("{n}x Great-Grandparents"),
        }
    } else {
        match depth {
            1 => "Children".to_string(),
            2 => "Grandchildren".to_string(),
            3 => "Great-Grandchildren".to_string(),
            n => format!("{n}x Great-Grandchildren"),
        }
    }
}
