//! Reusable date input widget.
//!
//! Every date in the app is edited through the same control: a calendar
//! selector (Gregorian, Julian, …), a precision qualifier (Exact, About, …)
//! and three numeric fields (day / month / year). A localized literal preview
//! is shown under the fields (e.g. « vers 1947 »). Qualifiers `Or` and
//! `Between` expose a second set of date fields.
//!
//! [`DateQualifier::FromAge`] is the odd one out: it is an *entry mode*, not a
//! stored qualifier. It swaps the day/month/year triplet for an age and the
//! year that age was observed in, and the date it stands for — `About
//! <year − age>` — is what gets saved. See [`DateParts::resolved`].
//!
//! [`format_date`] is the read-side counterpart: it turns the columns an event
//! carries back into the same localized phrase the editor previewed, so a date
//! reads identically wherever it is shown.

use chrono::{Datelike, NaiveDate};
use dioxus::html::input_data::keyboard_types::Key;
use dioxus::prelude::*;
use oxidgene_core::calendar::{convert as convert_components, days_in_month, months_in_year};
use oxidgene_core::enums::{Calendar, DateQualifier};
use oxidgene_core::types::Event as DomainEvent;

use crate::i18n::I18n;

// ── Month vocabularies ────────────────────────────────────────────────────
//
// Each calendar names its months differently, and GEDCOM expects the names of
// the calendar the date was recorded in — `@#DFRENCH R@ 2 BRUM 14`, never `2
// FEB 14`. Writing Gregorian abbreviations under a Republican escape produces
// a date no reader can take back.

/// GEDCOM month abbreviations (canonical storage form), shared by the
/// Gregorian and Julian calendars.
const GREGORIAN_MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// Tishrei through Elul. The thirteenth, Adar II, only falls in a leap year.
const HEBREW_MONTHS: [&str; 13] = [
    "TSH", "CSH", "KSL", "TVT", "SHV", "ADR", "ADS", "NSN", "IYR", "SVN", "TMZ", "AAV", "ELL",
];

/// Vendémiaire through Fructidor, then `COMP` for the five or six jours
/// complémentaires that close the Republican year.
const REPUBLICAN_MONTHS: [&str; 13] = [
    "VEND", "BRUM", "FRIM", "NIVO", "PLUV", "VENT", "GERM", "FLOR", "PRAI", "MESS", "THER", "FRUC",
    "COMP",
];

fn month_names(calendar: Calendar) -> &'static [&'static str] {
    match calendar {
        Calendar::Gregorian | Calendar::Julian => &GREGORIAN_MONTHS[..],
        Calendar::Hebrew => &HEBREW_MONTHS[..],
        Calendar::FrenchRepublican => &REPUBLICAN_MONTHS[..],
    }
}

/// Whether a month is picked from a list rather than typed as a number.
///
/// "3" is a month everyone can read in the calendar they live by, and typing it
/// is faster than opening a list. In a calendar nobody counts in it is just a
/// number: nothing on the screen says whether it means frimaire or ventôse, and
/// there is no reason to expect the reader to know the order by heart.
fn uses_named_months(calendar: Calendar) -> bool {
    !matches!(calendar, Calendar::Gregorian | Calendar::Julian)
}

/// The months to offer for a year, in order.
///
/// A year does not always have all of them: the Hebrew Adar II falls only in a
/// leap year, so offering it in a common one invites a date that never existed.
/// The month already entered is always kept in the list, whatever the year says
/// — a list that silently drops the selected value leaves the control showing
/// blank over a month the date still carries, and lying about the data is worse
/// than showing an impossible month that [`DateParts::validate`] explains.
///
/// Without a year nothing can be ruled out, so everything is offered.
fn month_options(calendar: Calendar, year: Option<i32>, current: Option<u8>) -> Vec<u8> {
    (1..=months_in_year(calendar))
        .filter(|m| current == Some(*m) || year.is_none_or(|y| days_in_month(calendar, y, *m) > 0))
        .collect()
}

/// i18n key for a month's readable name in its own calendar.
fn month_label_key(calendar: Calendar, month: u8) -> String {
    match calendar {
        Calendar::Gregorian | Calendar::Julian => format!("date.month.{month}"),
        Calendar::Hebrew => format!("date.month.hebrew.{month}"),
        Calendar::FrenchRepublican => format!("date.month.republican.{month}"),
    }
}

/// Plausible year window. It reaches back well past the first dynasties of
/// Egypt — a tree may perfectly well start there — and stops just short enough
/// on both sides to catch a slipped keystroke like "19477".
///
/// Year 0 is excluded: there is none. 1 BCE is followed by 1 CE.
const MIN_YEAR: i32 = -9999;
const MAX_YEAR: i32 = 2999;

/// That same window, counted the way each calendar counts it.
///
/// A calendar with its own era needs its own numbers for the same span of
/// history, or every date it can express is turned away: the Hebrew year 5784
/// is 2023, not a slip for 578, and an XIV is 1805 rather than antiquity.
fn year_bounds(calendar: Calendar) -> std::ops::RangeInclusive<i32> {
    match calendar {
        Calendar::Gregorian | Calendar::Julian => MIN_YEAR..=MAX_YEAR,
        // Year 1 opens in 3761 BCE; 6800 lands just past [`MAX_YEAR`].
        Calendar::Hebrew => 1..=6800,
        // An I opens in 1792; 1210 lands just past [`MAX_YEAR`].
        Calendar::FrenchRepublican => 1..=1210,
    }
}

/// Ceiling on an age entered in `FromAge` mode. The verified human record is
/// 122 years; anything past this is a typo, not a centenarian.
const MAX_AGE: u32 = 130;

/// A date broken into editable components.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct DateParts {
    pub calendar: Calendar,
    pub qualifier: DateQualifier,
    pub year: Option<i32>,
    pub month: Option<u8>,
    pub day: Option<u8>,
    pub year2: Option<i32>,
    pub month2: Option<u8>,
    pub day2: Option<u8>,
    /// `FromAge` only: the age observed, and the year it was observed in.
    /// Neither is persisted — [`DateParts::resolved`] turns the pair into the
    /// `About` date it stands for, and that is what every accessor reports.
    pub age: Option<u32>,
    pub age_ref_year: Option<i32>,
}

