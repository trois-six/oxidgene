use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Calendar, DateQualifier, EventType};

/// A genealogical event (birth, death, marriage, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub event_type: EventType,
    /// GEDCOM date phrase (free text, e.g. "ABT 1842", "BET 1800 AND 1810").
    pub date_value: Option<String>,
    /// Normalized date for sorting and filtering.
    pub date_sort: Option<NaiveDate>,
    /// Precision/shape of the date (exact, about, between, ...).
    pub date_qualifier: DateQualifier,
    /// Second date value, used by the `Or` and `Between` qualifiers.
    pub date_value2: Option<String>,
    /// Calendar system the date was recorded in.
    pub calendar: Calendar,
    /// Cause of death/burial/etc. Maps to GEDCOM `CAUS`.
    pub cause: Option<String>,
    pub place_id: Option<Uuid>,
    /// Set for individual events.
    pub person_id: Option<Uuid>,
    /// Set for family events.
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Event {
    /// Display year for this event: prefers the normalized `date_sort`,
    /// falling back to the first 4-digit token in the free-text
    /// `date_value` GEDCOM phrase (e.g. "ABT 1842" -> `Some(1842)`).
    pub fn year(&self) -> Option<i32> {
        year_from_date(self.date_sort, self.date_value.as_deref())
    }
}

/// Shared "resolve a display year" logic used everywhere a birth/death year
/// is shown (pedigree cards, person narrative, dictionary usage lists,
/// search results): prefer the normalized date, fall back to the first
/// 4-digit token in a free-text GEDCOM-style date phrase.
pub fn year_from_date(date_sort: Option<NaiveDate>, date_value: Option<&str>) -> Option<i32> {
    date_sort.map(|d| d.year()).or_else(|| {
        date_value?
            .split_whitespace()
            .find(|w| w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()))
            .and_then(|w| w.parse().ok())
    })
}

/// A witness (or godparent, etc.) linked to an [`Event`] — a pointer to
/// another [`Person`](crate::types::Person) in the tree, mirroring GEDCOM's
/// `ASSO`/`RELA` association structure. `relation` is free text (e.g.
/// "Godmother", "Witness").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventWitness {
    pub id: Uuid,
    pub event_id: Uuid,
    pub person_id: Uuid,
    pub relation: Option<String>,
    pub sort_order: i32,
}
