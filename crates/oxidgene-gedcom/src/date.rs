//! GEDCOM `DATE` ↔ domain date columns.
//!
//! The domain keeps a date in four orthogonal columns — `calendar`,
//! `date_qualifier`, `date_value` and `date_value2` — where `date_value` is the
//! bare date alone (`23 FEB 1947`), carrying neither the calendar escape nor the
//! qualifier prefix. A GEDCOM `DATE` line packs all of that into one string, so
//! importing means splitting it apart and exporting means putting it back
//! together. Both directions live here so they cannot drift.
//!
//! Escape and qualifier parsing is delegated to `ged_io` (its `calendar`
//! feature), which also normalises non-Gregorian dates when computing the
//! sortable date. Ranges are handled here: `ged_io` models a single instant, so
//! `BET … AND …` and `FROM … TO …` never reach it.
//!
//! ## Known lossy mappings
//!
//! Every GEDCOM qualifier now has its own domain variant, so the import
//! direction is lossless. Only domain values GEDCOM cannot express are lossy:
//!
//! - [`DateQualifier::Perhaps`] (a GeneWeb reading, "maybe") has no GEDCOM tag
//!   and is exported as `EST`, so a round trip returns
//!   [`DateQualifier::Estimated`].
//! - [`DateQualifier::Or`], likewise from GeneWeb, is exported as `BET … AND …`.
//! - [`DateQualifier::FromAge`] has no date of its own and exports bare.

use chrono::NaiveDate;
use ged_io::types::date::Date as GedDate;
use ged_io::types::date::calendar::{
    Calendar as GedCalendar, DateQualifier as GedQualifier, ParsedDateTime,
};
use oxidgene_core::{Calendar, DateQualifier};

/// A GEDCOM `DATE` split into the columns the domain stores.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportedDate {
    pub calendar: Calendar,
    pub qualifier: DateQualifier,
    pub value: Option<String>,
    pub value2: Option<String>,
    pub sort: Option<NaiveDate>,
}

fn from_ged_calendar(c: GedCalendar) -> Calendar {
    match c {
        GedCalendar::Gregorian => Calendar::Gregorian,
        GedCalendar::Julian => Calendar::Julian,
        GedCalendar::Hebrew => Calendar::Hebrew,
        GedCalendar::FrenchRepublican => Calendar::FrenchRepublican,
    }
}

fn to_ged_calendar(c: Calendar) -> GedCalendar {
    match c {
        Calendar::Gregorian => GedCalendar::Gregorian,
        Calendar::Julian => GedCalendar::Julian,
        Calendar::Hebrew => GedCalendar::Hebrew,
        Calendar::FrenchRepublican => GedCalendar::FrenchRepublican,
    }
}

fn from_ged_qualifier(q: GedQualifier) -> DateQualifier {
    match q {
        GedQualifier::Exact => DateQualifier::Exact,
        GedQualifier::About => DateQualifier::About,
        GedQualifier::Calculated => DateQualifier::Calculated,
        GedQualifier::Estimated => DateQualifier::Estimated,
        GedQualifier::Before => DateQualifier::Before,
        GedQualifier::After => DateQualifier::After,
    }
}

/// The GEDCOM tag a qualifier is written with, if it has one.
fn qualifier_tag(q: DateQualifier) -> Option<&'static str> {
    match q {
        DateQualifier::Exact | DateQualifier::FromAge => None,
        DateQualifier::About => Some("ABT"),
        DateQualifier::Calculated => Some("CAL"),
        // GEDCOM has no "perhaps"; `EST` (estimated) is its nearest reading.
        DateQualifier::Estimated | DateQualifier::Perhaps => Some("EST"),
        DateQualifier::Before => Some("BEF"),
        DateQualifier::After => Some("AFT"),
        // Ranges are emitted by `format` itself, which needs both dates.
        DateQualifier::Or | DateQualifier::Between => None,
    }
}

/// Splits `<head> <a> <sep> <b>` on the first top-level `sep`, e.g. `AND`/`TO`.
fn split_range<'a>(rest: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let pat = format!(" {sep} ");
    rest.split_once(&pat)
        .map(|(a, b)| (a.trim(), b.trim()))
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
}

