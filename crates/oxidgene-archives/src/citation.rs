//! Normalized archive citations read from a source title.
//!
//! A citation names its archive first, then the locality, the parish, the act
//! and its period, and may end with the cited views:
//!
//! ```text
//! <code> - <locality> - <parish> - <act> - <period> - <free…> - vue <n>[d|g]/<count>
//! ```
//!
//! such as `AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13`.
//! The code selects a catalogue entry, whose `citation` settings may adjust
//! the grammar ([`CitationGrammar`]); nothing here is specific to one archive
//! or one portal.

use std::fmt;

use oxidgene_core::calendar;
use oxidgene_core::enums::Calendar;
use serde::{Deserialize, Serialize};

/// What separates the fields of a normalized citation.
pub const SEPARATOR: &str = " - ";

/// The longest range of views one citation may span, such as `vue 5-6/13`.
const MAX_VIEW_RANGE: u16 = 10;

/// The last year of the French Republican calendar, an XIV (1805–1806).
const LAST_REPUBLICAN_YEAR: i32 = 14;

const ROMAN_NUMERALS: [&str; LAST_REPUBLICAN_YEAR as usize] = [
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII", "XIV",
];

/// One kind of act a register holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActKind {
    /// `N`, civil status.
    Birth,
    /// `B`, parish registers.
    Baptism,
    /// `M`, both.
    Marriage,
    /// `D`, civil status.
    Death,
    /// `S`, parish registers.
    Burial,
}

impl ActKind {
    const ALL: [Self; 5] = [
        Self::Birth,
        Self::Baptism,
        Self::Marriage,
        Self::Death,
        Self::Burial,
    ];

    /// The letter a normalized citation writes this act with.
    pub const fn letter(self) -> char {
        match self {
            Self::Birth => 'N',
            Self::Baptism => 'B',
            Self::Marriage => 'M',
            Self::Death => 'D',
            Self::Burial => 'S',
        }
    }

    pub fn from_letter(letter: char) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.letter() == letter)
    }
}

/// The act field of a citation, and an entry of a collection's `acts`.
///
/// Written as a code: one act letter (`N`), several for a register mixing
/// them (`BMS`, `NMD`), or a table code starting with `T` (`TB`, `TD`), kept
/// as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Act {
    /// The act kinds, in the order written, each once.
    Register(Vec<ActKind>),
    /// A table, by its code.
    Table(String),
}

impl Act {
    /// Reads an act code, or `None` for any other text.
    pub fn from_code(code: &str) -> Option<Self> {
        if let Some(table) = code.strip_prefix('T') {
            let valid = (1..=4).contains(&table.len())
                && table.bytes().all(|byte| byte.is_ascii_uppercase());
            return valid.then(|| Self::Table(code.to_owned()));
        }
        let mut kinds = Vec::new();
        for letter in code.chars() {
            let kind = ActKind::from_letter(letter)?;
            if kinds.contains(&kind) {
                return None;
            }
            kinds.push(kind);
        }
        (!kinds.is_empty()).then_some(Self::Register(kinds))
    }

    /// The act kinds of a register; none for a table.
    pub fn kinds(&self) -> &[ActKind] {
        match self {
            Self::Register(kinds) => kinds,
            Self::Table(_) => &[],
        }
    }
}

impl fmt::Display for Act {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(kinds) => kinds
                .iter()
                .try_for_each(|kind| write!(f, "{}", kind.letter())),
            Self::Table(code) => f.write_str(code),
        }
    }
}

impl TryFrom<String> for Act {
    type Error = String;

    fn try_from(code: String) -> Result<Self, Self::Error> {
        Self::from_code(&code).ok_or_else(|| format!("`{code}` is not an act code"))
    }
}

impl From<Act> for String {
    fn from(act: Act) -> Self {
        act.to_string()
    }
}

/// The half of a double page a citation points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// `d`, *droite*.
    Right,
    /// `g`, *gauche*.
    Left,
}

/// One cited image of a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CitedView {
    /// One-based view number, as cited.
    pub view: u16,
    pub side: Option<Side>,
}

/// A register's call number as cited, compared without spaces or case.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallNumber(String);

impl CallNumber {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `other` names the same register: `3E73/14` matches
    /// `3 E 73 / 14`.
    pub fn matches(&self, other: &str) -> bool {
        Self::folded(&self.0).eq(Self::folded(other))
    }

