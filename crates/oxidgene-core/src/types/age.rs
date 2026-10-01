//! The age a record gives for a person at an event (GEDCOM `AGE`).
//!
//! A register states an age, not a birth date: "aged 34", "under one year",
//! "a child". [`AgeAtEvent`] keeps that statement as GEDCOM writes it — the
//! canonical text is stored (`34y`, `< 1y 6m`, `CHILD`) — and turns it into
//! the interval of days it allows, so a check can compare it with the age
//! computed from a birth date. The parser is lenient about spacing and case
//! (`1y6m`, `34 Y`, a bare `34` for years) and strict about meaning: anything
//! that is not an age is refused rather than guessed at.

use std::fmt;
use std::str::FromStr;

/// Days in a year, on average over the Gregorian cycle.
const DAYS_PER_YEAR: f64 = 365.2425;
/// Days in a month, on average.
const DAYS_PER_MONTH: f64 = DAYS_PER_YEAR / 12.0;

/// The largest number of years an age may state.
pub const MAX_YEARS: u16 = 999;
/// The largest count of months, weeks or days an age may state: what a
/// GEDCOM writer holds for them (a byte).
pub const MAX_UNIT: u16 = 255;

/// Whether the real age was the one stated, less, or more.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AgeModifier {
    #[default]
    Exact,
    /// `<`: younger than the stated age.
    LessThan,
    /// `>`: older than the stated age.
    GreaterThan,
}

/// An age at an event, as GEDCOM states one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeAtEvent {
    /// `CHILD`: under eight years.
    Child,
    /// `INFANT`: under one year.
    Infant,
    /// `STILLBORN`: died just before, at or near birth.
    Stillborn,
    /// A duration in years, months, weeks and days, at least one of them set.
    Duration {
        modifier: AgeModifier,
        years: Option<u16>,
        months: Option<u16>,
        weeks: Option<u16>,
        days: Option<u16>,
    },
}

/// Why a text is not an age.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgeParseError {
    /// Nothing was given.
    Empty,
    /// The text is not an age: an unknown word, unit or shape.
    Malformed,
    /// A count is larger than an age can be.
    OutOfRange,
}

impl fmt::Display for AgeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "an age cannot be empty",
            Self::Malformed => {
                "an age is a duration such as 34y, < 1y 6m or 3m 2w 5d, or CHILD, INFANT or STILLBORN"
            }
            Self::OutOfRange => "an age states at most 999 years and 255 months, weeks or days",
        })
    }
}

impl std::error::Error for AgeParseError {}

impl AgeAtEvent {
    /// The ages, in days, this statement allows: the first day and the last
    /// one, or no last day for an age stated as a minimum (`> 80y`).
    ///
    /// An exact age names a completed duration: `34y` is any day from the
    /// 34th birthday to the eve of the 35th, and `34y 6m` any day of that
    /// seventh month. Years and months are averages (365.2425 and a twelfth
    /// of it), so a bound can be a day off a calendar's; checks comparing
    /// ages allow far more than that.
    pub fn bounds(&self) -> (u32, Option<u32>) {
        match *self {
            Self::Child => (0, Some(days(8.0 * DAYS_PER_YEAR) - 1)),
            Self::Infant => (0, Some(days(DAYS_PER_YEAR) - 1)),
            Self::Stillborn => (0, Some(0)),
            Self::Duration {
                modifier,
                years,
                months,
                weeks,
                days: d,
            } => {
                let stated = days(
                    f64::from(years.unwrap_or(0)) * DAYS_PER_YEAR
                        + f64::from(months.unwrap_or(0)) * DAYS_PER_MONTH
                        + f64::from(weeks.unwrap_or(0)) * 7.0
                        + f64::from(d.unwrap_or(0)),
                );
                // The smallest unit stated sets the precision.
                let unit = if d.is_some() {
                    1.0
                } else if weeks.is_some() {
                    7.0
                } else if months.is_some() {
                    DAYS_PER_MONTH
                } else {
                    DAYS_PER_YEAR
                };
                let next = days(f64::from(stated) + unit);
                match modifier {
                    AgeModifier::Exact => (stated, Some(next.saturating_sub(1).max(stated))),
                    AgeModifier::LessThan => (0, Some(stated.saturating_sub(1))),
                    AgeModifier::GreaterThan => (stated, None),
                }
            }
        }
    }

