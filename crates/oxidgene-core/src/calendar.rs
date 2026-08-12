//! Calendar arithmetic: the same instant, expressed in another calendar.
//!
//! A date is stored in the calendar it was *recorded* in — a parish register
//! kept under the Republic says « 21 ventôse an IV », not « 11 mars 1796 ». So
//! when a date already entered is re-labelled as belonging to another calendar,
//! its components have to be re-expressed rather than left standing: the same
//! day, renumbered.
//!
//! Everything goes through the Julian Day Number, the day count every calendar
//! here can be mapped onto, so each calendar only needs to know how to convert
//! to and from that one axis rather than to each of the others.
//!
//! This lives in `core`, not in `oxidgene-gedcom`: the editor is the main
//! caller and runs in WASM, where `ged_io` (which the server uses to derive
//! `date_sort`) cannot be reached. The two agree — the Republican anchors below
//! are the same documented dates `oxidgene-gedcom`'s tests pin `ged_io` to.
//!
//! Years follow the historical convention the rest of the app uses: there is no
//! year 0, and a year before the common era is negative (`-44` is 44 BCE).
//! Hebrew and Republican years only exist from 1 onwards.

use crate::enums::Calendar;

/// Euclidean division, rounding towards minus infinity.
///
/// Rust's `/` truncates towards zero, which breaks every one of the formulas
/// below as soon as a date lands before the common era.
const fn fdiv(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

// ── Gregorian & Julian ────────────────────────────────────────────────────

/// Historical year (no year 0) to the astronomical numbering the formulas use.
const fn astronomical(year: i32) -> i64 {
    if year < 0 {
        year as i64 + 1
    } else {
        year as i64
    }
}

/// Inverse of [`astronomical`].
const fn historical(year: i64) -> i32 {
    if year <= 0 {
        (year - 1) as i32
    } else {
        year as i32
    }
}

fn gregorian_to_jdn(year: i64, month: i64, day: i64) -> i64 {
    let a = fdiv(14 - month, 12);
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    day + fdiv(153 * m + 2, 5) + 365 * y + fdiv(y, 4) - fdiv(y, 100) + fdiv(y, 400) - 32045
}

fn jdn_to_gregorian(jdn: i64) -> (i64, i64, i64) {
    let a = jdn + 32044;
    let b = fdiv(4 * a + 3, 146097);
    let c = a - fdiv(146097 * b, 4);
    let d = fdiv(4 * c + 3, 1461);
    let e = c - fdiv(1461 * d, 4);
    let m = fdiv(5 * e + 2, 153);
    (
        100 * b + d - 4800 + fdiv(m, 10),
        m + 3 - 12 * fdiv(m, 10),
        e - fdiv(153 * m + 2, 5) + 1,
    )
}

fn julian_to_jdn(year: i64, month: i64, day: i64) -> i64 {
    let a = fdiv(14 - month, 12);
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    day + fdiv(153 * m + 2, 5) + 365 * y + fdiv(y, 4) - 32083
}

fn jdn_to_julian(jdn: i64) -> (i64, i64, i64) {
    let c = jdn + 32082;
    let d = fdiv(4 * c + 3, 1461);
    let e = c - fdiv(1461 * d, 4);
    let m = fdiv(5 * e + 2, 153);
    (
        d - 4800 + fdiv(m, 10),
        m + 3 - 12 * fdiv(m, 10),
        e - fdiv(153 * m + 2, 5) + 1,
    )
}

// ── French Republican ─────────────────────────────────────────────────────

/// The Gregorian date 1 Vendémiaire fell on, for every year the calendar was
/// actually in force (an I to an XIV, 1792–1805).
///
/// These are not computed: the Republican new year was the *observed* autumn
/// equinox at the Paris meridian, which no arithmetic rule reproduces exactly.
/// Tabulating the fourteen years the calendar lived is both shorter and right.
const REPUBLICAN_NEW_YEAR: [(i32, u32, u32); 14] = [
    (1792, 9, 22),
    (1793, 9, 22),
    (1794, 9, 22),
    (1795, 9, 23),
    (1796, 9, 22),
    (1797, 9, 22),
    (1798, 9, 22),
    (1799, 9, 23),
    (1800, 9, 23),
    (1801, 9, 23),
    (1802, 9, 23),
    (1803, 9, 24),
    (1804, 9, 23),
    (1805, 9, 23),
];

/// Whether a Republican year is *sextile* — six jours complémentaires instead
/// of five.
///
/// Within the calendar's own lifetime the answer comes from the table above:
/// years III, VII and XI were long. Past an XIV the calendar had been abolished
/// for a century, so any answer is an extrapolation; the equinox rule's own
/// rhythm — every fourth year, continuing III, VII, XI — is the one kept.
const fn republican_leap(year: i32) -> bool {
    if year <= 14 {
        matches!(year, 3 | 7 | 11)
    } else {
        year % 4 == 3
    }
}

fn republican_new_year_jdn(year: i32) -> Option<i64> {
    if year < 1 {
        return None;
    }
    if let Some(&(y, m, d)) = REPUBLICAN_NEW_YEAR.get(year as usize - 1) {
        return Some(gregorian_to_jdn(y as i64, m as i64, d as i64));
    }
    let last = REPUBLICAN_NEW_YEAR.len() as i32;
    let (y, m, d) = REPUBLICAN_NEW_YEAR[last as usize - 1];
    let mut jdn = gregorian_to_jdn(y as i64, m as i64, d as i64);
    for yy in last..year {
        jdn += if republican_leap(yy) { 366 } else { 365 };
    }
    Some(jdn)
}

// ── Hebrew ────────────────────────────────────────────────────────────────

/// JDN of 1 Tishrei of Hebrew year 1, the zero of [`hebrew_elapsed_days`].
const HEBREW_EPOCH_JDN: i64 = 347_997;

/// A Hebrew leap year carries a thirteenth month; seven fall in each cycle of
/// nineteen.
const fn hebrew_leap(year: i64) -> bool {
    (7 * year + 1).rem_euclid(19) < 7
}

/// Days from the Hebrew epoch to 1 Tishrei of `year`.
///
/// This is the classical calculation: the molad (mean lunar conjunction) of
/// Tishrei, then the four *dehiyyot* that push the new year forward so it never
/// lands on a day the festival calendar forbids.
fn hebrew_elapsed_days(year: i64) -> i64 {
    let cycles = fdiv(year - 1, 19);
    let in_cycle = (year - 1).rem_euclid(19);
    let months = 235 * cycles + 12 * in_cycle + fdiv(7 * in_cycle + 1, 19);

    let parts_elapsed = 204 + 793 * (months % 1080);
    let hours_elapsed = 5 + 12 * months + 793 * fdiv(months, 1080) + fdiv(parts_elapsed, 1080);
    let day = 1 + 29 * months + fdiv(hours_elapsed, 24);
    let parts = 1080 * (hours_elapsed % 24) + parts_elapsed % 1080;

    // Molad zaken, and the two rules that keep a year from being 356 or 382
    // days long.
    let postponed = parts >= 19440
        || (day % 7 == 2 && parts >= 9924 && !hebrew_leap(year))
        || (day % 7 == 1 && parts >= 16789 && hebrew_leap(year - 1));
    let day = if postponed { day + 1 } else { day };

    // Lo ADU rosh: the year may not open on a Sunday, Wednesday or Friday.
    if matches!(day % 7, 0 | 3 | 5) {
        day + 1
    } else {
        day
    }
}

fn hebrew_new_year_jdn(year: i64) -> i64 {
    HEBREW_EPOCH_JDN + hebrew_elapsed_days(year)
}

/// 353, 354 or 355 days — or 383, 384, 385 in a leap year. The two variable
/// months, Heshvan and Kislev, absorb the difference.
fn hebrew_year_length(year: i64) -> i64 {
    hebrew_elapsed_days(year + 1) - hebrew_elapsed_days(year)
}

// ── Public API ────────────────────────────────────────────────────────────

/// How many months a year of this calendar has.
///
/// The Hebrew year answers 13 whatever its length: the GEDCOM month list keeps
/// a slot for Adar II, which simply has no days in a common year.
pub const fn months_in_year(calendar: Calendar) -> u8 {
    match calendar {
        Calendar::Gregorian | Calendar::Julian => 12,
        Calendar::Hebrew | Calendar::FrenchRepublican => 13,
    }
}

/// Length of one month, or 0 for a month that does not exist that year
/// (Adar II outside a Hebrew leap year).
///
/// Months are numbered as GEDCOM lists them for each calendar: 1 = January,
/// 1 = Tishrei, 1 = Vendémiaire. The Republican thirteenth is not a month but
/// the five (six in a sextile year) jours complémentaires.
pub fn days_in_month(calendar: Calendar, year: i32, month: u8) -> u8 {
    if month < 1 || month > months_in_year(calendar) {
        return 0;
    }
    match calendar {
        Calendar::FrenchRepublican => match month {
            13 => {
                if republican_leap(year) {
                    6
                } else {
                    5
                }
            }
            _ => 30,
        },
        Calendar::Hebrew => {
            let y = year as i64;
            let leap = hebrew_leap(y);
            let length = hebrew_year_length(y);
            match month {
                // Heshvan is full only in a "complete" year, Kislev short only
                // in a "deficient" one.
                2 => {
                    if length % 10 == 5 {
                        30
                    } else {
                        29
                    }
                }
                3 => {
                    if length % 10 == 3 {
                        29
                    } else {
                        30
                    }
                }
                6 => {
                    if leap {
                        30
                    } else {
                        29
                    }
                }
                7 => {
                    if leap {
                        29
                    } else {
                        0
                    }
                }
                1 | 5 | 8 | 10 | 12 => 30,
                _ => 29,
            }
        }
        Calendar::Julian => match month {
            2 => {
                if astronomical(year).rem_euclid(4) == 0 {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        },
        Calendar::Gregorian => match month {
            2 => {
                let y = astronomical(year);
                if y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0) {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        },
    }
}

/// The Julian Day Number of a complete date, or `None` if that date does not
/// exist in this calendar.
pub fn to_jdn(calendar: Calendar, year: i32, month: u8, day: u8) -> Option<i64> {
    if year == 0 || day < 1 || day > days_in_month(calendar, year, month) {
        return None;
    }
    let (m, d) = (month as i64, day as i64);
    match calendar {
        Calendar::Gregorian => Some(gregorian_to_jdn(astronomical(year), m, d)),
        Calendar::Julian => Some(julian_to_jdn(astronomical(year), m, d)),
        Calendar::FrenchRepublican => Some(republican_new_year_jdn(year)? + (m - 1) * 30 + d - 1),
        Calendar::Hebrew => {
            if year < 1 {
                return None;
            }
            let mut jdn = hebrew_new_year_jdn(year as i64);
            for prior in 1..month {
                jdn += i64::from(days_in_month(calendar, year, prior));
            }
            Some(jdn + d - 1)
        }
    }
}

/// The date a Julian Day Number falls on in this calendar, or `None` when it
/// falls before the calendar begins.
pub fn from_jdn(calendar: Calendar, jdn: i64) -> Option<(i32, u8, u8)> {
    match calendar {
        Calendar::Gregorian | Calendar::Julian => {
            let (y, m, d) = match calendar {
                Calendar::Julian => jdn_to_julian(jdn),
                _ => jdn_to_gregorian(jdn),
            };
            Some((historical(y), m as u8, d as u8))
        }
        Calendar::FrenchRepublican => {
            let epoch = republican_new_year_jdn(1)?;
            if jdn < epoch {
                return None;
            }
            // The estimate is short by construction (366 > a real year), so the
            // search only ever walks forwards, a handful of steps.
            let mut year = ((jdn - epoch) / 366) as i32 + 1;
            while republican_new_year_jdn(year + 1).is_some_and(|start| start <= jdn) {
                year += 1;
            }
            let doy = jdn - republican_new_year_jdn(year)?;
            Some((year, (doy / 30) as u8 + 1, (doy % 30) as u8 + 1))
        }
        Calendar::Hebrew => {
            if jdn < hebrew_new_year_jdn(1) {
                return None;
            }
            let mut year = ((jdn - HEBREW_EPOCH_JDN) / 366) + 1;
            while hebrew_new_year_jdn(year + 1) <= jdn {
                year += 1;
            }
            let mut rest = jdn - hebrew_new_year_jdn(year);
            let year = year as i32;
            for month in 1..=months_in_year(calendar) {
                let len = i64::from(days_in_month(calendar, year, month));
                if rest < len {
                    return Some((year, month, rest as u8 + 1));
                }
                rest -= len;
            }
            None
        }
    }
}

/// Re-expresses a date in another calendar, keeping the precision it was
/// entered with.
///
/// A partial date names a period, not a day, so it is converted through the
/// *middle* of that period and truncated back: this hands back the year (or
/// month) the original overlaps most, rather than the one its first day happens
/// to touch — « 1900 » in Gregorian is « 1900 » in Julian, not « 1899 ».
///
/// Returns `None` when the date cannot be placed in the target calendar — a
/// Gregorian year long before the Republic, say — so the caller can leave what
/// the user typed alone instead of replacing it with a wrong answer.
pub fn convert(
    from: Calendar,
    to: Calendar,
    year: i32,
    month: Option<u8>,
    day: Option<u8>,
) -> Option<(i32, Option<u8>, Option<u8>)> {
    if from == to {
        return Some((year, month, day));
    }
    let (jdn, keep_month, keep_day) = match (month, day) {
        (Some(m), Some(d)) => (to_jdn(from, year, m, d)?, true, true),
        (Some(m), None) => {
            let len = i64::from(days_in_month(from, year, m));
            (to_jdn(from, year, m, 1)? + len / 2, true, false)
        }
        (None, _) => {
            let start = to_jdn(from, year, 1, 1)?;
            let next = year_after(year);
            let end = to_jdn(from, next, 1, 1).unwrap_or(start + 365);
            (start + (end - start) / 2, false, false)
        }
    };
    let (y, m, d) = from_jdn(to, jdn)?;
    Some((
        y,
        keep_month.then_some(m),
        (keep_month && keep_day).then_some(d),
    ))
}

/// The year that follows, skipping the year 0 that no calendar here has.
const fn year_after(year: i32) -> i32 {
    if year == -1 { 1 } else { year + 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jdn(y: i32, m: u8, d: u8) -> i64 {
        to_jdn(Calendar::Gregorian, y, m, d).expect("a Gregorian date")
    }

    #[test]
    fn the_gregorian_axis_is_anchored_where_it_should_be() {
        // 1 January 1 CE (proleptic Gregorian) is JDN 1721426, the reference
        // every other conversion here is measured against.
        assert_eq!(jdn(1, 1, 1), 1_721_426);
        assert_eq!(from_jdn(Calendar::Gregorian, 1_721_426), Some((1, 1, 1)));
    }

    #[test]
    fn a_date_before_the_common_era_survives_the_round_trip() {
        // No year 0: the day before 1 Jan 1 CE is 31 Dec 1 BCE.
        assert_eq!(
            from_jdn(Calendar::Gregorian, jdn(1, 1, 1) - 1),
            Some((-1, 12, 31))
        );
        // The Ides of March, 44 BCE.
        let ides = to_jdn(Calendar::Julian, -44, 3, 15).expect("a Julian date");
        assert_eq!(from_jdn(Calendar::Julian, ides), Some((-44, 3, 15)));
    }

    /// The switch England and its colonies did not make until 1752: the Julian
    /// calendar runs ten days behind the Gregorian one in 1582.
    #[test]
    fn julian_and_gregorian_differ_by_their_accumulated_drift() {
        let j = to_jdn(Calendar::Julian, 1582, 3, 15).expect("a Julian date");
        assert_eq!(from_jdn(Calendar::Gregorian, j), Some((1582, 3, 25)));
        assert_eq!(
            convert(
                Calendar::Julian,
                Calendar::Gregorian,
                1582,
                Some(3),
                Some(15)
            ),
            Some((1582, Some(3), Some(25)))
        );
    }

    /// The same anchors `oxidgene-gedcom` pins `ged_io` to, so the editor's
    /// conversion and the server's `date_sort` cannot disagree.
    #[test]
    fn republican_dates_land_on_their_documented_gregorian_day() {
        for (rep, greg) in [
            // 1 Vendémiaire an I: the equinox the calendar was anchored to.
            ((1, 1, 1), (1792, 9, 22)),
            // 9 Thermidor an II — the fall of Robespierre.
            ((2, 11, 9), (1794, 7, 27)),
            // 18 Brumaire an VIII — Bonaparte's coup.
            ((8, 2, 18), (1799, 11, 9)),
            // The last new year the calendar saw, and the day after it.
            ((14, 1, 1), (1805, 9, 23)),
            ((14, 2, 2), (1805, 10, 24)),
        ] {
            let (ry, rm, rd) = rep;
            let (gy, gm, gd) = greg;
            let converted = to_jdn(Calendar::FrenchRepublican, ry, rm, rd)
                .and_then(|j| from_jdn(Calendar::Gregorian, j));
            assert_eq!(converted, Some((gy, gm as u8, gd as u8)), "for {rep:?}");
            assert_eq!(
                from_jdn(Calendar::FrenchRepublican, jdn(gy, gm as u8, gd as u8)),
                Some((ry, rm, rd)),
                "back again, for {greg:?}"
            );
        }
    }

    /// The case that prompted all this: a date typed in one calendar and then
    /// re-labelled as belonging to another has to be renumbered.
    #[test]
    fn switching_a_gregorian_date_to_republican_renumbers_it() {
        assert_eq!(
            convert(
                Calendar::Gregorian,
                Calendar::FrenchRepublican,
                1796,
                Some(3),
                Some(11)
            ),
            Some((4, Some(6), Some(21))), // 21 ventôse an IV
        );
    }

    #[test]
    fn the_complementary_days_close_the_republican_year() {
        // An III was sextile: it had a sixth jour complémentaire.
        assert_eq!(days_in_month(Calendar::FrenchRepublican, 3, 13), 6);
        assert_eq!(days_in_month(Calendar::FrenchRepublican, 4, 13), 5);
        // 1 Vendémiaire an IV is the day after the last of them.
        let last = to_jdn(Calendar::FrenchRepublican, 3, 13, 6).expect("6 comp. an III");
        assert_eq!(
            from_jdn(Calendar::FrenchRepublican, last + 1),
            Some((4, 1, 1))
        );
    }

    #[test]
    fn a_republican_date_needs_a_republic() {
        // Nothing before 22 September 1792 has a Republican date.
        assert_eq!(from_jdn(Calendar::FrenchRepublican, jdn(1792, 9, 21)), None);
        assert_eq!(
            convert(
                Calendar::Gregorian,
                Calendar::FrenchRepublican,
                1750,
                None,
                None
            ),
            None
        );
    }

    #[test]
    fn hebrew_dates_land_on_their_known_gregorian_day() {
        for (heb, greg) in [
            // Rosh Hashanah 5784 and 5785.
            ((5784, 1, 1), (2023, 9, 16)),
            ((5785, 1, 1), (2024, 10, 3)),
            // Pesach 5784: 15 Nisan.
            ((5784, 8, 15), (2024, 4, 23)),
        ] {
            let (hy, hm, hd) = heb;
            let (gy, gm, gd) = greg;
            let converted =
                to_jdn(Calendar::Hebrew, hy, hm, hd).and_then(|j| from_jdn(Calendar::Gregorian, j));
            assert_eq!(converted, Some((gy, gm, gd)), "for {heb:?}");
            assert_eq!(
                from_jdn(Calendar::Hebrew, jdn(gy, gm, gd)),
                Some((hy, hm, hd)),
                "back again, for {greg:?}"
            );
        }
    }

    /// Adar II exists only in a leap year, and the year is always one of the
    /// six lengths the dehiyyot allow.
    #[test]
    fn the_hebrew_year_takes_one_of_six_shapes() {
        for year in 5700..5800 {
            let length = hebrew_year_length(year);
            let leap = hebrew_leap(year);
            assert!(
                matches!((leap, length), (false, 353..=355) | (true, 383..=385)),
                "year {year} is {length} days, leap={leap}"
            );
            let summed: i64 = (1..=13)
                .map(|m| i64::from(days_in_month(Calendar::Hebrew, year as i32, m)))
                .sum();
            assert_eq!(summed, length, "months of year {year} must fill it");
            assert_eq!(
                days_in_month(Calendar::Hebrew, year as i32, 7) > 0,
                leap,
                "Adar II exists only in a leap year ({year})"
            );
        }
    }

    /// A year on its own names a period, and comes back as the year that period
    /// mostly overlaps — not the one its first day grazes.
    #[test]
    fn a_year_alone_converts_to_the_year_it_overlaps() {
        assert_eq!(
            convert(Calendar::Gregorian, Calendar::Julian, 1900, None, None),
            Some((1900, None, None))
        );
        // An IV ran from September 1795 to September 1796.
        assert_eq!(
            convert(
                Calendar::FrenchRepublican,
                Calendar::Gregorian,
                4,
                None,
                None
            ),
            Some((1796, None, None))
        );
    }

    #[test]
    fn a_month_alone_keeps_its_precision() {
        let (y, m, d) = convert(
            Calendar::Gregorian,
            Calendar::FrenchRepublican,
            1796,
            Some(3),
            None,
        )
        .expect("March 1796 is under the Republic");
        assert_eq!((y, m), (4, Some(6))); // ventôse an IV
        assert_eq!(d, None);
    }

    #[test]
    fn converting_to_the_same_calendar_changes_nothing() {
        assert_eq!(
            convert(
                Calendar::Gregorian,
                Calendar::Gregorian,
                1947,
                Some(2),
                Some(23)
            ),
            Some((1947, Some(2), Some(23)))
        );
    }

    #[test]
    fn a_day_that_never_existed_has_no_place_on_the_axis() {
        assert_eq!(to_jdn(Calendar::Gregorian, 1900, 2, 30), None);
        assert_eq!(to_jdn(Calendar::Gregorian, 1900, 2, 29), None); // not a leap year
        assert!(to_jdn(Calendar::Julian, 1900, 2, 29).is_some()); // but it is a Julian one
        assert_eq!(to_jdn(Calendar::FrenchRepublican, 4, 13, 6), None);
        assert_eq!(to_jdn(Calendar::Hebrew, 5783, 7, 1), None); // no Adar II in 5783
    }

    /// Every day of a long stretch has to survive being written down and read
    /// back in each calendar — the only real proof the two directions agree.
    #[test]
    fn every_day_round_trips_through_every_calendar() {
        let start = jdn(1780, 1, 1);
        let end = jdn(1830, 1, 1);
        for jdn in start..end {
            for calendar in [
                Calendar::Gregorian,
                Calendar::Julian,
                Calendar::Hebrew,
                Calendar::FrenchRepublican,
            ] {
                let Some((y, m, d)) = from_jdn(calendar, jdn) else {
                    continue;
                };
                assert_eq!(
                    to_jdn(calendar, y, m, d),
                    Some(jdn),
                    "{calendar} {y}-{m}-{d} (jdn {jdn})"
                );
            }
        }
    }
}