    fn folded(text: &str) -> impl Iterator<Item = char> + '_ {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_uppercase)
    }

    /// Whether a free field is shaped like a call number: letters and digits
    /// with a few separators, at least one digit and one capital, and no
    /// lowercase word (`acte 26`, `Registre 1877`), while `1 Mi 456` is one.
    fn is_shaped(field: &str) -> bool {
        let allowed = |c: char| c.is_ascii_alphanumeric() || " /._-".contains(c);
        let mut lowercase_run = 0;
        let mut longest_lowercase_run = 0;
        for c in field.chars() {
            lowercase_run = if c.is_ascii_lowercase() {
                lowercase_run + 1
            } else {
                0
            };
            longest_lowercase_run = longest_lowercase_run.max(lowercase_run);
        }
        (1..=40).contains(&field.len())
            && field.chars().all(allowed)
            && field.chars().any(|c| c.is_ascii_digit())
            && field.chars().any(|c| c.is_ascii_uppercase())
            && !field.starts_with(|c: char| c.is_ascii_lowercase())
            && longest_lowercase_run < 3
    }
}

/// Per-archive adjustments of the grammar: the catalogue entry's `citation`
/// object. A field left out keeps its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CitationGrammar {
    /// Parish values that mean no parish. Default `["(aucun)"]`.
    pub no_parish: Vec<String>,
    /// Words introducing the cited views, compared without case. Default
    /// `["vue"]`.
    pub view_words: Vec<String>,
}

impl Default for CitationGrammar {
    fn default() -> Self {
        Self {
            no_parish: vec!["(aucun)".to_owned()],
            view_words: vec!["vue".to_owned()],
        }
    }
}

impl CitationGrammar {
    /// Rejects settings that would make every citation unreadable.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let blank = |values: &[String]| values.iter().any(|value| value.trim().is_empty());
        if blank(&self.no_parish) || self.view_words.is_empty() || blank(&self.view_words) {
            return Err("citation overrides need non-blank values and a view word".to_owned());
        }
        if self.view_words.iter().any(|word| word.contains(' ')) {
            return Err("a view word is a single word".to_owned());
        }
        Ok(())
    }

    fn is_no_parish(&self, field: &str) -> bool {
        self.no_parish.iter().any(|value| value == field)
    }

    /// The views part of a field starting with a view word, such as `5d/13`
    /// in `vue 5d/13`.
    fn view_spec<'a>(&self, field: &'a str) -> Option<&'a str> {
        let (word, spec) = field.split_once(' ')?;
        let word = word.to_lowercase();
        self.view_words
            .iter()
            .any(|candidate| candidate.to_lowercase() == word)
            .then(|| spec.trim())
    }
}

/// What a normalized citation identifies. Every part but the code, the
/// locality and the act is optional.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CitationParts {
    /// The archive code the title starts with, such as `AD44`.
    pub code: String,
    /// The locality, which may itself contain ` - `.
    pub locality: String,
    pub parish: Option<String>,
    pub act: Act,
    /// The first year of the period, Gregorian.
    pub year: Option<u16>,
    /// The period as written: `1877`, `1702-1703`, `an XII`.
    pub period: Option<String>,
    pub call_number: Option<CallNumber>,
    /// The cited views in order; empty when the title cites none, or cites
    /// one beyond its own view count.
    pub views: Vec<CitedView>,
    /// The register's image count, as cited.
    pub view_count: Option<u16>,
}

impl CitationParts {
    /// Reads a normalized source title, or `None` for any other title.
    pub fn parse(title: &str, grammar: &CitationGrammar) -> Option<Self> {
        let fields: Vec<&str> = title.split(SEPARATOR).map(str::trim).collect();
        let code = code_of(title)?;
        let act_at = find_act(&fields)?;
        let locality = fields[1..act_at - 1].join(SEPARATOR);
        if locality.is_empty() || grammar.is_no_parish(&locality) {
            return None;
        }
        let parish = Some(fields[act_at - 1])
            .filter(|parish| !parish.is_empty() && !grammar.is_no_parish(parish))
            .map(str::to_owned);
        let act = Act::from_code(fields[act_at])?;

        let mut rest = &fields[act_at + 1..];
        let mut period = None;
        let mut year = None;
        if let Some((field, first_year)) = rest
            .first()
            .and_then(|field| Some((*field, period_start(field)?)))
        {
            period = Some(field.to_owned());
            year = Some(first_year);
            rest = &rest[1..];
        }

        let mut views = Vec::new();
        let mut view_count = None;
        if let Some((free, spec)) = rest
            .split_last()
            .and_then(|(last, free)| Some((free, grammar.view_spec(last)?)))
        {
            rest = free;
            (views, view_count) = parse_views(spec).unwrap_or_default();
        }

        let call_number = rest
            .iter()
            .find(|field| CallNumber::is_shaped(field))
            .map(|field| CallNumber::new(*field));

        Some(Self {
            code: code.to_owned(),
            locality,
            parish,
            act,
            year,
            period,
            call_number,
            views,
            view_count,
        })
    }
}