impl DateParts {
    /// Build from persisted event fields (`date_value` / `date_value2` are the
    /// free-text GEDCOM phrases).
    pub fn from_fields(
        calendar: Calendar,
        qualifier: DateQualifier,
        date_value: Option<&str>,
        date_value2: Option<&str>,
    ) -> Self {
        let (year, month, day) = date_value
            .map(|v| parse_components(calendar, v))
            .unwrap_or_default();
        let (year2, month2, day2) = date_value2
            .map(|v| parse_components(calendar, v))
            .unwrap_or_default();
        Self {
            calendar,
            // `FromAge` is an entry mode we never write, so a stored row can
            // only carry it from before that was true. It reads back as the
            // `About` date it always stood for.
            qualifier: match qualifier {
                DateQualifier::FromAge => DateQualifier::About,
                other => other,
            },
            year,
            month,
            day,
            year2,
            month2,
            day2,
            age: None,
            age_ref_year: None,
        }
    }

    /// The same date, renumbered in another calendar.
    ///
    /// Changing the calendar says what a date *is*, not what it should become:
    /// « 11 mars 1796 » relabelled Republican is 21 ventôse an IV, not
    /// 11 frimaire an 1796 — which is a different day entirely, three years
    /// later. Every date the widget holds moves together, both ends of a range
    /// included, so a `Between` never ends up with one foot in each calendar.
    ///
    /// A date the target calendar cannot express — anything before the Republic
    /// for the Republican one — is left exactly as typed. Better to hand back
    /// what the user wrote and let [`DateParts::validate`] have its say than to
    /// replace it with a date we made up.
    pub fn in_calendar(&self, to: Calendar) -> Self {
        if to == self.calendar {
            return *self;
        }
        let renumber = |year: Option<i32>, month: Option<u8>, day: Option<u8>| {
            let Some(y) = year else {
                return (year, month, day);
            };
            match convert_components(self.calendar, to, y, month, day) {
                Some((y, m, d)) => (Some(y), m, d),
                None => (year, month, day),
            }
        };
        let (year, month, day) = renumber(self.year, self.month, self.day);
        let (year2, month2, day2) = renumber(self.year2, self.month2, self.day2);
        // An age is a count of years, which every calendar here measures the
        // same way; only the year it was observed in is calendar-bound.
        let (age_ref_year, _, _) = renumber(self.age_ref_year, None, None);
        Self {
            calendar: to,
            year,
            month,
            day,
            year2,
            month2,
            day2,
            age_ref_year,
            ..*self
        }
    }

    /// The date actually saved. Every qualifier but [`DateQualifier::FromAge`]
    /// resolves to itself; `FromAge` collapses into `About <year − age>`,
    /// because neither GEDCOM, GeneWeb, nor our own schema has a way to record
    /// "aged 14 in 2026" — only the birth year it implies.
    ///
    /// An incomplete pair resolves to no date at all, so picking the mode and
    /// typing nothing leaves the event dateless rather than half-dated.
    pub fn resolved(&self) -> Self {
        if self.qualifier != DateQualifier::FromAge {
            return *self;
        }
        let year = match (self.age, self.age_ref_year) {
            (Some(age), Some(ref_year)) => Some(ref_year - age as i32),
            _ => None,
        };
        Self {
            qualifier: if year.is_some() {
                DateQualifier::About
            } else {
                DateQualifier::Exact
            },
            year,
            month: None,
            day: None,
            year2: None,
            month2: None,
            day2: None,
            ..*self
        }
    }

    /// The qualifier to persist — see [`DateParts::resolved`].
    pub fn stored_qualifier(&self) -> DateQualifier {
        self.resolved().qualifier
    }

    /// Whether the age/reference-year pair replaces the day/month/year fields.
    pub fn is_from_age(&self) -> bool {
        self.qualifier == DateQualifier::FromAge
    }

    pub fn is_empty(&self) -> bool {
        let r = self.resolved();
        r.year.is_none() && r.month.is_none() && r.day.is_none()
    }

    pub fn needs_second_date(&self) -> bool {
        self.qualifier.needs_second_date()
    }

    /// Returns an i18n error key when the entry is not a date.
    ///
    /// Beyond the structural rules (a year is mandatory as soon as anything is
    /// entered, a day needs a month) this rejects triplets that name a day
    /// which never existed — 30 February is a typo, not a date — and ranges
    /// that run backwards. In `FromAge` mode an age only means something
    /// alongside the year it was observed in, so the pair must be filled in
    /// fully or not at all.
    pub fn validate(&self) -> Option<&'static str> {
        if self.is_from_age() {
            return match (self.age, self.age_ref_year) {
                (Some(_), None) => Some("date.error.age_year_required"),
                (None, Some(_)) => Some("date.error.age_required"),
                (Some(age), _) if age > MAX_AGE => Some("date.error.age_out_of_range"),
                (_, Some(y)) if !year_bounds(self.calendar).contains(&y) => {
                    Some("date.error.year_out_of_range")
                }
                _ => None,
            };
        }
        if let Some(key) = validate_triplet(self.calendar, self.year, self.month, self.day) {
            return Some(key);
        }
        if self.needs_second_date() {
            if self.year2.is_none() {
                return Some("date.error.year_required");
            }
            if let Some(key) = validate_triplet(self.calendar, self.year2, self.month2, self.day2) {
                return Some(key);
            }
            // `Or` offers two candidates, so their order says nothing; a
            // `Between` range that runs backwards is an entry mistake.
            if self.qualifier == DateQualifier::Between {
                let a = sort_date(self.year, self.month, self.day);
                let b = sort_date(self.year2, self.month2, self.day2);
                if matches!((a, b), (Some(a), Some(b)) if a > b) {
                    return Some("date.error.range_out_of_order");
                }
            }
        }
        None
    }

    /// Canonical `date_value` (GEDCOM-style: `23 FEB 1947` / `FEB 1947` / `1947`).
    pub fn date_value(&self) -> Option<String> {
        let r = self.resolved();
        format_value(r.calendar, r.year, r.month, r.day)
    }

    /// Canonical `date_value2` for the `Or` / `Between` second date.
    pub fn date_value2(&self) -> Option<String> {
        let r = self.resolved();
        if r.needs_second_date() {
            format_value(r.calendar, r.year2, r.month2, r.day2)
        } else {
            None
        }
    }

    /// Localized preview of the date as it will be saved and later displayed
    /// (e.g. « vers 2012 » for an age of 14 observed in 2026).
    ///
    /// Built from the canonical values rather than the raw fields so the
    /// preview and [`format_date`] can never disagree.
    pub fn literal(&self, i18n: &I18n) -> String {
        format_date(
            i18n,
            self.calendar,
            self.stored_qualifier(),
            self.date_value().as_deref(),
            self.date_value2().as_deref(),
        )
    }
}