/// Sortable date for a bare value, normalised to Gregorian so that Julian,
/// Hebrew and Republican dates order alongside the rest.
fn sort_date(value: &str, calendar: Calendar) -> Option<NaiveDate> {
    let escaped = match calendar {
        Calendar::Gregorian => value.to_string(),
        other => format!("{} {value}", to_ged_calendar(other).gedcom_escape()),
    };
    let parsed = ParsedDateTime::from_gedcom_date(&escaped).ok()?;
    let parsed = match parsed.calendar {
        GedCalendar::Gregorian => parsed,
        _ => parsed.convert_to(GedCalendar::Gregorian).ok()?,
    };
    // A missing month or day is the first of its period, matching how the UI
    // derives `date_sort` from a partial date.
    NaiveDate::from_ymd_opt(
        parsed.year?,
        u32::from(parsed.month.unwrap_or(1)),
        u32::from(parsed.day.unwrap_or(1)),
    )
}

/// The sortable Gregorian date a stored date value stands for.
///
/// `date_sort` is what orders events against one another, so it has to be
/// Gregorian whatever calendar the date was *written* in: a Republican
/// `2 BRUM 14` sorts as 24 October 1805, not as the second day of a second
/// month in year 14. Doing that conversion needs `ged_io`'s calendar support,
/// which is why this lives here rather than anywhere a client could reach.
///
/// Returns `None` for a missing, empty or unparseable value — an event whose
/// date is a free-text phrase simply has no place in a chronological order.
///
pub fn sort_key(calendar: Calendar, value: Option<&str>) -> Option<NaiveDate> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;
    sort_date(value, calendar)
}

/// Splits a raw GEDCOM `DATE` value into the domain's date columns.
///
/// Anything that cannot be recognised is preserved verbatim in `value`, so an
/// unparseable date is never silently dropped.
pub fn parse(raw: &str) -> ImportedDate {
    let raw = raw.trim();
    if raw.is_empty() {
        return ImportedDate::default();
    }

    // 1. Calendar escape (`@#DJULIAN@ …`), handled by ged_io.
    let probe = GedDate {
        value: Some(raw.to_string()),
        ..Default::default()
    };
    let calendar = probe.calendar().map(from_ged_calendar).unwrap_or_default();
    let body = probe
        .value_without_calendar()
        .unwrap_or_else(|| raw.to_string());
    let body = body.trim();

    // 2. Ranges, which ged_io does not model.
    let (qualifier, value, value2) = if let Some(rest) = body.strip_prefix("BET ") {
        match split_range(rest, "AND") {
            Some((a, b)) => (
                DateQualifier::Between,
                Some(a.to_string()),
                Some(b.to_string()),
            ),
            None => (DateQualifier::Between, Some(rest.trim().to_string()), None),
        }
    } else if let Some(rest) = body.strip_prefix("FROM ") {
        match split_range(rest, "TO") {
            Some((a, b)) => (
                DateQualifier::Between,
                Some(a.to_string()),
                Some(b.to_string()),
            ),
            None => (DateQualifier::Exact, Some(rest.trim().to_string()), None),
        }
    } else if let Some(rest) = body.strip_prefix("TO ") {
        (DateQualifier::Before, Some(rest.trim().to_string()), None)
    } else {
        // 3. Single date with an optional leading qualifier tag.
        let (tag, rest) = body.split_once(' ').unwrap_or((body, ""));
        match GedQualifier::parse(tag) {
            Some(q) if !rest.trim().is_empty() => {
                (from_ged_qualifier(q), Some(rest.trim().to_string()), None)
            }
            _ => (DateQualifier::Exact, Some(body.to_string()), None),
        }
    };

    let value = value.filter(|v| !v.is_empty());
    let sort = value.as_deref().and_then(|v| sort_date(v, calendar));

    ImportedDate {
        calendar,
        qualifier,
        value,
        value2,
        sort,
    }
}