/// The archive code a title starts with, when it is shaped like one:
/// capitals and digits, such as `AD44`.
pub fn code_of(title: &str) -> Option<&str> {
    let code = title.split(SEPARATOR).next()?.trim();
    let valid = (1..=12).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
    valid.then_some(code)
}

/// The index of the act field. The locality and the parish come first, so it
/// is at least the fourth field; an act code followed by a period wins over
/// an earlier one that is not.
fn find_act(fields: &[&str]) -> Option<usize> {
    let mut first = None;
    for (at, field) in fields.iter().enumerate().skip(3) {
        if Act::from_code(field).is_none() {
            continue;
        }
        if fields
            .get(at + 1)
            .is_some_and(|next| period_start(next).is_some())
        {
            return Some(at);
        }
        first.get_or_insert(at);
    }
    first
}

/// The Gregorian first year of a period field: `1877`, `1702-1703`, `an XII`,
/// `an XI-an XII`, `an XI-XII`.
fn period_start(field: &str) -> Option<u16> {
    let (first, last) = match field.split_once('-') {
        Some((first, last)) => (first.trim(), Some(last.trim())),
        None => (field, None),
    };
    let start = year_of(first)?;
    if let Some(last) = last {
        let end = year_of(last).or_else(|| republican_start(republican_numeral(last)?))?;
        if end < start {
            return None;
        }
    }
    Some(start)
}

/// A Gregorian year of four digits, or a Republican year `an <n>`.
fn year_of(text: &str) -> Option<u16> {
    if text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        return text.parse().ok();
    }
    let numeral = text
        .get(..3)
        .filter(|prefix| prefix.eq_ignore_ascii_case("an "))
        .and_then(|_| text.get(3..))?;
    republican_start(republican_numeral(numeral.trim())?)
}

/// A Republican year number, in Roman or Arabic numerals.
fn republican_numeral(text: &str) -> Option<i32> {
    let year = match text.parse::<i32>() {
        Ok(year) => year,
        Err(_) => {
            let upper = text.to_ascii_uppercase();
            let index = ROMAN_NUMERALS.iter().position(|roman| *roman == upper)?;
            i32::try_from(index).ok()? + 1
        }
    };
    (1..=LAST_REPUBLICAN_YEAR).contains(&year).then_some(year)
}

/// The Gregorian year a Republican year began in, its 1 Vendémiaire.
fn republican_start(year: i32) -> Option<u16> {
    let new_year = calendar::to_jdn(Calendar::FrenchRepublican, year, 1, 1)?;
    let (gregorian, _, _) = calendar::from_jdn(Calendar::Gregorian, new_year)?;
    u16::try_from(gregorian).ok()
}

/// `5d/13`, `5/13`, `5d`, or a range `5d-6g/13`. `None` when malformed or
/// when a view exceeds the cited count.
fn parse_views(spec: &str) -> Option<(Vec<CitedView>, Option<u16>)> {
    let (range, count) = match spec.split_once('/') {
        Some((range, count)) => (range, Some(count.trim().parse::<u16>().ok()?)),
        None => (spec, None),
    };
    let views = match range.split_once('-') {
        Some((first, last)) => view_range(view_token(first)?, view_token(last)?)?,
        None => vec![view_token(range)?],
    };
    if count.is_some_and(|count| views.iter().any(|view| view.view > count)) {
        return None;
    }
    Some((views, count))
}