/// The localized word a qualifier reads as in front of its date (« vers
/// 1850 »), or `None` for the ones that carry no prefix.
fn qualifier_prefix_key(q: DateQualifier) -> Option<&'static str> {
    match q {
        DateQualifier::Exact => None,
        DateQualifier::About => Some("date.prefix.about"),
        DateQualifier::Calculated => Some("date.prefix.calculated"),
        DateQualifier::Estimated => Some("date.prefix.estimated"),
        DateQualifier::Perhaps => Some("date.prefix.perhaps"),
        DateQualifier::Before => Some("date.prefix.before"),
        DateQualifier::After => Some("date.prefix.after"),
        DateQualifier::Between => Some("date.prefix.between"),
        // « 1800 ou 1810 » — the conjunction alone carries the meaning.
        DateQualifier::Or => None,
        // Never stored; `resolved` has already turned it into `About`. Shown
        // bare rather than guessed at should one survive in an old row.
        DateQualifier::FromAge => None,
    }
}

/// A stored date value rendered in the reader's language (« 23 févr. 1947 »).
///
/// Anything that does not parse — a free-text phrase kept verbatim on import —
/// is passed through untouched rather than dropped.
fn literal_value(i18n: &I18n, calendar: Calendar, raw: &str) -> String {
    let (year, month, day) = parse_components(calendar, raw);
    match literal_components(i18n, calendar, year, month, day) {
        s if s.is_empty() => raw.trim().to_string(),
        s => s,
    }
}

/// The localized phrase for an event's date columns — « vers 2012 », « avant
/// 3 janv. 1900 », « entre 1800 et 1810 ».
///
/// This is the one place a date becomes text for the reader; every view calls
/// it so the same event never reads two different ways.
pub fn format_date(
    i18n: &I18n,
    calendar: Calendar,
    qualifier: DateQualifier,
    value: Option<&str>,
    value2: Option<&str>,
) -> String {
    let first = value
        .map(|v| literal_value(i18n, calendar, v))
        .unwrap_or_default();
    let second = value2
        .filter(|_| qualifier.needs_second_date())
        .map(|v| literal_value(i18n, calendar, v))
        .unwrap_or_default();

    let body = match (first.is_empty(), second.is_empty()) {
        (true, true) => return String::new(),
        (false, true) => first,
        (true, false) => second,
        (false, false) => {
            let sep = if qualifier == DateQualifier::Or {
                i18n.t("person_form.date2_label_or")
            } else {
                i18n.t("person_form.date2_label_between")
            };
            format!("{first} {sep} {second}")
        }
    };

    match qualifier_prefix_key(qualifier) {
        Some(key) => format!("{} {body}", i18n.t(key)),
        None => body,
    }
}

/// [`format_date`] over an event's own columns — the form every view but the
/// editor needs. Empty when the event carries no date.
pub fn format_event_date(i18n: &I18n, event: &DomainEvent) -> String {
    format_date(
        i18n,
        event.calendar,
        event.date_qualifier,
        event.date_value.as_deref(),
        event.date_value2.as_deref(),
    )
}

/// Parse a free-text date into `(year, month, day)`.
///
/// Accepts ISO (`1947-02-23`, `1947-02`, `1947`), a bare year, and GEDCOM-style
/// phrases (`23 FEB 1947`, `FEB 1947`). A year before the common era may be
/// written either the GEDCOM way (`3000 BCE`, also `BC` / `B.C.`) or with a
/// leading minus, and comes back negative.
fn parse_components(calendar: Calendar, s: &str) -> (Option<i32>, Option<u8>, Option<u8>) {
    let s = s.trim();
    if s.is_empty() {
        return (None, None, None);
    }

    // Strip the era marker up front so the rest can read the year as a plain
    // number, then apply the sign to whatever it found.
    let (s, bce) = strip_bce(s);
    if bce {
        let (y, m, d) = parse_components(calendar, s);
        return (y.map(|y| -y.abs()), m, d);
    }
    if let Some(rest) = s.strip_prefix('-') {
        let (y, m, d) = parse_components(calendar, rest);
        return (y.map(|y| -y.abs()), m, d);
    }
    // ISO-ish: digits and dashes only.
    if s.contains('-') && s.chars().all(|c| c.is_ascii_digit() || c == '-') {
        let mut it = s.split('-');
        let year = it.next().and_then(|p| p.parse::<i32>().ok());
        let month = it
            .next()
            .and_then(|p| p.parse::<u8>().ok())
            .filter(|m| (1..=12).contains(m));
        let day = it
            .next()
            .and_then(|p| p.parse::<u8>().ok())
            .filter(|d| (1..=31).contains(d));
        return (year, month, day);
    }
    // Bare year.
    if let Ok(y) = s.parse::<i32>() {
        return (Some(y), None, None);
    }
    // Token mode: « 23 FEB 1947 », « 2 BRUM 14 », « 15 TSH 5784 ».
    let mut year = None;
    let mut month = None;
    let mut day = None;
    for tok in s.split_whitespace() {
        let up = tok.to_ascii_uppercase();
        if let Some(m) = month_names(calendar).iter().position(|&g| g == up) {
            month = Some((m + 1) as u8);
        } else if let Ok(n) = tok.parse::<i32>() {
            if tok.len() >= 4 {
                year = Some(n);
            } else if day.is_none() && (1..=31).contains(&n) {
                day = Some(n as u8);
            } else if year.is_none() {
                year = Some(n);
            }
        }
    }
    (year, month, day)
}

/// Splits a trailing era marker off a date, in any of the spellings GEDCOM
/// readers use. Returns the date without it, and whether one was there.
fn strip_bce(s: &str) -> (&str, bool) {
    for marker in ["B.C.E.", "BCE", "B.C.", "BC"] {
        if s.len() > marker.len() {
            let (head, tail) = s.split_at(s.len() - marker.len());
            if tail.eq_ignore_ascii_case(marker) {
                return (head.trim_end(), true);
            }
        }
    }
    (s, false)
}

/// Canonical storage string for a component triplet (year mandatory).
///
/// Years before the common era are written the way GEDCOM wants them —
/// `15 MAR 44 BCE`, not `-44` — so an export stays readable by other software.
fn format_value(
    calendar: Calendar,
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
) -> Option<String> {
    let y = year?;
    let names = month_names(calendar);
    let name = month
        .filter(|m| *m >= 1 && usize::from(*m) <= names.len())
        .map(|m| names[usize::from(m) - 1]);
    let day = day.filter(|d| (1..=31).contains(d));
    let era = if y < 0 { " BCE" } else { "" };
    let y = y.abs();
    Some(match (day, name) {
        (Some(d), Some(name)) => format!("{d} {name} {y}{era}"),
        (None, Some(name)) => format!("{name} {y}{era}"),
        _ => format!("{y}{era}"),
    })
}

