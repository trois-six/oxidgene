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
//! A series of records other than acts — a census, a military register, a
//! conscription list, succession tables — is named in words where the parish
//! and the act would stand, and its fields vary more:
//!
//! ```text
//! <code> - [<locality>] - [<period>] - <series in words> - [<period>] - <free…> - vue <n>[d|g]/<count>
//! ```
//!
//! such as `AD99 - Exampleville - Recensement - 1872 - 6 M 999 - vue 12g/40`.
//! The code selects a catalogue entry, whose `citation` settings may adjust
//! the grammar ([`CitationGrammar`]); nothing here is specific to one archive
//! or one portal.

use std::collections::BTreeMap;
use std::fmt;

use oxidgene_core::calendar;
use oxidgene_core::enums::{Calendar, DocumentCategory};
use oxidgene_core::search::fold_words;
use serde::{Deserialize, Serialize};

/// What separates the fields of a normalized citation.
pub const SEPARATOR: &str = " - ";

/// The longest range of views one citation may span, such as `vue 5-6/13`.
const MAX_VIEW_RANGE: u16 = 10;

/// The last year of the French Republican calendar, an XIV (1805–1806).
const LAST_REPUBLICAN_YEAR: i32 = 14;

/// The first year whose marriages the civil status records rather than the
/// parish registers.
const FIRST_CIVIL_STATUS_YEAR: u16 = 1793;

/// The words, folded, that introduce an act, matricule or entry number:
/// `acte 26`, `matricule 1268`, `n° 12`, `ordre 945`, `n° d'ordre 945`.
const NUMBER_WORDS: [&str; 9] = [
    "acte",
    "matricule",
    "n",
    "no",
    "numero",
    "ordre",
    "n d ordre",
    "no d ordre",
    "numero d ordre",
];

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
    /// `P`, publications of banns, filed with the marriages.
    Publication,
}

impl ActKind {
    const ALL: [Self; 6] = [
        Self::Birth,
        Self::Baptism,
        Self::Marriage,
        Self::Death,
        Self::Burial,
        Self::Publication,
    ];

    /// The letter a normalized citation writes this act with.
    pub const fn letter(self) -> char {
        match self {
            Self::Birth => 'N',
            Self::Baptism => 'B',
            Self::Marriage => 'M',
            Self::Death => 'D',
            Self::Burial => 'S',
            Self::Publication => 'P',
        }
    }

    pub fn from_letter(letter: char) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.letter() == letter)
    }

    /// The kind whose registers hold this one: publications of banns are
    /// kept with the marriages, so whatever holds marriages holds them.
    pub const fn filed_as(self) -> Self {
        match self {
            Self::Publication => Self::Marriage,
            other => other,
        }
    }
}

/// A series of records other than acts, which a citation names in words
/// where it would write an act code, or by its code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Series {
    /// `RP`, population censuses: the nominative lists of a commune and a
    /// year (French series M).
    Census,
    /// `RM`, military registers: the *registres matricules* of a class and a
    /// recruitment bureau, by matricule number (series R).
    MilitaryRegister,
    /// `CM`, conscription lists: conscripts, the contingent, the drawing of
    /// lots, the mobile national guard (series R).
    ConscriptionList,
    /// `TSA`, the tables of successions and absences of a registration
    /// office and a period (series Q).
    SuccessionTables,
    /// `RI`, the daily burial registers of a cemetery (*registres
    /// journaliers d'inhumation*), each burial under its entry number (*n°
    /// d'ordre*).
    CemeteryRegister,
}

impl Series {
    /// Every series, in the order its vocabulary is tried: a field naming
    /// two series is the first one's.
    pub const ALL: [Self; 5] = [
        Self::SuccessionTables,
        Self::MilitaryRegister,
        Self::ConscriptionList,
        Self::Census,
        Self::CemeteryRegister,
    ];

    /// The code a collection's `acts` and a normalized citation write.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Census => "RP",
            Self::MilitaryRegister => "RM",
            Self::ConscriptionList => "CM",
            Self::SuccessionTables => "TSA",
            Self::CemeteryRegister => "RI",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|series| series.code() == code)
    }

    /// The built-in French vocabulary: phrases of folded words, each of
    /// which a field must contain, in any order, to name the series.
    const fn phrases(self) -> &'static [&'static str] {
        match self {
            Self::Census => &["recensement", "liste nominative", "denombrement"],
            Self::MilitaryRegister => &["registre matricule", "matricule militaire"],
            Self::ConscriptionList => &[
                "conscrit",
                "conscription",
                "contingent",
                "tirage sort",
                "garde nationale mobile",
            ],
            Self::SuccessionTables => &["table succession", "succession absence"],
            Self::CemeteryRegister => &["registre journalier", "registre inhumation", "cimetiere"],
        }
    }

    /// The kind of record a document of this series is.
    pub const fn category(self) -> DocumentCategory {
        match self {
            Self::Census => DocumentCategory::Census,
            Self::MilitaryRegister | Self::ConscriptionList => DocumentCategory::MilitaryArchive,
            // Registration records of estates: the closest kind is that of
            // deeds, wills and inventories.
            Self::SuccessionTables => DocumentCategory::NotarialArchive,
            // A municipal record of a death, kept by the cemetery: the
            // closest kind is that of the civil records of deaths.
            Self::CemeteryRegister => DocumentCategory::CivilRecord,
        }
    }
}

impl fmt::Display for Series {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl TryFrom<String> for Series {
    type Error = String;

    fn try_from(code: String) -> Result<Self, Self::Error> {
        Self::from_code(&code).ok_or_else(|| format!("`{code}` is not a series code"))
    }
}

impl From<Series> for String {
    fn from(series: Series) -> Self {
        series.code().to_owned()
    }
}

/// Whether the folded words of a field contain every word of a phrase, a
/// word also matching its plural in `s` or `x`.
fn names_phrase(field_words: &[&str], phrase: &str) -> bool {
    let phrase = fold_words(phrase);
    !phrase.is_empty()
        && phrase.split(' ').all(|wanted| {
            field_words
                .iter()
                .any(|word| *word == wanted || word.strip_suffix(['s', 'x']) == Some(wanted))
        })
}

/// The kind of document a citation names, and an entry of a collection's
/// `acts`.
///
/// Written as a code: one act letter (`N`), several for a register mixing
/// them (`BMS`, `NMD`, `NPMD`), a table code starting with `T` (`TB`, `TD`),
/// kept as written, or a series code (`RP`, `RM`, `CM`, `TSA`, `RI`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Act {
    /// The act kinds, in the order written, each once.
    Register(Vec<ActKind>),
    /// A table, by its code.
    Table(String),
    /// A series of records other than acts.
    Series(Series),
}

