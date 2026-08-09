//! Reusable date input widget.
//!
//! Every date in the app is edited through the same control: a calendar
//! selector (Gregorian, Julian, …), a precision qualifier (Exact, About, …)
//! and three numeric fields (day / month / year). A localized literal preview
//! is shown under the fields (e.g. « 23 févr. 1947 »). Qualifiers `Or` and
//! `Between` expose a second set of date fields.

use chrono::NaiveDate;
use dioxus::prelude::*;
use oxidgene_core::enums::{Calendar, DateQualifier};
use std::str::FromStr;

use crate::i18n::I18n;

/// GEDCOM three-letter month abbreviations (canonical storage form).
const GEDCOM_MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

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
        let (year, month, day) = date_value.map(parse_components).unwrap_or_default();
        let (year2, month2, day2) = date_value2.map(parse_components).unwrap_or_default();
        Self {
            calendar,
            qualifier,
            year,
            month,
            day,
            year2,
            month2,
            day2,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.year.is_none() && self.month.is_none() && self.day.is_none()
    }

    pub fn needs_second_date(&self) -> bool {
        self.qualifier.needs_second_date()
    }

    /// Returns an i18n error key when the components are inconsistent.
    ///
    /// Rules: the year is mandatory as soon as a date is entered, the month is
    /// optional, and a day requires a month.
    pub fn validate(&self) -> Option<&'static str> {
        if self.year.is_none() && (self.month.is_some() || self.day.is_some()) {
            return Some("date.error.year_required");
        }
        if self.day.is_some() && self.month.is_none() {
            return Some("date.error.day_requires_month");
        }
        if self.needs_second_date() {
            if self.year2.is_none() {
                return Some("date.error.year_required");
            }
            if self.day2.is_some() && self.month2.is_none() {
                return Some("date.error.day_requires_month");
            }
        }
        None
    }

    /// Canonical `date_value` (GEDCOM-style: `23 FEB 1947` / `FEB 1947` / `1947`).
    pub fn date_value(&self) -> Option<String> {
        format_value(self.year, self.month, self.day)
    }

    /// Canonical `date_value2` for the `Or` / `Between` second date.
    pub fn date_value2(&self) -> Option<String> {
        if self.needs_second_date() {
            format_value(self.year2, self.month2, self.day2)
        } else {
            None
        }
    }

    /// Normalized sortable date (missing month/day default to 1).
    pub fn date_sort(&self) -> Option<NaiveDate> {
        sort_date(self.year, self.month, self.day)
    }

    /// Localized human-readable preview (e.g. « 23 févr. 1947 »), including the
    /// second date for `Or` / `Between` qualifiers.
    pub fn literal(&self, i18n: &I18n) -> String {
        let first = literal_components(i18n, self.year, self.month, self.day);
        if !self.needs_second_date() {
            return first;
        }
        let second = literal_components(i18n, self.year2, self.month2, self.day2);
        let sep = if self.qualifier == DateQualifier::Or {
            i18n.t("person_form.date2_label_or")
        } else {
            i18n.t("person_form.date2_label_between")
        };
        match (first.is_empty(), second.is_empty()) {
            (true, true) => String::new(),
            (false, true) => first,
            (true, false) => second,
            (false, false) => format!("{first} {sep} {second}"),
        }
    }
}