/// Checks one day/month/year triplet, from "is anything even here" through to
/// "did that day exist". Returns an i18n error key, or `None` when it holds up.
fn validate_triplet(
    calendar: Calendar,
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
) -> Option<&'static str> {
    let Some(y) = year else {
        return (month.is_some() || day.is_some()).then_some("date.error.year_required");
    };
    if y == 0 || !year_bounds(calendar).contains(&y) {
        return Some("date.error.year_out_of_range");
    }
    if day.is_some() && month.is_none() {
        return Some("date.error.day_requires_month");
    }
    let m = month?;
    // Out of the calendar's range, or a month that year does not have: Adar II
    // is the thirteenth month of a Hebrew leap year and of no other, so
    // « 5783 ADS » names a month that never came round.
    if m < 1 || m > months_in_year(calendar) || days_in_month(calendar, y, m) == 0 {
        return Some("date.error.month_out_of_range");
    }
    let d = day?;
    if d < 1 || d > days_in_month(calendar, y, m) {
        return Some("date.error.invalid_date");
    }
    None
}

/// A comparable date, used only to tell whether a `Between` range runs
/// backwards.
///
/// Deliberately *not* the event's `date_sort` column, which the API derives
/// for itself — normalising a Julian or Republican date onto the Gregorian
/// calendar needs `ged_io`, which a WASM frontend cannot reach. Ordering the
/// two ends of one range is a weaker question: both are in the same calendar,
/// so reading their components at face value ranks them correctly with no
/// conversion at all.
fn sort_date(year: Option<i32>, month: Option<u8>, day: Option<u8>) -> Option<NaiveDate> {
    let y = year?;
    let m = month.filter(|m| (1..=12).contains(m)).unwrap_or(1) as u32;
    let d = day.filter(|d| (1..=31).contains(d)).unwrap_or(1) as u32;
    NaiveDate::from_ymd_opt(y, m, d)
}

/// Localized literal for a component triplet (empty when no year).
fn literal_components(
    i18n: &I18n,
    calendar: Calendar,
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
) -> String {
    let Some(y) = year else { return String::new() };
    let name = month
        .filter(|m| *m >= 1 && *m <= months_in_year(calendar))
        .map(|m| i18n.t(&month_label_key(calendar, m)));
    let day = day.filter(|d| (1..=31).contains(d));
    let era = if y < 0 {
        format!(" {}", i18n.t("date.bce"))
    } else {
        String::new()
    };
    let y = y.abs();
    match (day, name) {
        (Some(d), Some(name)) => format!("{d} {name} {y}{era}"),
        (None, Some(name)) => format!("{name} {y}{era}"),
        _ => format!("{y}{era}"),
    }
}

/// PascalCase value used by the qualifier `<select>` options.
pub fn qualifier_value(q: DateQualifier) -> &'static str {
    match q {
        DateQualifier::Exact => "Exact",
        DateQualifier::About => "About",
        DateQualifier::Calculated => "Calculated",
        DateQualifier::Estimated => "Estimated",
        DateQualifier::Perhaps => "Perhaps",
        DateQualifier::Before => "Before",
        DateQualifier::After => "After",
        DateQualifier::Or => "Or",
        DateQualifier::Between => "Between",
        DateQualifier::FromAge => "FromAge",
    }
}

/// PascalCase value used by the calendar `<select>` options.
pub fn calendar_value(c: Calendar) -> &'static str {
    match c {
        Calendar::Gregorian => "Gregorian",
        Calendar::Julian => "Julian",
        Calendar::Hebrew => "Hebrew",
        Calendar::FrenchRepublican => "FrenchRepublican",
    }
}