impl Act {
    /// Reads a document kind's code, or `None` for any other text.
    pub fn from_code(code: &str) -> Option<Self> {
        if let Some(series) = Series::from_code(code) {
            return Some(Self::Series(series));
        }
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

    /// The act kinds of a register; none for a table or a series.
    pub fn kinds(&self) -> &[ActKind] {
        match self {
            Self::Register(kinds) => kinds,
            Self::Table(_) | Self::Series(_) => &[],
        }
    }

    /// Whether a register of these kinds holds acts of `kind`, publications
    /// of banns being filed with the marriages.
    pub fn includes(&self, kind: ActKind) -> bool {
        self.kinds()
            .iter()
            .any(|held| *held == kind || *held == kind.filed_as())
    }

    /// The kind a portal's act filter searches a register by: its first,
    /// whose category holds a mixed register, publications of banns being
    /// searched as marriages.
    pub fn primary_kind(&self) -> Option<ActKind> {
        self.kinds().first().map(|kind| kind.filed_as())
    }

    /// The kind of record a document of this act is, which attaching a
    /// cited view proposes (Archive Portals §6.4): a parish record for
    /// baptisms and burials, a civil record for births and deaths, and for
    /// marriages and banns alone one or the other by `year`; a table is of
    /// the acts its code names after the `T` (`TB`, `TD`), a civil record
    /// otherwise; a series is of its own kind. `None` for a marriage without
    /// a year.
    pub fn category(&self, year: Option<u16>) -> Option<DocumentCategory> {
        match self {
            Self::Register(kinds) => register_category(kinds, year),
            Self::Table(code) => match Self::from_code(&code[1..]) {
                Some(Self::Register(kinds)) => register_category(&kinds, year),
                _ => Some(DocumentCategory::CivilRecord),
            },
            Self::Series(series) => Some(series.category()),
        }
    }
}

fn register_category(kinds: &[ActKind], year: Option<u16>) -> Option<DocumentCategory> {
    let any = |wanted: &[ActKind]| kinds.iter().any(|kind| wanted.contains(kind));
    if any(&[ActKind::Baptism, ActKind::Burial]) {
        return Some(DocumentCategory::ParishRecord);
    }
    if any(&[ActKind::Birth, ActKind::Death]) {
        return Some(DocumentCategory::CivilRecord);
    }
    year.map(|year| {
        if year < FIRST_CIVIL_STATUS_YEAR {
            DocumentCategory::ParishRecord
        } else {
            DocumentCategory::CivilRecord
        }
    })
}

impl fmt::Display for Act {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Register(kinds) => kinds
                .iter()
                .try_for_each(|kind| write!(f, "{}", kind.letter())),
            Self::Table(code) => f.write_str(code),
            Self::Series(series) => f.write_str(series.code()),
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
///
/// A citation may give a register several call numbers, in fields of their
/// own — its microfilm's and its original's (`5MI825BIS - 4E 1927`) —:
/// they are kept together, joined by the citation's field separator, and
/// each is an [`alternative`](Self::alternatives).
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

    /// The call numbers cited: each field of a joined text when every one
    /// is shaped like a call number, the whole text otherwise (`MANS - LE`,
    /// `3 E 12 - 1`).
    pub fn alternatives(&self) -> Vec<&str> {
        let pieces: Vec<&str> = self.0.split(SEPARATOR).map(str::trim).collect();
        if pieces.len() > 1 && pieces.iter().all(|piece| Self::is_shaped(piece)) {
            pieces
        } else {
            vec![self.0.as_str()]
        }
    }

    /// Whether `other` names the same register as one of the cited call
    /// numbers ([`matched`](Self::matched)).
    pub fn matches(&self, other: &str) -> bool {
        self.matched(other) > 0
    }

    /// How many of the cited call numbers `other` carries: `3E73/14` matches
    /// `3 E 73 / 14`, and `4E212/45` matches `4 E 212 45`. A call number
    /// ending with a range of numbers (`5 Mi 9_374-376`, the microfilms of
    /// several years) also matches one it contains (`5 Mi 9_375`), and one
    /// that contains it. A portal's text may hold several call numbers —
    /// `4E 1927 / 5Mi 825 BIS [1134369/2]`, the original's, the microfilm's
    /// and an internal reference —, each of which is compared. A cited call
    /// number joining a digitization's to the original's (`9NUM/8E46`, a
    /// copy `9 NUM` of the register `8 E 46`) also matches a portal's text
    /// that is either alone: portals show one or the other.
    pub fn matched(&self, other: &str) -> usize {
        let written = Self::parts(other);
        self.alternatives()
            .into_iter()
            .filter(|mine| {
                written
                    .iter()
                    .any(|theirs| Self::same_register(mine, theirs))
                    || Self::slashed(mine)
                        .iter()
                        .any(|piece| Self::same_register(piece, other.trim()))
            })
            .count()
    }

    /// The call numbers a portal's text holds: the whole text, and, where
    /// it joins several, each of them — bracketed ones apart, and the rest
    /// cut at slashes when every piece has letters and digits (`4E 1927 /
    /// 5Mi 825 BIS`, while `9 E 250 / 1` is one call number).
    fn parts(text: &str) -> Vec<&str> {
        let mut parts = vec![text.trim()];
        let mut outside = text;
        if let Some((before, rest)) = text.split_once('[')
            && let Some((inside, after)) = rest.split_once(']')
            && after.trim().is_empty()
        {
            parts.push(inside.trim());
            outside = before.trim();
            parts.push(outside);
        }
        parts.extend(Self::slashed(outside));
        parts
    }

    /// The call numbers a text joins with slashes, when every piece has
    /// letters and digits (`4E 1927 / 5Mi 825 BIS`, `9NUM/8E46`); none
    /// otherwise (`9 E 250 / 1`, `3E73/14`: a slash before a bare number
    /// belongs to the call number).
    fn slashed(text: &str) -> Vec<&str> {
        let pieces: Vec<&str> = text.split('/').map(str::trim).collect();
        let call_like = |piece: &&str| {
            piece.chars().any(|c| c.is_ascii_alphabetic())
                && piece.chars().any(|c| c.is_ascii_digit())
        };
        if pieces.len() > 1 && pieces.iter().all(call_like) {
            pieces
        } else {
            Vec::new()
        }
    }

    /// Whether two single call numbers name the same register.
    fn same_register(mine: &str, theirs: &str) -> bool {
        let (upper_mine, upper_theirs) = (mine.to_uppercase(), theirs.to_uppercase());
        let (mine, theirs): (String, String) =
            (Self::folded(mine).collect(), Self::folded(theirs).collect());
        mine == theirs
            || Self::tokens(&upper_mine) == Self::tokens(&upper_theirs)
            || Self::numbered(&mine)
                .zip(Self::numbered(&theirs))
                .is_some_and(|((prefix, first, last), (other_prefix, from, to))| {
                    prefix == other_prefix
                        && ((first <= from && to <= last) || (from <= first && last <= to))
                })
    }

    /// A folded call number ending with a number or a range of numbers after
    /// a separator: its prefix, separator included, and the range.
    fn numbered(folded: &str) -> Option<(&str, u32, u32)> {
        let digits =
            |text: &str| text.len() - text.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        let last_digits = digits(folded);
        let last = folded[folded.len() - last_digits..].parse().ok()?;
        let rest = &folded[..folded.len() - last_digits];
        let (prefix, first) = match rest.strip_suffix('-') {
            Some(before) if digits(before) > 0 => {
                let first_digits = digits(before);
                let first = before[before.len() - first_digits..].parse().ok()?;
                (&before[..before.len() - first_digits], first)
            }
            _ => (rest, last),
        };
        (prefix.ends_with(['_', '/', '.', '-']) && first <= last).then_some((prefix, first, last))
    }

    fn folded(text: &str) -> impl Iterator<Item = char> + '_ {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_uppercase)
    }

