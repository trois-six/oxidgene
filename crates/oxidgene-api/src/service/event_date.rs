//! Derivation of an event's `date_sort` column.
//!
//! `date_sort` is not a field a client gets to set. It is the normalized
//! Gregorian date the event's own columns imply, and it exists so events can
//! be put in chronological order — which only works if every date, in every
//! calendar, is normalized the same way.
//!
//! A client cannot do that. Converting a Julian, Hebrew or French Republican
//! date to Gregorian needs `ged_io`'s calendar support, which lives behind
//! `oxidgene-gedcom` and is not reachable from the WASM frontend. When the
//! frontend tried anyway it simply read the month number as if it were
//! Gregorian, so a Republican `2 BRUM 14` sorted as the 2nd of a second month
//! in year 14 — in antiquity, thirteen centuries adrift — and a thirteenth
//! month produced no sort key at all.
//!
//! So both write surfaces call [`derive`] and ignore whatever the request
//! carried. Import does not go through here: it gets its sort date from
//! `oxidgene_gedcom::date::parse`, which uses the same conversion.

use chrono::NaiveDate;
use oxidgene_core::Calendar;
use oxidgene_gedcom::date;

/// The sort key for a date being written.
pub fn derive(calendar: Calendar, date_value: Option<&str>) -> Option<NaiveDate> {
    date::sort_key(calendar, date_value)
}

/// The sort key for a *patch*, which may touch the calendar, the date value,
/// both, or neither.
///
/// Either half left alone is taken from the stored event, because the two are
/// only meaningful together: changing a date's calendar without re-reading its
/// value, or its value without re-reading its calendar, would derive the key
/// from a date that never existed.
pub fn derive_patch(
    stored_calendar: Calendar,
    stored_value: Option<&str>,
    patch_calendar: Option<Calendar>,
    patch_value: Option<Option<&str>>,
) -> Option<NaiveDate> {
    let calendar = patch_calendar.unwrap_or(stored_calendar);
    let value = match patch_value {
        Some(v) => v,
        None => stored_value,
    };
    derive(calendar, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gregorian_date_keeps_its_own_day() {
        assert_eq!(
            derive(Calendar::Gregorian, Some("23 FEB 1947")),
            NaiveDate::from_ymd_opt(1947, 2, 23)
        );
    }

    /// The bug this module exists for: read as Gregorian, this date sorts in
    /// year 14 rather than 1805.
    #[test]
    fn a_republican_date_sorts_in_the_right_century() {
        let sort = derive(Calendar::FrenchRepublican, Some("2 BRUM 14"))
            .expect("a Republican date converts");
        assert_eq!(sort.format("%Y-%m").to_string(), "1805-10");
    }

    /// A thirteenth month is real in the Republican and Hebrew years, and used
    /// to yield no sort key at all when read against Gregorian rules.
    #[test]
    fn a_thirteenth_month_still_sorts() {
        assert!(derive(Calendar::FrenchRepublican, Some("3 COMP 8")).is_some());
    }

    #[test]
    fn a_dateless_event_has_no_sort_key() {
        assert_eq!(derive(Calendar::Gregorian, None), None);
        assert_eq!(derive(Calendar::Gregorian, Some("")), None);
    }

    #[test]
    fn a_patch_touching_neither_half_re_derives_from_what_is_stored() {
        assert_eq!(
            derive_patch(Calendar::Gregorian, Some("23 FEB 1947"), None, None),
            NaiveDate::from_ymd_opt(1947, 2, 23)
        );
    }

    #[test]
    fn a_patch_touching_one_half_reads_the_other_from_storage() {
        // Only the value changes: the stored Julian calendar still applies, so
        // the key is the Gregorian equivalent, not the literal date.
        let julian = derive_patch(
            Calendar::Julian,
            Some("1 JAN 1500"),
            None,
            Some(Some("15 MAR 1582")),
        );
        assert_eq!(julian, NaiveDate::from_ymd_opt(1582, 3, 25));

        // Only the calendar changes: the same stored value now means a
        // different day.
        let recalendared = derive_patch(
            Calendar::Gregorian,
            Some("15 MAR 1582"),
            Some(Calendar::Julian),
            None,
        );
        assert_eq!(recalendared, NaiveDate::from_ymd_opt(1582, 3, 25));
    }

    #[test]
    fn clearing_the_date_clears_the_sort_key() {
        assert_eq!(
            derive_patch(Calendar::Gregorian, Some("23 FEB 1947"), None, Some(None)),
            None
        );
    }
}