/// Inverse of [`qualifier_value`]. Unknown values fall back to the default
/// rather than erroring: the only producer is our own `<option>` list.
fn qualifier_from_value(s: &str) -> DateQualifier {
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

/// Inverse of [`calendar_value`].
fn calendar_from_value(s: &str) -> Calendar {
    match s {
        "Julian" => Calendar::Julian,
        "Hebrew" => Calendar::Hebrew,
        "FrenchRepublican" => Calendar::FrenchRepublican,
        _ => Calendar::Gregorian,
    }
}

pub fn qualifier_options(i18n: &I18n) -> Element {
    let keys = [
        ("Exact", "exact"),
        ("About", "about"),
        ("Calculated", "calculated"),
        ("Estimated", "estimated"),
        ("Perhaps", "perhaps"),
        ("Before", "before"),
        ("After", "after"),
        ("Or", "or"),
        ("Between", "between"),
        ("FromAge", "from_age"),
    ];
    rsx! {
        for (value, key) in keys {
            option {
                value: "{value}",
                {i18n.t(&format!("date_qualifier.{key}"))}
            }
        }
    }
}

pub fn calendar_options(i18n: &I18n) -> Element {
    let keys = [
        ("Gregorian", "gregorian"),
        ("Julian", "julian"),
        ("Hebrew", "hebrew"),
        ("FrenchRepublican", "french_republican"),
    ];
    rsx! {
        for (value, key) in keys {
            option {
                value: "{value}",
                {i18n.t(&format!("calendar.{key}"))}
            }
        }
    }
}

fn opt_num<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

/// Everything but the digits, dropped.
///
/// The keystroke guard already turns away typed letters; this catches what it
/// cannot see — a paste, a drop, an IME commit — so a stray character is never
/// read as part of the number.
fn digits_only(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// A year field, where a leading minus is how you say "before the common era".
fn parse_year(s: &str) -> Option<i32> {
    let negative = s.trim_start().starts_with('-');
    let t = digits_only(s);
    if t.is_empty() {
        return None;
    }
    let y: i32 = t.parse().ok()?;
    Some(if negative { -y } else { y })
}

fn parse_u8(s: &str) -> Option<u8> {
    let t = digits_only(s);
    if t.is_empty() { None } else { t.parse().ok() }
}

fn parse_u32(s: &str) -> Option<u32> {
    let t = digits_only(s);
    if t.is_empty() { None } else { t.parse().ok() }
}

/// Turns away any keystroke that would put a non-digit in a date field, while
/// letting editing keys (arrows, Backspace, Tab) and clipboard shortcuts past.
///
/// `allow_sign` opens the door to a minus in the year fields, which is how a
/// BCE year is entered.
fn numeric_keydown(e: &Event<KeyboardData>, allow_sign: bool) {
    let Key::Character(typed) = e.key() else {
        return;
    };
    if e.modifiers().ctrl() || e.modifiers().meta() {
        return;
    }
    let ok = typed
        .chars()
        .all(|c| c.is_ascii_digit() || (allow_sign && c == '-'));
    if !ok {
        e.prevent_default();
    }
}

/// The age and the year it was observed in, shown in place of the day/month/
/// year triplet while the `FromAge` mode is selected.
fn age_inputs(mut parts: Signal<DateParts>, i18n: I18n, on_change: EventHandler<()>) -> Element {
    let p = parts();
    rsx! {
        input {
            class: "pf-date-part pf-date-age",
            r#type: "text",
            inputmode: "numeric",
            maxlength: 3,
            placeholder: "{i18n.t(\"date.ph_age\")}",
            value: opt_num(p.age),
            onkeydown: |e| numeric_keydown(&e, false),
            oninput: move |e| {
                let mut np = parts();
                np.age = parse_u32(&e.value());
                parts.set(np);
                on_change.call(());
            },
        }
        span { class: "pf-date-separator", {i18n.t("date.age_in")} }
        input {
            class: "pf-date-part pf-date-yyyy",
            r#type: "text",
            inputmode: "numeric",
            maxlength: 5,
            placeholder: "{i18n.t(\"date.ph_year\")}",
            value: opt_num(p.age_ref_year),
            onkeydown: |e| numeric_keydown(&e, true),
            oninput: move |e| {
                let mut np = parts();
                np.age_ref_year = parse_year(&e.value());
                parts.set(np);
                on_change.call(());
            },
        }
    }
}

/// The day / month / year triplet (or its `Or` / `Between` counterpart).
fn part_inputs(
    mut parts: Signal<DateParts>,
    second: bool,
    i18n: I18n,
    on_change: EventHandler<()>,
) -> Element {
    let p = parts();
    let (d, m, y) = if second {
        (p.day2, p.month2, p.year2)
    } else {
        (p.day, p.month, p.year)
    };
    rsx! {
        input {
            class: "pf-date-part pf-date-dd",
            r#type: "text",
            inputmode: "numeric",
            maxlength: 2,
            placeholder: "{i18n.t(\"date.ph_day\")}",
            value: opt_num(d),
            onkeydown: |e| numeric_keydown(&e, false),
            oninput: move |e| {
                let mut np = parts();
                // Out-of-range values are kept, not clamped away: `validate`
                // says what is wrong with them, where silently blanking the
                // field would just look like the app eating keystrokes.
                let v = parse_u8(&e.value());
                if second { np.day2 = v; } else { np.day = v; }
                parts.set(np);
                on_change.call(());
            },
        }
        if uses_named_months(p.calendar) {
            // Named months are chosen, not typed: see `uses_named_months`.
            select {
                class: "pf-date-month-select",
                onchange: move |e| {
                    let mut np = parts();
                    let v = parse_u8(&e.value());
                    if second { np.month2 = v; } else { np.month = v; }
                    parts.set(np);
                    on_change.call(());
                },
                // Bound through `selected` on each option rather than `value`
                // on the select: the list is built by a loop, so it is
                // appended *after* the element's own attributes are applied,
                // and a value set on a select that has no options yet selects
                // nothing at all — which is how a converted date came back
                // showing "MM" over a month it knew perfectly well.
                //
                // The empty entry carries no word: a date known to the year
                // alone is ordinary — « an VI », with no month in the record —
                // and blank says that better than any label could. It holds a
                // non-breaking space only so the row keeps its height and stays
                // clickable, an empty <option> collapsing to nothing in some
                // engines.
                option { value: "", selected: m.is_none(), "\u{00A0}" }
                for idx in month_options(p.calendar, y, m) {
                    option {
                        value: "{idx}",
                        selected: m == Some(idx),
                        {i18n.t(&month_label_key(p.calendar, idx))}
                    }
                }
            }
        } else {
            input {
                class: "pf-date-part pf-date-mm",
                r#type: "text",
                inputmode: "numeric",
                maxlength: 2,
                placeholder: "{i18n.t(\"date.ph_month\")}",
                value: opt_num(m),
                onkeydown: |e| numeric_keydown(&e, false),
                oninput: move |e| {
                    let mut np = parts();
                    let v = parse_u8(&e.value());
                    if second { np.month2 = v; } else { np.month = v; }
                    parts.set(np);
                    on_change.call(());
                },
            }
        }
        input {
            class: "pf-date-part pf-date-yyyy",
            r#type: "text",
            inputmode: "numeric",
            // Five, so a BCE year still fits with its minus sign.
            maxlength: 5,
            placeholder: "{i18n.t(\"date.ph_year\")}",
            value: opt_num(y),
            onkeydown: |e| numeric_keydown(&e, true),
            oninput: move |e| {
                let mut np = parts();
                let v = parse_year(&e.value());
                if second { np.year2 = v; } else { np.year = v; }
                parts.set(np);
                on_change.call(());
            },
        }
    }
}

/// Date editor: calendar + qualifier + day/month/year (+ optional second date),
/// with a localized literal preview underneath.
#[component]
pub fn DateInput(
    parts: Signal<DateParts>,
    i18n: I18n,
    /// Fired on every edit, so the host form can flag itself dirty.
    on_change: EventHandler<()>,
) -> Element {
    let mut parts = parts;
    let p = parts();
    let literal = p.literal(&i18n);
    let sep = if p.qualifier == DateQualifier::Or {
        i18n.t("person_form.date2_label_or")
    } else {
        i18n.t("person_form.date2_label_between")
    };
    rsx! {
        div { class: "pf-date-widget",
            div { class: "pf-date-row",
                select {
                    class: "pf-date-calendar",
                    value: calendar_value(p.calendar),
                    onchange: move |e| {
                        // The date already entered is re-expressed, not
                        // relabelled — see `DateParts::in_calendar`.
                        let np = parts().in_calendar(calendar_from_value(&e.value()));
                        parts.set(np);
                        on_change.call(());
                    },
                    {calendar_options(&i18n)}
                }
                select {
                    class: "pf-date-qualifier-select",
                    value: qualifier_value(p.qualifier),
                    onchange: move |e| {
                        let mut np = parts();
                        np.qualifier = qualifier_from_value(&e.value());
                        // An age is nearly always one observed today (a census
                        // return, a living person), so offer this year rather
                        // than an empty field the user has to fill every time.
                        if np.is_from_age() && np.age_ref_year.is_none() {
                            np.age_ref_year = Some(chrono::Local::now().year());
                        }
                        parts.set(np);
                        on_change.call(());
                    },
                    {qualifier_options(&i18n)}
                }
                if p.is_from_age() {
                    {age_inputs(parts, i18n, on_change)}
                } else {
                    {part_inputs(parts, false, i18n, on_change)}
                    if p.needs_second_date() {
                        span { class: "pf-date-separator", "{sep}" }
                        {part_inputs(parts, true, i18n, on_change)}
                    }
                }
            }
            // The error takes the literal's place: with the entry broken there
            // is nothing honest to preview, and saying why beats a blank line.
            if let Some(key) = p.validate() {
                div { class: "pf-date-error", {i18n.t(key)} }
            } else if !literal.is_empty() {
                div { class: "pf-date-literal", "{literal}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    fn en() -> I18n {
        I18n(Language::En)
    }

    fn fr() -> I18n {
        I18n(Language::Fr)
    }

    /// Most tests only care about the common calendar.
    fn greg(s: &str) -> (Option<i32>, Option<u8>, Option<u8>) {
        parse_components(Calendar::Gregorian, s)
    }

    #[test]
    fn parse_iso_full() {
        assert_eq!(greg("1947-02-23"), (Some(1947), Some(2), Some(23)));
    }

    #[test]
    fn parse_iso_year_month() {
        assert_eq!(greg("1947-02"), (Some(1947), Some(2), None));
    }

    #[test]
    fn parse_bare_year() {
        assert_eq!(greg("1947"), (Some(1947), None, None));
    }

    #[test]
    fn parse_gedcom_full() {
        assert_eq!(greg("23 FEB 1947"), (Some(1947), Some(2), Some(23)));
    }

    #[test]
    fn parse_gedcom_month_year() {
        assert_eq!(greg("FEB 1947"), (Some(1947), Some(2), None));
    }

    #[test]
    fn parse_empty() {
        assert_eq!(greg(""), (None, None, None));
        assert_eq!(greg("   "), (None, None, None));
    }

    #[test]
    fn from_fields_roundtrip() {
        let p = DateParts::from_fields(
            Calendar::Gregorian,
            DateQualifier::Exact,
            Some("23 FEB 1947"),
            None,
        );
        assert_eq!(p.year, Some(1947));
        assert_eq!(p.month, Some(2));
        assert_eq!(p.day, Some(23));
        assert_eq!(p.date_value().as_deref(), Some("23 FEB 1947"));
    }

    #[test]
    fn date_value_partial() {
        let mut p = DateParts {
            year: Some(1947),
            ..Default::default()
        };
        assert_eq!(p.date_value().as_deref(), Some("1947"));
        p.month = Some(2);
        assert_eq!(p.date_value().as_deref(), Some("FEB 1947"));
    }

    /// Only used to order the two ends of a `Between` range, so a partial
    /// date takes the first of its period.
    #[test]
    fn a_partial_date_compares_from_the_start_of_its_period() {
        assert_eq!(
            sort_date(Some(1947), None, None),
            NaiveDate::from_ymd_opt(1947, 1, 1)
        );
        assert_eq!(
            sort_date(Some(1947), Some(2), None),
            NaiveDate::from_ymd_opt(1947, 2, 1)
        );
    }

    #[test]
    fn validate_year_required() {
        let mut p = DateParts {
            month: Some(2),
            ..Default::default()
        };
        assert_eq!(p.validate(), Some("date.error.year_required"));
        p.year = Some(1947);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn validate_day_requires_month() {
        let mut p = DateParts {
            year: Some(1947),
            day: Some(23),
            ..Default::default()
        };
        assert_eq!(p.validate(), Some("date.error.day_requires_month"));
        p.month = Some(2);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn validate_second_date_year_required() {
        let mut p = DateParts {
            qualifier: DateQualifier::Between,
            year: Some(1940),
            ..Default::default()
        };
        assert_eq!(p.validate(), Some("date.error.year_required"));
        p.year2 = Some(1950);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn literal_localized() {
        let p = DateParts {
            year: Some(1947),
            month: Some(2),
            day: Some(23),
            ..Default::default()
        };
        assert_eq!(p.literal(&en()), "23 Feb 1947");
        assert_eq!(p.literal(&fr()), "23 févr. 1947");
    }

    #[test]
    fn literal_between_includes_second_date() {
        let p = DateParts {
            qualifier: DateQualifier::Between,
            year: Some(1940),
            year2: Some(1950),
            month2: Some(6),
            ..Default::default()
        };
        assert_eq!(p.literal(&en()), "between 1940 and Jun 1950");
        assert_eq!(p.literal(&fr()), "entre 1940 et juin 1950");
    }

    #[test]
    fn literal_carries_the_qualifier() {
        let p = DateParts {
            qualifier: DateQualifier::Before,
            year: Some(1900),
            ..Default::default()
        };
        assert_eq!(p.literal(&en()), "before 1900");
        assert_eq!(p.literal(&fr()), "avant 1900");
    }

    // ── FromAge ──────────────────────────────────────────────────────
    //
    // The mode saves nothing of its own: what lands in the DB is the `About`
    // year the age implies.

    fn aged(age: Option<u32>, in_year: Option<i32>) -> DateParts {
        DateParts {
            qualifier: DateQualifier::FromAge,
            age,
            age_ref_year: in_year,
            ..Default::default()
        }
    }

    #[test]
    fn from_age_resolves_to_an_about_year() {
        let p = aged(Some(14), Some(2026));
        assert_eq!(p.stored_qualifier(), DateQualifier::About);
        assert_eq!(p.date_value().as_deref(), Some("2012"));
        assert_eq!(p.date_value2(), None);
        assert_eq!(p.literal(&fr()), "vers 2012");
        assert_eq!(p.literal(&en()), "about 2012");
    }

    #[test]
    fn an_untouched_from_age_is_no_date_at_all() {
        let p = aged(None, None);
        assert!(p.is_empty());
        assert_eq!(p.validate(), None);
        assert_eq!(p.date_value(), None);
        // Not `About` — an empty date must not claim a precision.
        assert_eq!(p.stored_qualifier(), DateQualifier::Exact);
        assert_eq!(p.literal(&en()), "");
    }

    #[test]
    fn half_a_from_age_pair_is_rejected() {
        assert_eq!(
            aged(Some(14), None).validate(),
            Some("date.error.age_year_required")
        );
        assert_eq!(
            aged(None, Some(2026)).validate(),
            Some("date.error.age_required")
        );
        assert_eq!(aged(Some(14), Some(2026)).validate(), None);
    }

    /// Rows written before `FromAge` stopped being persisted must still open
    /// in the editor as the date they always meant.
    #[test]
    fn a_stored_from_age_row_reads_back_as_about() {
        let p = DateParts::from_fields(
            Calendar::Gregorian,
            DateQualifier::FromAge,
            Some("2012"),
            None,
        );
        assert_eq!(p.qualifier, DateQualifier::About);
        assert_eq!(p.year, Some(2012));
    }

    // ── format_date ──────────────────────────────────────────────────

    /// `format_date` for the calendar almost every date uses.
    fn fmt(i18n: &I18n, q: DateQualifier, v: Option<&str>, v2: Option<&str>) -> String {
        format_date(i18n, Calendar::Gregorian, q, v, v2)
    }

    #[test]
    fn format_date_matches_what_the_editor_previewed() {
        assert_eq!(
            fmt(&fr(), DateQualifier::About, Some("2012"), None),
            "vers 2012"
        );
        assert_eq!(
            fmt(&fr(), DateQualifier::Or, Some("1800"), Some("1810")),
            "1800 ou 1810"
        );
        assert_eq!(
            fmt(&en(), DateQualifier::Exact, Some("23 FEB 1947"), None),
            "23 Feb 1947"
        );
    }

    #[test]
    fn format_date_keeps_an_unparseable_value() {
        assert_eq!(
            fmt(
                &en(),
                DateQualifier::Exact,
                Some("sometime in the war"),
                None
            ),
            "sometime in the war"
        );
    }

    #[test]
    fn format_date_of_nothing_is_empty() {
        assert_eq!(fmt(&en(), DateQualifier::About, None, None), "");
        assert_eq!(fmt(&en(), DateQualifier::About, Some(""), None), "");
    }

    // ── Calendars ────────────────────────────────────────────────────
    //
    // A date must be written with the month names of the calendar it was
    // recorded in. Writing `2 FEB 14` under a Republican escape produces a
    // GEDCOM line no reader can take back.

    fn republican(y: i32, m: u8, d: u8) -> DateParts {
        DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(y),
            month: Some(m),
            day: Some(d),
            ..Default::default()
        }
    }

    #[test]
    fn a_republican_date_is_written_with_republican_months() {
        let p = republican(14, 2, 2);
        assert_eq!(p.date_value().as_deref(), Some("2 BRUM 14"));
        assert_eq!(p.literal(&fr()), "2 brumaire 14");
    }

    /// Picking another calendar says which one the date was recorded in, so
    /// the date has to be renumbered into it — the day does not move.
    #[test]
    fn changing_calendar_renumbers_the_date() {
        let gregorian = DateParts {
            year: Some(1796),
            month: Some(3),
            day: Some(11),
            ..Default::default()
        };
        let p = gregorian.in_calendar(Calendar::FrenchRepublican);
        assert_eq!((p.year, p.month, p.day), (Some(4), Some(6), Some(21)));
        assert_eq!(p.date_value().as_deref(), Some("21 VENT 4"));
        assert_eq!(p.literal(&fr()), "21 ventôse 4");
        // And back again, unchanged.
        let back = p.in_calendar(Calendar::Gregorian);
        assert_eq!(
            (back.year, back.month, back.day),
            (Some(1796), Some(3), Some(11))
        );
    }

    #[test]
    fn changing_calendar_moves_both_ends_of_a_range() {
        let p = DateParts {
            qualifier: DateQualifier::Between,
            year: Some(1799),
            month: Some(11),
            day: Some(9),
            year2: Some(1805),
            month2: Some(10),
            day2: Some(24),
            ..Default::default()
        }
        .in_calendar(Calendar::FrenchRepublican);
        assert_eq!((p.year, p.month, p.day), (Some(8), Some(2), Some(18)));
        assert_eq!((p.year2, p.month2, p.day2), (Some(14), Some(2), Some(2)));
    }

    /// A date the target calendar has no way to express is left exactly as
    /// typed rather than replaced by an invention.
    #[test]
    fn a_date_the_new_calendar_cannot_express_is_left_alone() {
        let p = DateParts {
            year: Some(1750),
            month: Some(3),
            day: Some(11),
            ..Default::default()
        }
        .in_calendar(Calendar::FrenchRepublican);
        assert_eq!((p.year, p.month, p.day), (Some(1750), Some(3), Some(11)));
        assert_eq!(p.calendar, Calendar::FrenchRepublican);
    }

    /// A year on its own is a period, not a day: it converts to the year it
    /// overlaps rather than the one its first day grazes.
    #[test]
    fn a_year_alone_keeps_its_precision_across_calendars() {
        let p = DateParts {
            year: Some(1796),
            ..Default::default()
        }
        .in_calendar(Calendar::FrenchRepublican);
        assert_eq!((p.year, p.month, p.day), (Some(4), None, None));
    }

    /// The Republican year closes with five (six in a sextile year) jours
    /// complémentaires, which the thirteenth month slot holds.
    #[test]
    fn the_complementary_days_are_a_month_of_their_own() {
        let p = DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(4),
            month: Some(13),
            day: Some(5),
            ..Default::default()
        };
        assert_eq!(p.validate(), None);
        assert_eq!(p.date_value().as_deref(), Some("5 COMP 4"));
        // An IV was not sextile: it had no sixth complementary day.
        let sixth = DateParts { day: Some(6), ..p };
        assert_eq!(sixth.validate(), Some("date.error.invalid_date"));
    }

    /// Adar II only comes round in a Hebrew leap year, so it is neither
    /// offered nor accepted in a common one — but a month already entered is
    /// still listed, so the control never shows blank over a date that has one.
    #[test]
    fn adar_ii_belongs_to_a_leap_year_alone() {
        assert!(month_options(Calendar::Hebrew, Some(5784), None).contains(&7));
        assert!(!month_options(Calendar::Hebrew, Some(5783), None).contains(&7));
        assert!(month_options(Calendar::Hebrew, Some(5783), Some(7)).contains(&7));
        assert!(month_options(Calendar::Hebrew, None, None).contains(&7));

        let p = DateParts {
            calendar: Calendar::Hebrew,
            year: Some(5783),
            month: Some(7),
            ..Default::default()
        };
        assert_eq!(p.validate(), Some("date.error.month_out_of_range"));
        assert_eq!(
            DateParts {
                year: Some(5784),
                ..p
            }
            .validate(),
            None
        );
    }

    /// Each calendar counts the same span of history its own way, so the
    /// plausible window has to be counted its way too.
    #[test]
    fn a_year_is_judged_by_its_own_calendars_era() {
        let hebrew = DateParts {
            calendar: Calendar::Hebrew,
            year: Some(5784),
            ..Default::default()
        };
        assert_eq!(hebrew.validate(), None, "5784 is 2023, not a typo");
        // The same number under an era that has not reached it.
        assert_eq!(
            DateParts {
                calendar: Calendar::Gregorian,
                ..hebrew
            }
            .validate(),
            Some("date.error.year_out_of_range")
        );
        assert_eq!(
            DateParts {
                calendar: Calendar::FrenchRepublican,
                year: Some(14),
                ..Default::default()
            }
            .validate(),
            None
        );
    }

    /// The month is optional, exactly as it is in the numeric field: a record
    /// that gives only « an VI » must stay enterable.
    #[test]
    fn a_named_calendar_still_takes_a_year_on_its_own() {
        let p = DateParts {
            calendar: Calendar::FrenchRepublican,
            year: Some(6),
            ..Default::default()
        };
        assert_eq!(p.validate(), None);
        assert_eq!(p.date_value().as_deref(), Some("6"));
    }

    #[test]
    fn a_republican_date_reads_back_into_the_same_fields() {
        let p = DateParts::from_fields(
            Calendar::FrenchRepublican,
            DateQualifier::Exact,
            Some("2 BRUM 14"),
            None,
        );
        assert_eq!((p.year, p.month, p.day), (Some(14), Some(2), Some(2)));
    }

    #[test]
    fn a_hebrew_date_uses_its_own_months() {
        let p = DateParts {
            calendar: Calendar::Hebrew,
            year: Some(5784),
            month: Some(1),
            day: Some(15),
            ..Default::default()
        };
        assert_eq!(p.date_value().as_deref(), Some("15 TSH 5784"));
        assert_eq!(p.literal(&en()), "15 Tishrei 5784");
    }

    /// Both calendars carry a thirteenth month, which the Gregorian rules
    /// would reject outright.
    #[test]
    fn a_thirteenth_month_is_allowed_where_one_exists() {
        assert_eq!(republican(14, 13, 3).validate(), None);
        assert_eq!(
            DateParts {
                calendar: Calendar::Gregorian,
                year: Some(1947),
                month: Some(13),
                ..Default::default()
            }
            .validate(),
            Some("date.error.month_out_of_range")
        );
    }

    // ── BCE ──────────────────────────────────────────────────────────

    #[test]
    fn a_bce_year_is_stored_the_gedcom_way() {
        let p = DateParts {
            year: Some(-3000),
            ..Default::default()
        };
        assert_eq!(p.date_value().as_deref(), Some("3000 BCE"));
        assert_eq!(p.literal(&en()), "3000 BCE");
        assert_eq!(p.literal(&fr()), "3000 av. J.-C.");
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn every_spelling_of_the_era_marker_reads_back_negative() {
        for raw in ["44 BCE", "44 BC", "44 B.C.", "44 b.c.e.", "-44"] {
            assert_eq!(greg(raw).0, Some(-44), "failed for {raw}");
        }
    }

    #[test]
    fn a_bce_date_survives_a_round_trip() {
        let p = DateParts {
            year: Some(-44),
            month: Some(3),
            day: Some(15),
            ..Default::default()
        };
        let stored = p.date_value().unwrap();
        assert_eq!(stored, "15 MAR 44 BCE");
        assert_eq!(greg(&stored), (Some(-44), Some(3), Some(15)));
    }

    // ── Validation ───────────────────────────────────────────────────

    #[test]
    fn a_day_that_never_existed_is_rejected() {
        let feb30 = DateParts {
            year: Some(1947),
            month: Some(2),
            day: Some(30),
            ..Default::default()
        };
        assert_eq!(feb30.validate(), Some("date.error.invalid_date"));
    }

    /// 1900 was a leap year in the Julian calendar but not the Gregorian one,
    /// so the same triplet has to be judged by the calendar it was written in.
    #[test]
    fn the_leap_rule_follows_the_calendar() {
        let feb29_1900 = |calendar| DateParts {
            calendar,
            year: Some(1900),
            month: Some(2),
            day: Some(29),
            ..Default::default()
        };
        assert_eq!(
            feb29_1900(Calendar::Gregorian).validate(),
            Some("date.error.invalid_date")
        );
        assert_eq!(feb29_1900(Calendar::Julian).validate(), None);
    }

    #[test]
    fn an_implausible_year_is_rejected_but_antiquity_is_not() {
        let year = |y| DateParts {
            year: Some(y),
            ..Default::default()
        };
        // The pharaohs are in range; a slipped keystroke and year zero are not.
        assert_eq!(year(-3100).validate(), None);
        assert_eq!(year(19477).validate(), Some("date.error.year_out_of_range"));
        assert_eq!(year(0).validate(), Some("date.error.year_out_of_range"));
    }

    #[test]
    fn a_backwards_range_is_rejected() {
        let mut p = DateParts {
            qualifier: DateQualifier::Between,
            year: Some(1810),
            year2: Some(1800),
            ..Default::default()
        };
        assert_eq!(p.validate(), Some("date.error.range_out_of_order"));
        // `Or` offers two candidates, so their order carries no claim.
        p.qualifier = DateQualifier::Or;
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn an_implausible_age_is_rejected() {
        assert_eq!(
            aged(Some(400), Some(2026)).validate(),
            Some("date.error.age_out_of_range")
        );
        assert_eq!(aged(Some(103), Some(2026)).validate(), None);
    }

    #[test]
    fn a_number_field_keeps_only_its_digits() {
        assert_eq!(parse_u8("2a"), Some(2));
        assert_eq!(parse_u8("abc"), None);
        assert_eq!(parse_year("1947x"), Some(1947));
        assert_eq!(parse_year("-3000"), Some(-3000));
        // A minus anywhere but the front is not a sign.
        assert_eq!(parse_year("30-00"), Some(3000));
    }

    /// A second value is dead weight unless the qualifier asks for one.
    #[test]
    fn format_date_ignores_a_stray_second_value() {
        assert_eq!(
            fmt(&en(), DateQualifier::About, Some("1800"), Some("1810")),
            "about 1800"
        );
    }
}