    /// The runs of letters and of digits of a call number, read apart
    /// wherever a space, a separator or a change between letters and digits
    /// falls, numbers without their leading zeros: `4E212/45` is `4 E 212
    /// 45` (`4E21245` is not: its digits run together), and `9R0001` is
    /// `9 R 1`.
    fn tokens(folded: &str) -> Vec<&str> {
        Self::runs(folded)
            .into_iter()
            .map(|run| {
                let trimmed = run.trim_start_matches("0");
                if run.starts_with(|c: char| c.is_ascii_digit()) && !trimmed.is_empty() {
                    trimmed
                } else if run.starts_with("0") {
                    "0"
                } else {
                    run
                }
            })
            .collect()
    }

    /// The runs of letters and of digits of a call number ([`tokens`](Self::tokens)).
    fn runs(folded: &str) -> Vec<&str> {
        let mut tokens = Vec::new();
        let mut start = None;
        let mut digits = false;
        for (at, c) in folded.char_indices() {
            if !c.is_alphanumeric() {
                if let Some(from) = start.take() {
                    tokens.push(&folded[from..at]);
                }
                continue;
            }
            match start {
                Some(from) if c.is_ascii_digit() != digits => {
                    tokens.push(&folded[from..at]);
                    start = Some(at);
                }
                Some(_) => {}
                None => start = Some(at),
            }
            digits = c.is_ascii_digit();
        }
        if let Some(from) = start {
            tokens.push(&folded[from..]);
        }
        tokens
    }

    /// Whether a free field is shaped like a call number: letters and digits
    /// with a few separators, at least one digit and one capital, and no
    /// lowercase word (`acte 26`, `Registre 1877`), while `1 Mi 456` is one.
    pub(crate) fn is_shaped(field: &str) -> bool {
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
    /// Phrases naming a series, added to the built-in French vocabulary:
    /// `{"RP": ["dénombrement de population"]}`. Default none.
    pub series: BTreeMap<Series, Vec<String>>,
    /// Cities cited by their numbered districts (`Paris 11e`, `Paris XIe`,
    /// `11e arrondissement`), which recognition writes alike: `Paris 11e`.
    /// Default none.
    pub districts: Vec<District>,
    /// Other names citations give a locality, by the name the archive's
    /// portal knows it by: `{"Ivry": ["Ivry-sur-Seine"]}`. Default none.
    pub localities: BTreeMap<String, Vec<String>>,
    /// Where the archive's call numbers write their register's period:
    /// templates of text with a `{first}` year and an optional `{last}` one,
    /// four digits each, such as `RJ{first}{last}` for `PAN_RJ19171918_04`.
    /// A citation without a year takes the period of its call number.
    /// Default none.
    pub call_number_periods: Vec<String>,
}

/// A city whose citations name one of its numbered districts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct District {
    /// The city's name, as citations write it: `Paris`.
    pub city: String,
    /// How many districts it has, numbered from 1.
    pub count: u8,
}

impl Default for CitationGrammar {
    fn default() -> Self {
        Self {
            no_parish: vec!["(aucun)".to_owned()],
            view_words: vec!["vue".to_owned()],
            series: BTreeMap::new(),
            districts: Vec::new(),
            localities: BTreeMap::new(),
            call_number_periods: Vec::new(),
        }
    }
}

/// The year placeholders of a call-number period template.
const FIRST_YEAR: &str = "{first}";
const LAST_YEAR: &str = "{last}";

/// A call-number period template cut at its placeholders: the text before
/// `{first}`, between it and `{last}`, and after; `None` without `{last}`.
fn template_parts(template: &str) -> Option<(&str, Option<&str>, &str)> {
    let (before, rest) = template.split_once(FIRST_YEAR)?;
    let (between, after) = match rest.split_once(LAST_YEAR) {
        Some((between, after)) => (Some(between), after),
        None => (None, rest),
    };
    let clean = |text: &str| !text.chars().any(|c| "{}".contains(c));
    (clean(before) && between.is_none_or(clean) && clean(after)).then_some((before, between, after))
}