/// Parse a free-text date into `(year, month, day)`.
///
/// Accepts ISO (`1947-02-23`, `1947-02`, `1947`), a bare year, and GEDCOM-style
/// phrases (`23 FEB 1947`, `FEB 1947`).
fn parse_components(s: &str) -> (Option<i32>, Option<u8>, Option<u8>) {
    let s = s.trim();
    if s.is_empty() {
        return (None, None, None);
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
    // Token mode: « 23 FEB 1947 ».
    let mut year = None;
    let mut month = None;
    let mut day = None;
    for tok in s.split_whitespace() {
        let up = tok.to_ascii_uppercase();
        if let Some(m) = GEDCOM_MONTHS.iter().position(|&g| g == up) {
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

/// Canonical storage string for a component triplet (year mandatory).
fn format_value(year: Option<i32>, month: Option<u8>, day: Option<u8>) -> Option<String> {
    let y = year?;
    let month = month.filter(|m| (1..=12).contains(m));
    let day = day.filter(|d| (1..=31).contains(d));
    Some(match (day, month) {
        (Some(d), Some(m)) => format!("{d} {} {y}", GEDCOM_MONTHS[(m - 1) as usize]),
        (None, Some(m)) => format!("{} {y}", GEDCOM_MONTHS[(m - 1) as usize]),
        _ => y.to_string(),
    })
}

fn sort_date(year: Option<i32>, month: Option<u8>, day: Option<u8>) -> Option<NaiveDate> {
    let y = year?;
    let m = month.filter(|m| (1..=12).contains(m)).unwrap_or(1) as u32;
    let d = day.filter(|d| (1..=31).contains(d)).unwrap_or(1) as u32;
    NaiveDate::from_ymd_opt(y, m, d)
}

/// Localized literal for a component triplet (empty when no year).
fn literal_components(
    i18n: &I18n,
    year: Option<i32>,
    month: Option<u8>,
    day: Option<u8>,
) -> String {
    let Some(y) = year else { return String::new() };
    let month = month.filter(|m| (1..=12).contains(m));
    let day = day.filter(|d| (1..=31).contains(d));
    match (day, month) {
        (Some(d), Some(m)) => {
            let key = format!("date.month.{m}");
            format!("{d} {} {y}", i18n.t(&key))
        }
        (None, Some(m)) => {
            let key = format!("date.month.{m}");
            format!("{} {y}", i18n.t(&key))
        }
        _ => y.to_string(),
    }
}

/// PascalCase value used by the qualifier `<select>` options.
pub fn qualifier_value(q: DateQualifier) -> &'static str {
    match q {
        DateQualifier::Exact => "Exact",
        DateQualifier::About => "About",
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

pub fn qualifier_options(i18n: &I18n) -> Element {
    let keys = [
        ("Exact", "exact"),
        ("About", "about"),
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
                {i18n.t_args(&format!("date_qualifier.{key}"), &[])}
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
                {i18n.t_args(&format!("calendar.{key}"), &[])}
            }
        }
    }
}

fn opt_num<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

fn parse_i32(s: &str) -> Option<i32> {
    let t = s.trim();
    if t.is_empty() { None } else { t.parse().ok() }
}

fn parse_u8(s: &str) -> Option<u8> {
    let t = s.trim();
    if t.is_empty() { None } else { t.parse().ok() }
}

/// The day / month / year triplet (or its `Or` / `Between` counterpart).
fn part_inputs(mut parts: Signal<DateParts>, second: bool, i18n: I18n) -> Element {
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
            oninput: move |e| {
                let mut np = parts();
                let v = parse_u8(&e.value()).filter(|d| (1..=31).contains(d));
                if second { np.day2 = v; } else { np.day = v; }
                parts.set(np);
            },
        }
        input {
            class: "pf-date-part pf-date-mm",
            r#type: "text",
            inputmode: "numeric",
            maxlength: 2,
            placeholder: "{i18n.t(\"date.ph_month\")}",
            value: opt_num(m),
            oninput: move |e| {
                let mut np = parts();
                let v = parse_u8(&e.value()).filter(|m| (1..=12).contains(m));
                if second { np.month2 = v; } else { np.month = v; }
                parts.set(np);
            },
        }
        input {
            class: "pf-date-part pf-date-yyyy",
            r#type: "text",
            inputmode: "numeric",
            maxlength: 4,
            placeholder: "{i18n.t(\"date.ph_year\")}",
            value: opt_num(y),
            oninput: move |e| {
                let mut np = parts();
                let v = parse_i32(&e.value());
                if second { np.year2 = v; } else { np.year = v; }
                parts.set(np);
            },
        }
    }
}

/// Date editor: calendar + qualifier + day/month/year (+ optional second date),
/// with a localized literal preview underneath.
#[component]
pub fn DateInput(parts: Signal<DateParts>, i18n: I18n) -> Element {
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
                        let mut np = parts();
                        np.calendar = Calendar::from_str(&e.value()).unwrap_or_default();
                        parts.set(np);
                    },
                    {calendar_options(&i18n)}
                }
                select {
                    class: "pf-date-qualifier-select",
                    value: qualifier_value(p.qualifier),
                    onchange: move |e| {
                        let mut np = parts();
                        np.qualifier = DateQualifier::from_str(&e.value()).unwrap_or_default();
                        parts.set(np);
                    },
                    {qualifier_options(&i18n)}
                }
                {part_inputs(parts, false, i18n)}
                if p.needs_second_date() {
                    span { class: "pf-date-separator", "{sep}" }
                    {part_inputs(parts, true, i18n)}
                }
            }
            if !literal.is_empty() {
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
        I18n(Language::English)
    }

    fn fr() -> I18n {
        I18n(Language::French)
    }

    #[test]
    fn parse_iso_full() {
        assert_eq!(
            parse_components("1947-02-23"),
            (Some(1947), Some(2), Some(23))
        );
    }

    #[test]
    fn parse_iso_year_month() {
        assert_eq!(parse_components("1947-02"), (Some(1947), Some(2), None));
    }

    #[test]
    fn parse_bare_year() {
        assert_eq!(parse_components("1947"), (Some(1947), None, None));
    }

    #[test]
    fn parse_gedcom_full() {
        assert_eq!(
            parse_components("23 FEB 1947"),
            (Some(1947), Some(2), Some(23))
        );
    }

    #[test]
    fn parse_gedcom_month_year() {
        assert_eq!(parse_components("FEB 1947"), (Some(1947), Some(2), None));
    }

    #[test]
    fn parse_empty() {
        assert_eq!(parse_components(""), (None, None, None));
        assert_eq!(parse_components("   "), (None, None, None));
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
        assert_eq!(p.date_sort(), NaiveDate::from_ymd_opt(1947, 2, 23));
    }

    #[test]
    fn date_value_partial() {
        let mut p = DateParts::default();
        p.year = Some(1947);
        assert_eq!(p.date_value().as_deref(), Some("1947"));
        p.month = Some(2);
        assert_eq!(p.date_value().as_deref(), Some("FEB 1947"));
    }

    #[test]
    fn date_sort_defaults_missing_components() {
        let mut p = DateParts::default();
        p.year = Some(1947);
        assert_eq!(p.date_sort(), NaiveDate::from_ymd_opt(1947, 1, 1));
        p.month = Some(2);
        assert_eq!(p.date_sort(), NaiveDate::from_ymd_opt(1947, 2, 1));
    }

    #[test]
    fn validate_year_required() {
        let mut p = DateParts::default();
        p.month = Some(2);
        assert_eq!(p.validate(), Some("date.error.year_required"));
        p.year = Some(1947);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn validate_day_requires_month() {
        let mut p = DateParts::default();
        p.year = Some(1947);
        p.day = Some(23);
        assert_eq!(p.validate(), Some("date.error.day_requires_month"));
        p.month = Some(2);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn validate_second_date_year_required() {
        let mut p = DateParts::default();
        p.qualifier = DateQualifier::Between;
        p.year = Some(1940);
        assert_eq!(p.validate(), Some("date.error.year_required"));
        p.year2 = Some(1950);
        assert_eq!(p.validate(), None);
    }

    #[test]
    fn literal_localized() {
        let mut p = DateParts::default();
        p.year = Some(1947);
        p.month = Some(2);
        p.day = Some(23);
        assert_eq!(p.literal(&en()), "23 Feb 1947");
        assert_eq!(p.literal(&fr()), "23 févr. 1947");
    }

    #[test]
    fn literal_between_includes_second_date() {
        let mut p = DateParts::default();
        p.qualifier = DateQualifier::Between;
        p.year = Some(1940);
        p.year2 = Some(1950);
        p.month2 = Some(6);
        assert_eq!(p.literal(&en()), "1940 and Jun 1950");
    }
}