    /// The whole years an exact or bounded duration states, for display:
    /// `Some(34)` for `34y` and `34y 6m`, `Some(0)` for `6m`.
    pub fn years(&self) -> Option<u16> {
        match *self {
            Self::Duration { years, .. } => Some(years.unwrap_or(0)),
            _ => None,
        }
    }
}

/// `value` days, rounded down.
fn days(value: f64) -> u32 {
    // Bounded by `MAX_YEARS` years plus `MAX_UNIT` months: far below
    // `u32::MAX`, and never negative.
    value.floor() as u32
}

impl fmt::Display for AgeAtEvent {
    /// The canonical GEDCOM form: `CHILD`, `34y`, `< 1y 6m`, `> 2w 3d`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Child => f.write_str("CHILD"),
            Self::Infant => f.write_str("INFANT"),
            Self::Stillborn => f.write_str("STILLBORN"),
            Self::Duration {
                modifier,
                years,
                months,
                weeks,
                days,
            } => {
                match modifier {
                    AgeModifier::Exact => {}
                    AgeModifier::LessThan => f.write_str("< ")?,
                    AgeModifier::GreaterThan => f.write_str("> ")?,
                }
                let parts = [(years, 'y'), (months, 'm'), (weeks, 'w'), (days, 'd')];
                let mut first = true;
                for (count, unit) in parts {
                    if let Some(count) = count {
                        if !first {
                            f.write_str(" ")?;
                        }
                        write!(f, "{count}{unit}")?;
                        first = false;
                    }
                }
                Ok(())
            }
        }
    }
}

impl FromStr for AgeAtEvent {
    type Err = AgeParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AgeParseError::Empty);
        }
        for (word, age) in [
            ("CHILD", Self::Child),
            ("INFANT", Self::Infant),
            ("STILLBORN", Self::Stillborn),
        ] {
            if text.eq_ignore_ascii_case(word) {
                return Ok(age);
            }
        }
        let (modifier, rest) = match text.as_bytes()[0] {
            b'<' => (AgeModifier::LessThan, &text[1..]),
            b'>' => (AgeModifier::GreaterThan, &text[1..]),
            _ => (AgeModifier::Exact, text),
        };
        let units = parse_units(rest.trim())?;
        Ok(Self::Duration {
            modifier,
            years: units[0],
            months: units[1],
            weeks: units[2],
            days: units[3],
        })
    }
}

/// The years, months, weeks and days of `text` (`34y 6m`, `1y6m`, a bare
/// `34` for years), each at most once and in that order.
fn parse_units(text: &str) -> Result<[Option<u16>; 4], AgeParseError> {
    let mut units: [Option<u16>; 4] = [None; 4];
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(AgeParseError::Malformed);
    }
    if compact.bytes().all(|b| b.is_ascii_digit()) {
        units[0] = Some(count(&compact, MAX_YEARS)?);
        return Ok(units);
    }
    let mut last = None;
    let mut digits = String::new();
    for c in compact.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let slot = match c.to_ascii_lowercase() {
            'y' => 0,
            'm' => 1,
            'w' => 2,
            'd' => 3,
            _ => return Err(AgeParseError::Malformed),
        };
        // Each unit once, largest first, and always after a number.
        if digits.is_empty() || last.is_some_and(|previous| slot <= previous) {
            return Err(AgeParseError::Malformed);
        }
        let max = if slot == 0 { MAX_YEARS } else { MAX_UNIT };
        units[slot] = Some(count(&digits, max)?);
        digits.clear();
        last = Some(slot);
    }
    if !digits.is_empty() {
        return Err(AgeParseError::Malformed);
    }
    Ok(units)
}