/// Four digits at the start of `text`, as a year.
fn leading_year(text: &str) -> Option<u16> {
    text.get(..4)
        .filter(|digits| digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
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
        if self
            .series
            .values()
            .flatten()
            .any(|phrase| fold_words(phrase).is_empty())
        {
            return Err("a series phrase needs a word".to_owned());
        }
        if self
            .districts
            .iter()
            .any(|district| fold_words(&district.city).is_empty() || district.count == 0)
        {
            return Err("a district rule needs a city and a count".to_owned());
        }
        if self
            .localities
            .iter()
            .any(|(name, others)| blank(std::slice::from_ref(name)) || blank(others))
        {
            return Err("locality names must not be blank".to_owned());
        }
        if self
            .call_number_periods
            .iter()
            .any(|template| template_parts(template).is_none())
        {
            return Err(
                "a call-number period is text around `{first}` and an optional `{last}`".to_owned(),
            );
        }
        Ok(())
    }

    /// The period a call number writes, as text (`1917-1918`), and its first
    /// year, by the first of `call_number_periods` it matches.
    pub fn call_number_period(&self, call_number: &str) -> Option<(String, u16)> {
        self.call_number_periods.iter().find_map(|template| {
            let (before, between, after) = template_parts(template)?;
            call_number.match_indices(before).find_map(|(at, _)| {
                let rest = &call_number[at + before.len()..];
                let first = leading_year(rest)?;
                let rest = &rest[4..];
                let (last, rest) = match between {
                    Some(between) => {
                        let rest = rest.strip_prefix(between)?;
                        (leading_year(rest)?, &rest[4..])
                    }
                    None => (first, rest),
                };
                if first > last || !rest.starts_with(after) {
                    return None;
                }
                let period = if first == last {
                    first.to_string()
                } else {
                    format!("{first}-{last}")
                };
                Some((period, first))
            })
        })
    }

    /// The name the archive's portal knows a locality by, when citations
    /// give it another (`localities`), compared folded.
    pub fn locality_name(&self, written: &str) -> Option<&str> {
        let wanted = fold_words(written);
        self.localities
            .iter()
            .find(|(_, others)| others.iter().any(|other| fold_words(other) == wanted))
            .map(|(name, _)| name.as_str())
    }

    fn is_no_parish(&self, field: &str) -> bool {
        self.no_parish.iter().any(|value| value == field)
    }

    /// The series a field names: by its code, or in words of the built-in
    /// vocabulary or of this archive's additions, compared folded (case,
    /// accents and punctuation ignored).
    pub fn series_of(&self, field: &str) -> Option<Series> {
        if let Some(series) = Series::from_code(field) {
            return Some(series);
        }
        let folded = fold_words(field);
        let words: Vec<&str> = folded.split(' ').collect();
        Series::ALL.into_iter().find(|series| {
            series
                .phrases()
                .iter()
                .copied()
                .chain(
                    self.series
                        .get(series)
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
                )
                .any(|phrase| names_phrase(&words, phrase))
        })
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
/// locality and the act is optional; the locality is empty only for a series
/// cited without one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CitationParts {
    /// The archive code the title starts with, such as `AD44`.
    pub code: String,
    /// The locality, which may itself contain ` - `; for a military series,
    /// the recruitment bureau.
    pub locality: String,
    pub parish: Option<String>,
    /// The kind of document: an act, a table or a series.
    pub act: Act,
    /// The first year of the period, Gregorian; a military series' class.
    pub year: Option<u16>,
    /// The period as written: `1877`, `1702-1703`, `an XII`.
    pub period: Option<String>,
    pub call_number: Option<CallNumber>,
    /// The act or matricule number: `acte 26`, `matricule 1268`, or a bare
    /// `348`. Compared with the numbers a register spans, never sent.
    pub number: Option<u32>,
    /// The cited views in order; empty when the title cites none, or cites
    /// one beyond its own view count.
    pub views: Vec<CitedView>,
    /// The register's image count, as cited.
    pub view_count: Option<u16>,
}

/// What the free fields after the act or the series give.
struct Tail {
    call_number: Option<CallNumber>,
    number: Option<u32>,
    views: Vec<CitedView>,
    view_count: Option<u16>,
}

impl Tail {
    /// Reads the free fields, the last of which may cite the views: after a
    /// view word (`vue 5d/13`), or bare (`579/833`) after a number.
    fn read(mut rest: &[&str], grammar: &CitationGrammar) -> Self {
        let mut views = Vec::new();
        let mut view_count = None;
        if let Some((free, spec)) = rest.split_last().and_then(|(last, free)| {
            let spec = grammar.view_spec(last).or_else(|| {
                let after_number = free.last().is_some_and(|field| number_of(field).is_some());
                (after_number && parse_views(last).is_some()).then_some(*last)
            })?;
            Some((free, spec))
        }) {
            rest = free;
            (views, view_count) = parse_views(spec).unwrap_or_default();
        }
        let call_numbers: Vec<&str> = rest
            .iter()
            .copied()
            .filter(|field| CallNumber::is_shaped(field))
            .collect();
        Self {
            call_number: (!call_numbers.is_empty())
                .then(|| CallNumber::new(call_numbers.join(SEPARATOR))),
            number: rest.iter().find_map(|field| number_of(field)),
            views,
            view_count,
        }
    }
}

impl CitationParts {
    /// Reads a normalized source title, or `None` for any other title: an
    /// act code wins, and a title without one may name a series in words.
    pub fn parse(title: &str, grammar: &CitationGrammar) -> Option<Self> {
        let fields: Vec<&str> = title.split(SEPARATOR).map(str::trim).collect();
        let code = code_of(title)?;
        let mut parts = find_act(&fields)
            .and_then(|act_at| Self::parse_act(code, &fields, act_at, grammar))
            .or_else(|| Self::parse_series(code, &fields, grammar))?;
        if parts.year.is_none()
            && let Some((period, year)) = parts
                .call_number
                .as_ref()
                .and_then(|call_number| grammar.call_number_period(call_number.as_str()))
        {
            parts.year = Some(year);
            parts.period = Some(period);
        }
        Some(parts)
    }

    /// `<code> - <locality> - <parish> - <act> - <period> - <free…>`, or
    /// without the parish, `<code> - <locality> - <act> - <period> - <free…>`.
    fn parse_act(
        code: &str,
        fields: &[&str],
        act_at: usize,
        grammar: &CitationGrammar,
    ) -> Option<Self> {
        let parish_at = (act_at > 2).then(|| act_at - 1);
        let locality = fields[1..parish_at.unwrap_or(act_at)].join(SEPARATOR);
        if locality.is_empty() || grammar.is_no_parish(&locality) {
            return None;
        }
        let parish = parish_at
            .map(|at| fields[at])
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
            period = Some(exact_period(field).to_owned());
            year = Some(first_year);
            rest = &rest[1..];
        }
        let tail = Tail::read(rest, grammar);
        Some(Self {
            code: code.to_owned(),
            locality,
            parish,
            act,
            year,
            period,
            call_number: tail.call_number,
            number: tail.number,
            views: tail.views,
            view_count: tail.view_count,
        })
    }

    /// `<code> - [<locality>] - [<period>] - <series> - [<period>] - <free…>`:
    /// the first field naming a series, the fields before it the locality
    /// (none, or a `no_parish` value, for a series without one). The period
    /// is the field after the series, or else the one before it, or else a
    /// year in parentheses in a free field (`Bureau de … n° 1 à 1586 (1870)`).
    fn parse_series(code: &str, fields: &[&str], grammar: &CitationGrammar) -> Option<Self> {
        let (at, series) = fields
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(at, field)| Some((at, grammar.series_of(field)?)))?;
        let mut before = &fields[1..at];
        let mut rest = &fields[at + 1..];
        let mut period = None;
        if let Some((first, after)) = rest.split_first()
            && let Some(year) = period_start(first)
        {
            period = Some((exact_period(first), year));
            rest = after;
        } else if let Some((last, head)) = before.split_last()
            && let Some(year) = period_start(last)
        {
            period = Some((exact_period(last), year));
            before = head;
        }
        if let Some((last, head)) = before.split_last()
            && grammar.is_no_parish(last)
        {
            before = head;
        }
        let period = period.or_else(|| rest.iter().find_map(|field| parenthesized_period(field)));
        let tail = Tail::read(rest, grammar);
        let mut locality = before.join(SEPARATOR);
        if locality.is_empty() {
            // `Cimetière parisien de Exampleville`: the series names its place.
            locality = place_in(fields[at]).unwrap_or_default().to_owned();
        }
        Some(Self {
            code: code.to_owned(),
            locality,
            parish: None,
            act: Act::Series(series),
            year: period.map(|(_, year)| year),
            period: period.map(|(text, _)| text.to_owned()),
            call_number: tail.call_number,
            number: tail.number,
            views: tail.views,
            view_count: tail.view_count,
        })
    }

    /// The kind of record the cited document is (Archive Portals §6.4).
    pub fn category(&self) -> Option<DocumentCategory> {
        self.act.category(self.year)
    }
}

/// An act, matricule or entry number: `acte 26`, `matricule 1268`, `n° 12`,
/// `ordre 945`, or a bare `348`.
fn number_of(field: &str) -> Option<u32> {
    let folded = fold_words(field);
    let digits = match folded.rsplit_once(' ') {
        Some((words, digits)) if NUMBER_WORDS.contains(&words) => digits,
        Some(_) => return None,
        None => folded.as_str(),
    };
    let valid = (1..=7).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit());
    valid.then(|| digits.parse().ok()).flatten()
}

/// The place a field naming a series names after a preposition, its name
/// capitalized: `Exampleville` in `Cimetière parisien de Exampleville` or
/// `cimetière d'Exampleville`, `Exemple` in `Cimetière de l'Exemple`.
fn place_in(field: &str) -> Option<&str> {
    const PREPOSITIONS: [&str; 5] = ["de ", "du ", "des ", "d'", "d\u{2019}"];
    const ARTICLES: [&str; 4] = ["l'", "L'", "l\u{2019}", "L\u{2019}"];
    let starts_word = |at: usize| at == 0 || field[..at].ends_with(' ');
    field
        .char_indices()
        .filter(|(at, _)| starts_word(*at))
        .find_map(|(at, _)| {
            let rest = &field[at..];
            let after = PREPOSITIONS
                .into_iter()
                .find_map(|preposition| rest.strip_prefix(preposition))?
                .trim_start();
            let after = ARTICLES
                .into_iter()
                .find_map(|article| after.strip_prefix(article))
                .unwrap_or(after);
            after.starts_with(char::is_uppercase).then_some(after)
        })
}

