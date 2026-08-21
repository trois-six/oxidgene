use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Calendar, DateQualifier, EventType};

/// A genealogical event (birth, death, marriage, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub event_type: EventType,
    /// The date alone, without qualifier or calendar escape (e.g. "1842",
    /// "23 FEB 1947"). The qualifier lives in `date_qualifier`, the calendar in
    /// `calendar`, and a range's second date in `date_value2`; GEDCOM packs all
    /// four into one line, which `oxidgene_gedcom::date` splits and rebuilds.
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

    /// [`Event::year`] with the precision that produced it, for the surfaces
    /// that show a year alone and would otherwise present a guess as a fact.
    ///
    /// A `Between`/`Or` date carries both of its years, since the range is the
    /// fact — "between 1691 and 1693" says more than either year alone.
    pub fn qualified_year(&self) -> Option<QualifiedYear> {
        let year2 = self
            .date_qualifier
            .needs_second_date()
            .then(|| year_from_date(None, self.date_value2.as_deref()))
            .flatten();
        Some(QualifiedYear {
            year: self.year()?,
            qualifier: self.date_qualifier,
            year2,
        })
    }
}

/// A display year and how much it should be trusted.
///
/// A pedigree card has room for a year and nothing else, so the qualifier has
/// to travel *with* the year or it is dropped at the last step: `Some(1849)`
/// cannot remember that the record said "about". Keeping the pair together
/// means the card, the dictionary and the relative lists all render the same
/// hedge from the same value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualifiedYear {
    pub year: i32,
    pub qualifier: DateQualifier,
    /// The far end of a `Between`/`Or` range, when it is known.
    pub year2: Option<i32>,
}

impl QualifiedYear {
    /// A single year of the given precision — no range.
    pub fn new(year: i32, qualifier: DateQualifier) -> Self {
        Self {
            year,
            qualifier,
            year2: None,
        }
    }

    /// The widest useful form: a range shows both of its years
    /// (`1691..1693`, `1691|1693`), anything else is the mark plus the year.
    ///
    /// This is [`Display`](std::fmt::Display); see [`Self::narrow`] for the
    /// version that fits where this one does not.
    pub fn wide(&self) -> String {
        match (self.range_separator(), self.year2) {
            (Some(sep), Some(year2)) => format!("{}{sep}{year2}", self.year),
            _ => self.narrow(),
        }
    }

    /// The compact form, always a mark plus one year (`.. 1691`, `ca 1849`).
    ///
    /// A range loses its far end here, but keeps the mark saying it *is* a
    /// range — so the card understates rather than misleads. Used when
    /// [`Self::wide`] does not fit the card, and the full text is a hover away.
    pub fn narrow(&self) -> String {
        format!("{}{}", self.qualifier.short_prefix(), self.year)
    }

    /// The separator between a range's two years, or `None` if not a range.
    fn range_separator(&self) -> Option<&'static str> {
        match self.qualifier {
            DateQualifier::Between => Some(".."),
            DateQualifier::Or => Some("|"),
            _ => None,
        }
    }
}

impl std::fmt::Display for QualifiedYear {
    /// The short form drawn on a card — `1849`, `ca 1849`, `< 1917`,
    /// `1691..1693`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.wide())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A year drawn alone has to say how much it can be trusted, or a guess
    /// reads as a fact. This is the `ca 1849` / `< 1917` form.
    #[test]
    fn a_qualified_year_wears_its_precision() {
        assert_eq!(
            QualifiedYear::new(1849, DateQualifier::Exact).to_string(),
            "1849"
        );
        assert_eq!(
            QualifiedYear::new(1849, DateQualifier::About).to_string(),
            "ca 1849"
        );
        assert_eq!(
            QualifiedYear::new(1917, DateQualifier::Before).to_string(),
            "< 1917"
        );
        assert_eq!(
            QualifiedYear::new(1912, DateQualifier::After).to_string(),
            "> 1912"
        );
    }

    fn dated(value: &str, qualifier: DateQualifier) -> Event {
        let now = Utc::now();
        Event {
            id: Uuid::nil(),
            tree_id: Uuid::nil(),
            event_type: crate::enums::EventType::Birth,
            date_value: Some(value.to_string()),
            date_sort: None,
            date_qualifier: qualifier,
            date_value2: None,
            calendar: Calendar::default(),
            cause: None,
            place_id: None,
            person_id: None,
            family_id: None,
            description: None,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// `year()` drops the qualifier by design — it returns a number. The point
    /// of `qualified_year()` is that the pair travels together to the card.
    #[test]
    fn qualified_year_keeps_what_year_discards() {
        let about = dated("ABT 1849", DateQualifier::About);
        assert_eq!(about.year(), Some(1849));
        assert_eq!(about.qualified_year().unwrap().to_string(), "ca 1849");

        // No date at all: nothing to qualify, and nothing to draw.
        let mut undated = dated("", DateQualifier::About);
        undated.date_value = None;
        assert!(undated.qualified_year().is_none());
    }
}