/// Rebuilds a GEDCOM `DATE` value from the domain's columns.
///
/// Returns `None` when there is no date to write.
pub fn format(
    calendar: Calendar,
    qualifier: DateQualifier,
    value: Option<&str>,
    value2: Option<&str>,
) -> Option<String> {
    let value = value.map(str::trim).filter(|v| !v.is_empty())?;

    let body = match qualifier {
        // `Or` has no GEDCOM form; `BET … AND …` is the closest.
        DateQualifier::Between | DateQualifier::Or => {
            match value2.map(str::trim).filter(|v| !v.is_empty()) {
                Some(second) => format!("BET {value} AND {second}"),
                None => value.to_string(),
            }
        }
        other => match qualifier_tag(other) {
            Some(tag) => format!("{tag} {value}"),
            None => value.to_string(),
        },
    };

    Some(match calendar {
        Calendar::Gregorian => body,
        other => format!("{} {body}", to_ged_calendar(other).gedcom_escape()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_date_keeps_its_value_and_defaults() {
        let d = parse("23 FEB 1947");
        assert_eq!(d.calendar, Calendar::Gregorian);
        assert_eq!(d.qualifier, DateQualifier::Exact);
        assert_eq!(d.value.as_deref(), Some("23 FEB 1947"));
        assert_eq!(d.value2, None);
        assert_eq!(d.sort, NaiveDate::from_ymd_opt(1947, 2, 23));
    }

    #[test]
    fn a_qualifier_tag_moves_out_of_the_value() {
        let d = parse("ABT 1850");
        assert_eq!(d.qualifier, DateQualifier::About);
        assert_eq!(d.value.as_deref(), Some("1850"));
        assert_eq!(d.sort, NaiveDate::from_ymd_opt(1850, 1, 1));
    }

    #[test]
    fn calculated_and_estimated_keep_their_own_identity() {
        assert_eq!(parse("CAL 1850").qualifier, DateQualifier::Calculated);
        assert_eq!(parse("EST 1850").qualifier, DateQualifier::Estimated);
    }

    /// `Perhaps` is a GeneWeb reading with no GEDCOM tag — it degrades to `EST`
    /// on the way out, which is the one qualifier that cannot round-trip.
    #[test]
    fn perhaps_degrades_to_estimated() {
        let s = format(
            Calendar::Gregorian,
            DateQualifier::Perhaps,
            Some("1850"),
            None,
        );
        assert_eq!(s.as_deref(), Some("EST 1850"));
        assert_eq!(parse("EST 1850").qualifier, DateQualifier::Estimated);
    }

    #[test]
    fn a_between_range_fills_both_values() {
        let d = parse("BET 1800 AND 1810");
        assert_eq!(d.qualifier, DateQualifier::Between);
        assert_eq!(d.value.as_deref(), Some("1800"));
        assert_eq!(d.value2.as_deref(), Some("1810"));
    }

    #[test]
    fn a_from_to_period_is_read_as_a_range() {
        let d = parse("FROM 1914 TO 1918");
        assert_eq!(d.qualifier, DateQualifier::Between);
        assert_eq!(d.value.as_deref(), Some("1914"));
        assert_eq!(d.value2.as_deref(), Some("1918"));
    }

    #[test]
    fn a_calendar_escape_is_lifted_out_of_the_value() {
        let d = parse("@#DJULIAN@ 15 MAR 1582");
        assert_eq!(d.calendar, Calendar::Julian);
        assert_eq!(d.value.as_deref(), Some("15 MAR 1582"));
        // Julian 15 Mar 1582 is 25 Mar 1582 Gregorian — the sort date is
        // normalised so it orders against Gregorian dates.
        assert_eq!(d.sort, NaiveDate::from_ymd_opt(1582, 3, 25));
    }

    #[test]
    fn an_unrecognised_date_is_preserved_verbatim() {
        let d = parse("sometime around the war");
        assert_eq!(d.qualifier, DateQualifier::Exact);
        assert_eq!(d.value.as_deref(), Some("sometime around the war"));
        assert_eq!(d.sort, None);
    }

    #[test]
    fn an_empty_date_yields_nothing() {
        assert_eq!(parse("   "), ImportedDate::default());
        assert_eq!(parse("   ").value, None);
    }

    #[test]
    fn formatting_puts_the_tag_back() {
        let s = format(
            Calendar::Gregorian,
            DateQualifier::About,
            Some("1850"),
            None,
        );
        assert_eq!(s.as_deref(), Some("ABT 1850"));
    }

    #[test]
    fn formatting_a_range_rebuilds_bet_and() {
        let s = format(
            Calendar::Gregorian,
            DateQualifier::Between,
            Some("1800"),
            Some("1810"),
        );
        assert_eq!(s.as_deref(), Some("BET 1800 AND 1810"));
    }

    #[test]
    fn formatting_or_falls_back_to_bet_and() {
        let s = format(
            Calendar::Gregorian,
            DateQualifier::Or,
            Some("1800"),
            Some("1810"),
        );
        assert_eq!(s.as_deref(), Some("BET 1800 AND 1810"));
    }

    #[test]
    fn formatting_restores_the_calendar_escape() {
        let s = format(
            Calendar::Julian,
            DateQualifier::Exact,
            Some("15 MAR 1582"),
            None,
        );
        assert_eq!(s.as_deref(), Some("@#DJULIAN@ 15 MAR 1582"));
    }

    #[test]
    fn formatting_nothing_yields_none() {
        assert_eq!(
            format(Calendar::Gregorian, DateQualifier::Exact, None, None),
            None
        );
        assert_eq!(
            format(Calendar::Gregorian, DateQualifier::Exact, Some("  "), None),
            None
        );
    }

    /// The whole point of `sort_key`: a date written in another calendar has
    /// to land where it belongs among Gregorian ones.
    #[test]
    fn sort_key_normalises_other_calendars() {
        // Julian 15 Mar 1582 is 25 Mar 1582 Gregorian.
        assert_eq!(
            sort_key(Calendar::Julian, Some("15 MAR 1582")),
            NaiveDate::from_ymd_opt(1582, 3, 25)
        );
        assert_eq!(
            sort_key(Calendar::Gregorian, Some("23 FEB 1947")),
            NaiveDate::from_ymd_opt(1947, 2, 23)
        );
        // 2 Brumaire XIV is late 1805, not "month 2 of year 14" — which is
        // where its own numbering would file it, in antiquity, if nothing
        // converted it.
        let republican = sort_key(Calendar::FrenchRepublican, Some("2 BRUM 14"))
            .expect("a Republican date converts");
        assert_eq!(republican.format("%Y-%m").to_string(), "1805-10");
    }

    /// Republican dates land on their documented Gregorian day.
    #[test]
    fn republican_dates_land_on_their_documented_gregorian_day() {
        for (raw, expected) in [
            // The epoch: the autumn equinox the calendar was anchored to.
            ("1 VEND 1", (1792, 9, 22)),
            // 9 Thermidor An II — the fall of Robespierre.
            ("9 THER 2", (1794, 7, 27)),
            // 18 Brumaire An VIII — Bonaparte's coup.
            ("18 BRUM 8", (1799, 11, 9)),
            // The last new year the calendar saw, and the day after it.
            ("1 VEND 14", (1805, 9, 23)),
            ("2 BRUM 14", (1805, 10, 24)),
        ] {
            let (y, m, d) = expected;
            assert_eq!(
                sort_key(Calendar::FrenchRepublican, Some(raw)),
                NaiveDate::from_ymd_opt(y, m, d),
                "for {raw}"
            );
        }
    }

    /// The other calendars are checked against known dates too.
    #[test]
    fn the_other_calendars_are_left_alone() {
        for (calendar, raw, expected) in [
            // Rosh Hashanah 5784.
            (Calendar::Hebrew, "1 TSH 5784", (2023, 9, 16)),
            (Calendar::Julian, "15 MAR 1582", (1582, 3, 25)),
            (Calendar::Gregorian, "23 FEB 1947", (1947, 2, 23)),
        ] {
            let (y, m, d) = expected;
            assert_eq!(
                sort_key(calendar, Some(raw)),
                NaiveDate::from_ymd_opt(y, m, d),
                "for {raw}"
            );
        }
    }

    #[test]
    fn sort_key_of_nothing_sortable_is_none() {
        assert_eq!(sort_key(Calendar::Gregorian, None), None);
        assert_eq!(sort_key(Calendar::Gregorian, Some("   ")), None);
        assert_eq!(
            sort_key(Calendar::Gregorian, Some("sometime around the war")),
            None
        );
    }

    /// The pairing that matters: what we write must read back unchanged.
    #[test]
    fn the_common_forms_survive_a_round_trip() {
        for raw in [
            "23 FEB 1947",
            "ABT 1850",
            "BEF 3 JAN 1900",
            "AFT 1920",
            "CAL 1799",
            "EST 1805",
            "BET 1800 AND 1810",
            "@#DJULIAN@ 15 MAR 1582",
        ] {
            let d = parse(raw);
            let back = format(
                d.calendar,
                d.qualifier,
                d.value.as_deref(),
                d.value2.as_deref(),
            );
            assert_eq!(back.as_deref(), Some(raw), "round trip failed for {raw}");
        }
    }
}