/// A period written in parentheses within a free field, such as the class
/// year of `Bureau de Exampleville n° 1 à 1586 (1870)`.
fn parenthesized_period(field: &str) -> Option<(&str, u16)> {
    field.split('(').skip(1).find_map(|after| {
        let inner = after.split_once(')')?.0.trim();
        Some((inner, period_start(inner)?))
    })
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
/// is the fourth field or a later one; an act code followed by a period wins
/// over an earlier one that is not. A citation leaving the parish out
/// (`AD75 - Paris 11e - N - 1917`) writes its act third, followed by a
/// period, which is read when no later field is a dated act code.
fn find_act(fields: &[&str]) -> Option<usize> {
    let dated = |at: usize| {
        fields
            .get(at + 1)
            .is_some_and(|next| period_start(next).is_some())
    };
    let mut first = None;
    for (at, field) in fields.iter().enumerate().skip(3) {
        if Act::from_code(field).is_none() {
            continue;
        }
        if dated(at) {
            return Some(at);
        }
        first.get_or_insert(at);
    }
    let without_parish = fields
        .get(2)
        .is_some_and(|field| Act::from_code(field).is_some())
        && dated(2);
    if without_parish { Some(2) } else { first }
}

/// The Gregorian first year of a period field: `1877`, `1702-1703`, `an XII`,
/// `an XI-an XII`, `an XI-XII`, a note in parentheses after it left aside
/// (`1931 (A-H, collection communale)`).
fn period_start(field: &str) -> Option<u16> {
    let field = exact_period(field);
    let field = field
        .trim_end()
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .map(|(period, _)| period.trim())
        .filter(|period| !period.is_empty())
        .unwrap_or(field);
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

/// The number of an ordinal written with its French suffix, in any case
/// and with or without accents: `3e`, `1re`, `1er`, `2ème`, `2nde`.
pub(crate) fn ordinal(word: &str) -> Option<u32> {
    let folded = fold_words(word);
    let digits = folded.find(|c: char| !c.is_ascii_digit())?;
    let number = folded[..digits].parse().ok().filter(|number| *number > 0)?;
    ["e", "er", "re", "ere", "eme", "nd", "nde"]
        .contains(&&folded[digits..])
        .then_some(number)
}

/// The words marking a period as approximate before it, as the French
/// vocabulary's `approximate` list writes them (`env. 1792-1952`, `vers
/// 1750`), with the tilde (`~1850`).
const APPROXIMATE: [&str; 8] = [
    "environ ", "env. ", "env ", "vers ", "circa ", "ca. ", "ca ", "c. ",
];

/// A period field without the word marking it as approximate: its years are
/// the register's all the same.
fn exact_period(field: &str) -> &str {
    let field = field.trim();
    if let Some(rest) = field.strip_prefix('~') {
        return rest.trim_start();
    }
    APPROXIMATE
        .iter()
        .find_map(|word| {
            field
                .get(..word.len())
                .filter(|head| head.eq_ignore_ascii_case(word))
                .map(|_| field[word.len()..].trim_start())
        })
        .unwrap_or(field)
}

/// A Gregorian year of four digits, alone or ending a date (`05/03/1871`,
/// `26 juillet 1849`, `1er juillet 1849`), or a Republican year `an <n>`.
fn year_of(text: &str) -> Option<u16> {
    if text.len() == 4 && text.bytes().all(|byte| byte.is_ascii_digit()) {
        return text.parse().ok();
    }
    if let Some(at) = text.len().checked_sub(4)
        && text.is_char_boundary(at)
        && text[at..].bytes().all(|byte| byte.is_ascii_digit())
        && day_before(&text[..at]).is_some()
    {
        return text[at..].parse().ok();
    }
    let numeral = text
        .get(..3)
        .filter(|prefix| prefix.eq_ignore_ascii_case("an "))
        .and_then(|_| text.get(3..))?;
    republican_start(republican_numeral(numeral.trim())?)
}

/// The French month names, folded, which full dates write between the years
/// of a period.
pub(crate) const MONTHS: [&str; 12] = [
    "janvier",
    "fevrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "aout",
    "septembre",
    "octobre",
    "novembre",
    "decembre",
];

/// The month, and the day if written, that a date writes just before its
/// year: `05/03/` (`05/03/1871`), `26 juillet ` or `1er juillet `.
pub(crate) fn day_before(before: &str) -> Option<(u32, Option<u32>)> {
    /// The one or two digits `text` ends with, and what precedes them.
    fn trailing_number(text: &str) -> Option<(&str, u32)> {
        let digits = text.len() - text.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        if !(1..=2).contains(&digits) {
            return None;
        }
        let at = text.len() - digits;
        Some((&text[..at], text[at..].parse().ok()?))
    }
    let day_of = |day: u32| (1..=31).contains(&day).then_some(day);
    if let Some(rest) = before.strip_suffix('/') {
        let (rest, month) = trailing_number(rest)?;
        if !(1..=12).contains(&month) {
            return None;
        }
        let day = rest
            .strip_suffix('/')
            .and_then(trailing_number)
            .and_then(|(_, day)| day_of(day));
        return Some((month, day));
    }
    let mut words = before.split_whitespace().rev();
    let named = fold_words(words.next()?);
    let month = MONTHS.iter().position(|month| named == *month)?;
    let day = words.next().and_then(|word| {
        let word = word.rsplit(['-', '\u{2013}']).next().unwrap_or(word);
        if fold_words(word) == "1er" {
            Some(1)
        } else {
            trailing_number(word)
                .filter(|(rest, _)| rest.is_empty())
                .and_then(|(_, day)| day_of(day))
        }
    });
    Some((u32::try_from(month).ok()? + 1, day))
}

/// A Republican year number, in Roman or Arabic numerals.
pub(crate) fn republican_numeral(text: &str) -> Option<i32> {
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
pub(crate) fn republican_start(year: i32) -> Option<u16> {
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
pub(crate) fn view_range(first: CitedView, last: CitedView) -> Option<Vec<CitedView>> {
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
    fn keeps_every_call_number_a_citation_gives() {
        let citation = parse(
            "AD99 - Exampleville - (aucun) - NMD - 1805-1821 - 5MI999BIS - 4E 9927 - 9999999/2 - vue 55d/425",
        )
        .expect("a normalized citation");
        let call_number = citation.call_number.expect("call numbers");
        assert_eq!(call_number.as_str(), "5MI999BIS - 4E 9927");
        assert_eq!(call_number.alternatives(), ["5MI999BIS", "4E 9927"]);
        // A portal's cell joining the original's, the microfilm's and an
        // internal reference.
        assert_eq!(call_number.matched("4E 9927 / 5Mi 999 BIS [9999999/2]"), 2);
        assert_eq!(call_number.matched("4E 9926 / 5Mi 999 BIS [9999999/1]"), 1);
        assert_eq!(call_number.matched("4E 9928"), 0);
        // Fields that are not each a call number stay one.
        assert_eq!(CallNumber::new("3 E 12 - 1").alternatives(), ["3 E 12 - 1"]);
    }

    #[test]
    fn compares_call_numbers_by_their_letters_and_digits() {
        let cited = CallNumber::new("4E212/45");
        assert!(cited.matches("4 E 212 45"));
        assert!(cited.matches("4E 212/45"));
        assert!(!cited.matches("4 E 21245"));
        assert!(!cited.matches("4 E 212 46"));
        // A slash before a bare number belongs to the call number.
        assert!(!CallNumber::new("9 E 250").matches("9 E 250 / 1"));
        assert!(!CallNumber::new("3E1").matches("3E1/2"));
        // Numbers padded with zeros.
        assert!(CallNumber::new("9 R 1").matches("9R0001"));
        assert!(!CallNumber::new("9 R 1").matches("9R0010"));
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
            ("05/03/1871-28/03/1871", 1871),
            ("26 juillet 1849-1er février 1850", 1849),
            ("1er juillet 1849", 1849),
        ] {
            let citation = parse(&format!("AD44 - Exampleville - (aucun) - D - {period}"))
                .expect("a normalized citation");
            assert_eq!(citation.year, Some(year), "{period}");
            assert_eq!(citation.period.as_deref(), Some(period));
        }
        // An approximate period: its years, without the word marking it.
        for (written, period, year) in [
            ("env 1792-1952", "1792-1952", 1792),
            ("env. 1792-1952", "1792-1952", 1792),
            ("Vers 1750", "1750", 1750),
            ("ca 1750", "1750", 1750),
            ("circa an XII", "an XII", 1803),
            ("~1850", "1850", 1850),
        ] {
            let citation = parse(&format!("AD44 - Exampleville - (aucun) - TD - {written}"))
                .expect("a normalized citation");
            assert_eq!(citation.year, Some(year), "{written}");
            assert_eq!(citation.period.as_deref(), Some(period), "{written}");
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
    fn a_range_of_microfilms_matches_the_ones_it_holds() {
        let range = CallNumber::new("9Mi 999_374-376");
        for (written, matches) in [
            ("9Mi 999_374-376", true),
            ("9 Mi 999_375", true),
            ("9Mi 999_374-375", true),
            ("9Mi 999_370-380", true),
            ("9Mi 999_371-373", false),
            ("9Mi 999_377", false),
            ("9Mi 998_375", false),
        ] {
            assert_eq!(range.matches(written), matches, "{written}");
        }
        assert!(CallNumber::new("9Mi 999_375").matches("9Mi 999_374-376"));
        // A number is not a range of the numbers it starts with.
        assert!(!CallNumber::new("3 E 73 / 14").matches("3 E 73 / 1"));
        assert!(!CallNumber::new("1 R 1213").matches("1 R 1213-1215"));
    }

    #[test]
    fn a_digitization_s_call_number_matches_either_part() {
        let joined = CallNumber::new("9NUM/8E99");
        for (written, matches) in [
            ("9 NUM /8E99", true),
            ("8 E 99", true),
            ("9 NUM", true),
            ("8 E 98", false),
            ("9 NUM /8E98", false),
        ] {
            assert_eq!(joined.matches(written), matches, "{written}");
        }
        assert!(CallNumber::new("8 E 99").matches("9 NUM /8E99"));
        // A volume after a slash is no second call number.
        assert!(!CallNumber::new("3E73/14").matches("14"));
        assert!(!CallNumber::new("4 E 8050/10").matches("4 E 8050"));
    }

    #[test]
    fn reads_a_census_year_followed_by_its_part() {
        let citation = parse(
            "AD99 - Exampleville - Recensement - 1931 (A-H, collection communale) - 9 Mi 9999_ 19 - vue 490d/662",
        )
        .expect("a census citation");
        assert_eq!(citation.act, Act::Series(Series::Census));
        assert_eq!(citation.locality, "Exampleville");
        assert_eq!(citation.year, Some(1931));
        assert_eq!(
            citation.call_number.as_ref().map(CallNumber::as_str),
            Some("9 Mi 9999_ 19")
        );
        assert!(citation.call_number.unwrap().matches("9 Mi 9999_19"));
        assert_eq!(citation.views, [view(490, Some(Side::Right))]);
        assert_eq!(citation.view_count, Some(662));
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
            ..CitationGrammar::default()
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

    /// The act, locality, year, period, call number, number, views and view
    /// count of a series citation.
    type SeriesParts = (
        Act,
        String,
        Option<u16>,
        Option<String>,
        Option<String>,
        Option<u32>,
        Vec<CitedView>,
        Option<u16>,
    );

    /// A series citation's parts that tell registers apart.
    fn series_parts(title: &str) -> SeriesParts {
        let citation = parse(title).unwrap_or_else(|| panic!("a series citation: {title}"));
        assert_eq!(citation.code, "AD99", "{title}");
        assert_eq!(citation.parish, None, "{title}");
        (
            citation.act,
            citation.locality,
            citation.year,
            citation.period,
            citation.call_number.map(|call| call.as_str().to_owned()),
            citation.number,
            citation.views,
            citation.view_count,
        )
    }

    #[test]
    fn reads_a_census_named_in_words() {
        let census = Act::Series(Series::Census);
        let s = |text: &str| Some(text.to_owned());
        assert_eq!(
            series_parts("AD99 - Exampleville - Recensement - 1872 - 6 M 999 - vue 12g/40"),
            (
                census.clone(),
                "Exampleville".to_owned(),
                Some(1872),
                s("1872"),
                s("6 M 999"),
                None,
                vec![view(12, Some(Side::Left))],
                Some(40)
            )
        );
        // A free field between the year and the call number.
        assert_eq!(
            series_parts(
                "AD99 - Exampleville - Recensement - 1866 - Canton est - 7 M 999 - vue 204d/242"
            ),
            (
                census.clone(),
                "Exampleville".to_owned(),
                Some(1866),
                s("1866"),
                s("7 M 999"),
                None,
                vec![view(204, Some(Side::Right))],
                Some(242)
            )
        );
        // Variant wording, the year after or before the series.
        for title in [
            "AD99 - Exampleville - Recensements de population des communes - 1936 - D2M8/999 - vue 189d/260",
            "AD99 - Exampleville - 1936 - Recensements de population des communes - D2M8/999 - vue 189d/260",
        ] {
            assert_eq!(
                series_parts(title),
                (
                    census.clone(),
                    "Exampleville".to_owned(),
                    Some(1936),
                    s("1936"),
                    s("D2M8/999"),
                    None,
                    vec![view(189, Some(Side::Right))],
                    Some(260)
                ),
                "{title}"
            );
        }
        // The normalized form with the series code reads the same.
        let citation = parse("AD99 - Exampleville - (aucun) - RP - 1872 - 6 M 999 - vue 12/40")
            .expect("a normalized citation");
        assert_eq!(
            (citation.act, citation.year, citation.parish),
            (census, Some(1872), None)
        );
    }

    #[test]
    fn reads_succession_and_absence_tables() {
        let tables = Act::Series(Series::SuccessionTables);
        let s = |text: &str| Some(text.to_owned());
        assert_eq!(
            series_parts(
                "AD99 - Exampleville - Tables des successions et absences - 1897-1898 - Q_NUM_EXA_50 - vue 13/159"
            ),
            (
                tables.clone(),
                "Exampleville".to_owned(),
                Some(1897),
                s("1897-1898"),
                s("Q_NUM_EXA_50"),
                None,
                vec![view(13, None)],
                Some(159)
            )
        );
        for wording in [
            "Table des successions et absences",
            "Table alphabétique des successions et absences",
        ] {
            assert_eq!(
                series_parts(&format!(
                    "AD99 - Exampleville - {wording} - 1895-1913 - 3 Q 9999 - acte 31 - vue 102/181"
                )),
                (
                    tables.clone(),
                    "Exampleville".to_owned(),
                    Some(1895),
                    s("1895-1913"),
                    s("3 Q 9999"),
                    Some(31),
                    vec![view(102, None)],
                    Some(181)
                ),
                "{wording}"
            );
        }
    }

    #[test]
    fn reads_military_registers_and_conscription_lists() {
        let registers = Act::Series(Series::MilitaryRegister);
        let lists = Act::Series(Series::ConscriptionList);
        let s = |text: &str| Some(text.to_owned());
        // The locality is the recruitment bureau and the year the class; a
        // bare matricule, then a bare view.
        assert_eq!(
            series_parts(
                "AD99 - Exampleville - Registres matricules - 1898 - 1 R 9999 - 348 - 579/833"
            ),
            (
                registers.clone(),
                "Exampleville".to_owned(),
                Some(1898),
                s("1898"),
                s("1 R 9999"),
                Some(348),
                vec![view(579, None)],
                Some(833)
            )
        );
        // The call number before a free field holding the class.
        assert_eq!(
            series_parts(
                "AD99 - Exampleville - Registre matricules - 1 R 999 - \
                 Bureau de Exampleville n° 1 à 1586 (1870) - matricule 1268 - vue 319/436"
            ),
            (
                registers.clone(),
                "Exampleville".to_owned(),
                Some(1870),
                s("1870"),
                s("1 R 999"),
                Some(1268),
                vec![view(319, None)],
                Some(436)
            )
        );
        // No locality; the years in the series' own name are not its period.
        assert_eq!(
            series_parts(
                "AD99 - Registres matricules des classes 1859 à 1940 - 1871 - 1 R 9999 - vue 181/196"
            ),
            (
                registers,
                String::new(),
                Some(1871),
                s("1871"),
                s("1 R 9999"),
                None,
                vec![view(181, None)],
                Some(196)
            )
        );
        assert_eq!(
            series_parts("AD99 - Exampleville - Conscrits militaires - 1897"),
            (
                lists.clone(),
                "Exampleville".to_owned(),
                Some(1897),
                s("1897"),
                None,
                None,
                Vec::new(),
                None
            )
        );
        assert_eq!(
            series_parts(
                "AD99 - Exampleville - Liste départementale du contingent et de la garde nationale mobile \
                 - 1 R 999 - matricule 2189 - vue 183g/476"
            ),
            (
                lists,
                "Exampleville".to_owned(),
                None,
                None,
                s("1 R 999"),
                Some(2189),
                vec![view(183, Some(Side::Left))],
                Some(476)
            )
        );
    }

    #[test]
    fn a_bare_view_follows_a_number_only() {
        let citation = parse("AD99 - Exampleville - Registres matricules - 1898 - 579/833")
            .expect("a series citation");
        assert!(citation.views.is_empty());
        let citation = parse("AD44 - Exampleville - (aucun) - N - 1877 - acte 26 - 5/13")
            .expect("a normalized citation");
        assert_eq!(
            (citation.number, citation.views, citation.view_count),
            (Some(26), vec![view(5, None)], Some(13))
        );
    }

    #[test]
    fn an_act_code_wins_over_series_words() {
        let citation = parse("AD44 - Exampleville - (aucun) - N - 1877 - Recensement")
            .expect("a normalized citation");
        assert_eq!(citation.act, Act::Register(vec![ActKind::Birth]));
        // Neither an act nor a series: no citation.
        assert_eq!(parse("AD99 - Exampleville - Cadastre - 1830"), None);
    }

    #[test]
    fn reads_parish_tables_and_publications_of_banns() {
        for code in ["TB", "TM", "TS", "TN", "TD"] {
            let citation =
                parse(&format!("AD99 - Exampleville - (aucun) - {code} - 1750")).expect("a table");
            assert_eq!(citation.act, Act::Table(code.to_owned()));
        }
        use ActKind::{Birth, Death, Marriage, Publication};
        for (code, kinds) in [
            ("NPMD", vec![Birth, Publication, Marriage, Death]),
            ("NMDP", vec![Birth, Marriage, Death, Publication]),
            ("PM", vec![Publication, Marriage]),
            ("P", vec![Publication]),
        ] {
            let act = Act::from_code(code).expect("an act code");
            assert_eq!(act, Act::Register(kinds.clone()), "{code}");
            assert_eq!(act.to_string(), code);
            // Banns are searched, and held, as marriages.
            assert_eq!(act.primary_kind(), Some(kinds[0].filed_as()), "{code}");
            assert!(Act::from_code("NMD").unwrap().includes(Publication));
        }
        assert_eq!(Act::from_code("PP"), None);
    }

    #[test]
    fn series_codes_serialize_like_acts() {
        for series in Series::ALL {
            let act = Act::Series(series);
            let json = serde_json::to_string(&act).unwrap();
            assert_eq!(json, format!("\"{}\"", series.code()));
            assert_eq!(serde_json::from_str::<Act>(&json).unwrap(), act);
        }
        // `TSA` is the succession tables, not a table code.
        assert_eq!(
            Act::from_code("TSA"),
            Some(Act::Series(Series::SuccessionTables))
        );
        assert!(serde_json::from_str::<Series>("\"XX\"").is_err());
    }

    #[test]
    fn an_archive_adds_series_words() {
        let grammar: CitationGrammar = serde_json::from_str(
            r#"{"series": {"RP": ["Dénombrement des habitants"], "CM": ["levée"]}}"#,
        )
        .unwrap();
        assert!(grammar.validate().is_ok());
        assert_eq!(grammar.no_parish, ["(aucun)"]);
        let title = "AD99 - Exampleville - Levées militaires - 1813";
        let citation = CitationParts::parse(title, &grammar).expect("a series citation");
        assert_eq!(citation.act, Act::Series(Series::ConscriptionList));
        assert_eq!(parse(title), None);
        // The built-in vocabulary stays.
        assert_eq!(
            grammar.series_of("Recensement de population"),
            Some(Series::Census)
        );
        assert_eq!(
            grammar.series_of("DENOMBREMENT des habitants"),
            Some(Series::Census)
        );

        assert!(serde_json::from_str::<CitationGrammar>(r#"{"series": {"XX": ["a"]}}"#).is_err());
        let blank: CitationGrammar =
            serde_json::from_str(r#"{"series": {"RP": [" - "]}}"#).unwrap();
        assert!(blank.validate().is_err());
    }

    #[test]
    fn reads_an_act_cited_without_its_parish() {
        let citation =
            parse("AD99 - Exampleville 11e - D - 1918 - 12D 999 - acte 4343 - vue 3d/31")
                .expect("a normalized citation");
        assert_eq!(citation.locality, "Exampleville 11e");
        assert_eq!(citation.parish, None);
        assert_eq!(citation.act, Act::Register(vec![ActKind::Death]));
        assert_eq!(citation.year, Some(1918));
        assert_eq!(citation.call_number, Some(CallNumber::new("12D 999")));
        assert_eq!(citation.number, Some(4343));
        assert_eq!(citation.views, [view(3, Some(Side::Right))]);
        assert_eq!(citation.view_count, Some(31));
        // A dated act code after the parish field still wins.
        let citation = parse("AD99 - Exampleville - N - Saint-Exemple - B - 1750").unwrap();
        assert_eq!(citation.locality, "Exampleville - N");
        assert_eq!(citation.parish.as_deref(), Some("Saint-Exemple"));
        assert_eq!(citation.act, Act::Register(vec![ActKind::Baptism]));
    }

    /// The grammar of an archive whose cemetery registers are cited with a
    /// `C` and a call number writing their period.
    fn cemetery_grammar() -> CitationGrammar {
        serde_json::from_str(
            r#"{
                "series": {"RI": ["C", "inhumation"]},
                "call_number_periods": ["_RJ{first}{last}_"]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn reads_a_cemetery_register_and_its_entry_number() {
        let grammar = cemetery_grammar();
        assert!(grammar.validate().is_ok());
        let parts = |title: &str| {
            let citation = CitationParts::parse(title, &grammar)
                .unwrap_or_else(|| panic!("a cemetery citation: {title}"));
            (
                citation.act,
                citation.locality,
                citation.year,
                citation.period,
                citation.call_number.map(|call| call.as_str().to_owned()),
                citation.number,
                citation.views,
                citation.view_count,
            )
        };
        let register = Act::Series(Series::CemeteryRegister);
        let s = |text: &str| Some(text.to_owned());
        // The year from the call number when the citation writes none.
        assert_eq!(
            parts("AD99 - Exampleville - C - XXX_RJ19041904_01 - ordre 945 - vue 18/31"),
            (
                register.clone(),
                "Exampleville".to_owned(),
                Some(1904),
                s("1904"),
                s("XXX_RJ19041904_01"),
                Some(945),
                vec![view(18, None)],
                Some(31),
            )
        );
        // The citation's own year wins.
        assert_eq!(
            parts("AD99 - Exampleville - C - 1918 - XXX_RJ19171918_03 - ordre 981 - vue 20/31").2,
            Some(1918)
        );
        // A register over two years.
        let (_, _, year, period, ..) =
            parts("AD99 - Exampleville - C - XXX_RJ19171918_04 - ordre 1500 - vue 9/31");
        assert_eq!((year, period), (Some(1917), s("1917-1918")));
        // The series in words, and the number after `n° d'ordre`.
        for title in [
            "AD99 - Exampleville - Registres journaliers d'inhumation - XXX_RJ19041904_01 - n° d'ordre 945 - vue 18/31",
            "AD99 - Exampleville - inhumation - 1904 - XXX_RJ19041904_01 - numéro d'ordre 945 - vue 18/31",
            "AD99 - Cimetière parisien de Exampleville - XXX_RJ19041904_01 - ordre 945 - vue 18/31",
            "AD99 - Cimetière d'Exampleville - 1904 - XXX_RJ19041904_01 - ordre 945 - vue 18/31",
        ] {
            let (act, locality, year, _, call_number, number, ..) = parts(title);
            assert_eq!(
                (act, locality.as_str(), year, call_number.as_deref(), number),
                (
                    register.clone(),
                    "Exampleville",
                    Some(1904),
                    Some("XXX_RJ19041904_01"),
                    Some(945)
                ),
                "{title}"
            );
        }
        // `C` names the series only where the archive says so; the
        // built-in words do everywhere.
        assert_eq!(
            parse("AD99 - Exampleville - C - XXX_RJ19041904_01 - ordre 945"),
            None
        );
        assert_eq!(
            parse("AD99 - Exampleville - Cimetière - 1904 - ordre 945").map(|parts| parts.act),
            Some(register)
        );
        // Without the archive's call-number periods, no year.
        assert_eq!(
            parse("AD99 - Exampleville - Cimetière - XXX_RJ19041904_01 - ordre 945")
                .unwrap()
                .year,
            None
        );
    }

    #[test]
    fn reads_the_period_a_call_number_writes() {
        let grammar = cemetery_grammar();
        let period = |call_number: &str| grammar.call_number_period(call_number);
        assert_eq!(period("XXX_RJ19041904_01"), Some(("1904".to_owned(), 1904)));
        assert_eq!(
            period("XXX_RJ18601869_01"),
            Some(("1860-1869".to_owned(), 1860))
        );
        for other in [
            "XXX_RJ1904_01",
            "XXX_RJ19051904_01",
            "XXX_RI19041904_01",
            "4E 1234",
        ] {
            assert_eq!(period(other), None, "{other}");
        }
        let single: CitationGrammar =
            serde_json::from_str(r#"{"call_number_periods": ["E {first}/"]}"#).unwrap();
        assert_eq!(
            single.call_number_period("3 E 1877/12"),
            Some(("1877".to_owned(), 1877))
        );
        for templates in [r#"["RJ"]"#, r#"["{last}{first}"]"#, r#"["{first}{other}"]"#] {
            let grammar: CitationGrammar =
                serde_json::from_str(&format!(r#"{{"call_number_periods": {templates}}}"#))
                    .unwrap();
            assert!(grammar.validate().is_err(), "{templates}");
        }
    }

    #[test]
    fn names_a_locality_as_the_portal_knows_it() {
        let grammar: CitationGrammar = serde_json::from_str(
            r#"{
                "localities": {"Exemple": ["Exemple-sur-Mer", "Nord"]},
                "districts": [{"city": "Exampleville", "count": 20}]
            }"#,
        )
        .unwrap();
        assert!(grammar.validate().is_ok());
        assert_eq!(grammar.locality_name("exemple sur mer"), Some("Exemple"));
        assert_eq!(grammar.locality_name("Nord"), Some("Exemple"));
        assert_eq!(grammar.locality_name("Exemple"), None);
        for invalid in [
            r#"{"localities": {"Exemple": [" "]}}"#,
            r#"{"districts": [{"city": "Exampleville", "count": 0}]}"#,
            r#"{"districts": [{"city": " ", "count": 20}]}"#,
        ] {
            let grammar: CitationGrammar = serde_json::from_str(invalid).unwrap();
            assert!(grammar.validate().is_err(), "{invalid}");
        }
        assert!(
            serde_json::from_str::<CitationGrammar>(
                r#"{"districts": [{"city": "Exampleville", "count": 20, "other": 1}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn proposes_the_kind_of_record() {
        use DocumentCategory::{
            Census, CivilRecord, MilitaryArchive, NotarialArchive, ParishRecord,
        };
        for (title, category) in [
            (
                "AD44 - Exampleville - (aucun) - B - 1702",
                Some(ParishRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - BMS - 1702",
                Some(ParishRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - D - 1877",
                Some(CivilRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - NPMD - 1877",
                Some(CivilRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - M - 1702",
                Some(ParishRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - PM - 1877",
                Some(CivilRecord),
            ),
            ("AD44 - Exampleville - (aucun) - M - acte 3", None),
            (
                "AD44 - Exampleville - (aucun) - TB - 1750",
                Some(ParishRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - TD - 1803",
                Some(CivilRecord),
            ),
            (
                "AD44 - Exampleville - (aucun) - TA - 1803",
                Some(CivilRecord),
            ),
            ("AD99 - Exampleville - Recensement - 1872", Some(Census)),
            (
                "AD99 - Exampleville - Registres matricules - 1898",
                Some(MilitaryArchive),
            ),
            (
                "AD99 - Exampleville - Conscrits militaires - 1897",
                Some(MilitaryArchive),
            ),
            (
                "AD99 - Exampleville - Tables des successions et absences - 1897",
                Some(NotarialArchive),
            ),
            (
                "AD99 - Exampleville - Registres journaliers d'inhumation - 1904",
                Some(CivilRecord),
            ),
        ] {
            assert_eq!(parse(title).unwrap().category(), category, "{title}");
        }
    }

    #[test]
    fn rejects_titles_that_are_not_normalized() {
        for title in [
            "Parish register of Exampleville",
            "ad44 - Exampleville - (aucun) - N - 1877",
            "AD44 - (aucun) - N - 1877",
            "AD44 - Exampleville - (aucun) - X - 1877",
            // Without its parish, the act needs its period.
            "AD44 - Exampleville - N - vue 3",
            "AD44 - (aucun) - (aucun) - N - 1877",
        ] {
            assert_eq!(parse(title), None, "{title}");
        }
    }
}