/// `digits` as a count no larger than `max`.
fn count(digits: &str, max: u16) -> Result<u16, AgeParseError> {
    match digits.parse::<u32>() {
        Ok(n) if n <= u32::from(max) => Ok(n as u16),
        _ => Err(AgeParseError::OutOfRange),
    }
}

/// `text` in the canonical form [`AgeAtEvent`] stores, or why it is not an
/// age. Blank text is no age at all (`Ok(None)`).
pub fn normalize(text: &str) -> Result<Option<String>, AgeParseError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    text.parse::<AgeAtEvent>().map(|age| Some(age.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn age(text: &str) -> AgeAtEvent {
        text.parse().unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    #[test]
    fn canonical_forms_round_trip() {
        for text in [
            "34y",
            "< 1y 6m",
            "> 80y",
            "3m 2w 5d",
            "CHILD",
            "INFANT",
            "STILLBORN",
            "0d",
        ] {
            assert_eq!(age(text).to_string(), text);
        }
    }

    #[test]
    fn lenient_spellings_normalize() {
        for (text, canonical) in [
            ("1y6m", "1y 6m"),
            (" 34 Y ", "34y"),
            ("34", "34y"),
            ("<1y", "< 1y"),
            (">  2W 3D", "> 2w 3d"),
            ("child", "CHILD"),
            ("Stillborn", "STILLBORN"),
        ] {
            assert_eq!(age(text).to_string(), canonical, "{text}");
        }
    }

    #[test]
    fn what_is_not_an_age_is_refused() {
        assert_eq!("".parse::<AgeAtEvent>(), Err(AgeParseError::Empty));
        assert_eq!("  ".parse::<AgeAtEvent>(), Err(AgeParseError::Empty));
        for text in [
            "majeur", "about 30", "30 ans", "y", "<", "6m 1y", "1y 1y", "1y6", "3x", "-3y",
        ] {
            assert_eq!(
                text.parse::<AgeAtEvent>(),
                Err(AgeParseError::Malformed),
                "{text}"
            );
        }
        assert_eq!(
            "1000y".parse::<AgeAtEvent>(),
            Err(AgeParseError::OutOfRange)
        );
        assert_eq!("256d".parse::<AgeAtEvent>(), Err(AgeParseError::OutOfRange));
        assert_eq!(
            "99999999999d".parse::<AgeAtEvent>(),
            Err(AgeParseError::OutOfRange)
        );
    }

    #[test]
    fn an_exact_age_spans_its_smallest_unit() {
        let (min, max) = age("34y").bounds();
        assert_eq!(min, 12418);
        assert_eq!(max, Some(12782));
        let (min, max) = age("34y 6m").bounds();
        assert!(min > 12418 && max.unwrap() < 12782);
        assert_eq!(age("5d").bounds(), (5, Some(5)));
        assert_eq!(age("2w").bounds(), (14, Some(20)));
    }

    #[test]
    fn modifiers_and_keywords_open_one_side() {
        assert_eq!(age("< 1y").bounds(), (0, Some(364)));
        assert_eq!(age("> 80y").bounds(), (29219, None));
        assert_eq!(age("INFANT").bounds(), (0, Some(364)));
        assert_eq!(age("CHILD").bounds(), (0, Some(2920)));
        assert_eq!(age("STILLBORN").bounds(), (0, Some(0)));
    }

    #[test]
    fn years_are_the_whole_years_stated() {
        assert_eq!(age("34y 6m").years(), Some(34));
        assert_eq!(age("6m").years(), Some(0));
        assert_eq!(age("CHILD").years(), None);
    }

    #[test]
    fn normalize_keeps_blank_as_none() {
        assert_eq!(normalize(" "), Ok(None));
        assert_eq!(normalize("1y6m"), Ok(Some("1y 6m".to_string())));
        assert!(normalize("majeur").is_err());
    }
}