/// `5`, `5d` or `5g`.
fn view_token(token: &str) -> Option<CitedView> {
    let token = token.trim();
    let (number, side) = match token.strip_suffix('d') {
        Some(number) => (number, Some(Side::Right)),
        None => match token.strip_suffix('g') {
            Some(number) => (number, Some(Side::Left)),
            None => (token, None),
        },
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let view = number.parse::<u16>().ok().filter(|view| *view > 0)?;
    Some(CitedView { view, side })
}

/// Every view from `first` to `last`; the sides cited apply to the ends.
fn view_range(first: CitedView, last: CitedView) -> Option<Vec<CitedView>> {
    if last.view < first.view || last.view - first.view >= MAX_VIEW_RANGE {
        return None;
    }
    if first.view == last.view {
        let side = (first.side == last.side).then_some(first.side).flatten();
        return Some(vec![CitedView {
            view: first.view,
            side,
        }]);
    }
    Some(
        (first.view..=last.view)
            .map(|view| CitedView {
                view,
                side: match view {
                    _ if view == first.view => first.side,
                    _ if view == last.view => last.side,
                    _ => None,
                },
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(title: &str) -> Option<CitationParts> {
        CitationParts::parse(title, &CitationGrammar::default())
    }

    fn view(view: u16, side: Option<Side>) -> CitedView {
        CitedView { view, side }
    }

    #[test]
    fn reads_a_birth_with_a_right_hand_view() {
        let citation =
            parse("AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - vue 5d/13")
                .expect("a normalized citation");

        assert_eq!(citation.code, "AD44");
        assert_eq!(citation.locality, "Exampleville");
        assert_eq!(citation.parish, None);
        assert_eq!(citation.act, Act::Register(vec![ActKind::Birth]));
        assert_eq!(citation.year, Some(1877));
        assert_eq!(citation.period.as_deref(), Some("1877"));
        assert_eq!(citation.call_number, Some(CallNumber::new("3E1/2")));
        assert_eq!(citation.views, [view(5, Some(Side::Right))]);
        assert_eq!(citation.view_count, Some(13));
    }

    #[test]
    fn keeps_a_locality_that_contains_the_separator_and_reads_the_parish() {
        let citation =
            parse("AD44 - Example - Part - Saint-Example - B - 1791 - 3E1/2 - acte 4 - vue 3g/12")
                .expect("a normalized citation");

        assert_eq!(citation.locality, "Example - Part");
        assert_eq!(citation.parish.as_deref(), Some("Saint-Example"));
        assert_eq!(citation.act, Act::Register(vec![ActKind::Baptism]));
        assert_eq!(citation.views, [view(3, Some(Side::Left))]);
    }

    #[test]
    fn reads_every_act_letter_their_combinations_and_tables() {
        for (code, act) in [
            ("N", Act::Register(vec![ActKind::Birth])),
            ("B", Act::Register(vec![ActKind::Baptism])),
            ("M", Act::Register(vec![ActKind::Marriage])),
            ("D", Act::Register(vec![ActKind::Death])),
            ("S", Act::Register(vec![ActKind::Burial])),
            (
                "BMS",
                Act::Register(vec![ActKind::Baptism, ActKind::Marriage, ActKind::Burial]),
            ),
            (
                "NMD",
                Act::Register(vec![ActKind::Birth, ActKind::Marriage, ActKind::Death]),
            ),
            ("TB", Act::Table("TB".to_owned())),
        ] {
            let citation = parse(&format!("AD85 - Exampleville - (aucun) - {code} - 1802"))
                .expect("a normalized citation");
            assert_eq!(citation.code, "AD85");
            assert_eq!(citation.act, act);
            assert_eq!(citation.act.to_string(), code);
            assert!(citation.views.is_empty());
        }
        for code in ["NN", "X", "T", "Tb", "TABCDE", ""] {
            assert_eq!(Act::from_code(code), None, "{code}");
        }
    }

    #[test]
    fn reads_the_first_year_of_a_period() {
        for (period, year) in [
            ("1877", 1877),
            ("1702-1703", 1702),
            ("an I", 1792),
            ("an XII", 1803),
            ("An xii", 1803),
            ("an 3", 1794),
            ("an XI-an XII", 1802),
            ("an XI-XII", 1802),
            ("an XIV", 1805),
            ("1792-an II", 1792),
        ] {
            let citation = parse(&format!("AD44 - Exampleville - (aucun) - D - {period}"))
                .expect("a normalized citation");
            assert_eq!(citation.year, Some(year), "{period}");
            assert_eq!(citation.period.as_deref(), Some(period));
        }
        for period in ["circa", "an XV", "an 0", "1703-1702", "187"] {
            let citation = parse(&format!("AD44 - Exampleville - (aucun) - D - {period}"))
                .expect("a citation without a period");
            assert_eq!(citation.year, None, "{period}");
            assert_eq!(citation.period, None, "{period}");
        }
    }

    #[test]
    fn finds_the_call_number_among_free_fields() {
        let citation = parse("AD44 - Exampleville - (aucun) - M - 1850 - acte 3 - 3 E 73 / 14")
            .expect("a normalized citation");
        let call_number = citation.call_number.expect("a call number");
        assert_eq!(call_number.as_str(), "3 E 73 / 14");
        assert!(call_number.matches("3E73/14"));
        assert!(call_number.matches("3e73/14"));
        assert!(!call_number.matches("3E73/15"));

        for (field, shaped) in [
            ("1 Mi 456", true),
            ("6NUM8/003/050", true),
            ("acte 26", false),
            ("Registre 1877", false),
            ("Exampleville", false),
        ] {
            assert_eq!(CallNumber::is_shaped(field), shaped, "{field}");
        }
        let without = parse("AD44 - Exampleville - (aucun) - M - 1850 - acte 3")
            .expect("a normalized citation");
        assert_eq!(without.call_number, None);
    }

    #[test]
    fn reads_a_range_of_views() {
        let citation = parse("AD44 - Exampleville - (aucun) - M - 1850 - acte 3 - vue 5d-6g/13")
            .expect("a normalized citation");
        assert_eq!(
            citation.views,
            [view(5, Some(Side::Right)), view(6, Some(Side::Left))]
        );
        assert_eq!(citation.view_count, Some(13));

        let citation = parse("AD44 - Exampleville - (aucun) - M - 1850 - vue 4-6/13")
            .expect("a normalized citation");
        assert_eq!(
            citation.views,
            [view(4, None), view(5, None), view(6, None)]
        );

        let citation = parse("AD44 - Exampleville - (aucun) - M - 1850 - vue 7g")
            .expect("a normalized citation");
        assert_eq!(citation.views, [view(7, Some(Side::Left))]);
        assert_eq!(citation.view_count, None);
    }

    #[test]
    fn drops_an_inconsistent_view_but_keeps_the_register() {
        for views in [
            "vue 14d/13",
            "vue 6-5/13",
            "vue 0/13",
            "vue 1-40/80",
            "vue x/13",
        ] {
            let citation = parse(&format!(
                "AD44 - Exampleville - (aucun) - N - 1877 - 3E1/2 - acte 26 - {views}"
            ))
            .expect("a normalized citation");
            assert!(citation.views.is_empty(), "{views}");
            assert_eq!(citation.view_count, None, "{views}");
            assert_eq!(citation.call_number, Some(CallNumber::new("3E1/2")));
        }
    }

    #[test]
    fn prefers_the_act_followed_by_a_period() {
        // An `M` inside the locality is not the act when a later act code is
        // followed by a year.
        let citation =
            parse("AD44 - Example - Part - M - (aucun) - B - 1791").expect("a normalized citation");
        assert_eq!(citation.locality, "Example - Part - M");
        assert_eq!(citation.act, Act::Register(vec![ActKind::Baptism]));
    }

    #[test]
    fn applies_the_archive_overrides() {
        let grammar = CitationGrammar {
            no_parish: vec!["-".to_owned(), "(none)".to_owned()],
            view_words: vec!["image".to_owned()],
        };
        let title = "AB12 - Exampleville - (none) - N - 1877 - image 5/13";
        let citation = CitationParts::parse(title, &grammar).expect("a normalized citation");
        assert_eq!(citation.parish, None);
        assert_eq!(citation.views, [view(5, None)]);

        // The default grammar reads neither.
        let citation = parse(title).expect("a normalized citation");
        assert_eq!(citation.parish.as_deref(), Some("(none)"));
        assert!(citation.views.is_empty());

        assert!(grammar.validate().is_ok());
        let blank = CitationGrammar {
            view_words: Vec::new(),
            ..CitationGrammar::default()
        };
        assert!(blank.validate().is_err());
    }

    #[test]
    fn the_grammar_overrides_deserialize_with_defaults() {
        let grammar: CitationGrammar =
            serde_json::from_str(r#"{"view_words": ["image"]}"#).unwrap();
        assert_eq!(grammar.no_parish, ["(aucun)"]);
        assert_eq!(grammar.view_words, ["image"]);
        assert!(serde_json::from_str::<CitationGrammar>(r#"{"other": 1}"#).is_err());
    }

    #[test]
    fn rejects_titles_that_are_not_normalized() {
        for title in [
            "Parish register of Exampleville",
            "ad44 - Exampleville - (aucun) - N - 1877",
            "AD44 - (aucun) - N - 1877",
            "AD44 - Exampleville - (aucun) - X - 1877",
            "AD44 - Exampleville - N - 1877",
            "AD44 - (aucun) - (aucun) - N - 1877",
        ] {
            assert_eq!(parse(title), None, "{title}");
        }
    }
}
