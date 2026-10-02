//! Dates written out in words, in every interface language and in Latin, and
//! written dates read back into numbers (`docs/ui-tools.md` §8).
//!
//! Interface-language patterns and vocabularies come from locale JSON.
//! Latin remains historical domain logic, independent of the UI language.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use oxidgene_core::calendar::days_in_month;
use oxidgene_core::enums::Calendar;
use oxidgene_core::search::fold_words;

use crate::i18n::Language;

/// The latest year written out: Roman numerals stop at 3999.
pub const MAX_YEAR: i32 = 3999;

/// A date to write, or read back: a year, and a month (1 to 12) and a day
/// when known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ymd {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

/// How a date is written: every part in words, or the day and year in
/// figures around the month's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Long,
    Short,
}

/// When the year begins: on 1 January, or on 25 March as in the
/// Annunciation style many registers kept, where a date from 1 January to
/// 24 March still bears the previous year's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YearStart {
    January,
    Annunciation,
}

impl Ymd {
    /// The year as a register kept in `start` numbered it.
    pub fn in_style(self, start: YearStart) -> Self {
        let early = match (self.month, self.day) {
            (Some(1 | 2), _) => true,
            (Some(3), Some(day)) => day < 25,
            _ => false,
        };
        if start == YearStart::Annunciation && early && self.year > 1 {
            Self {
                year: self.year - 1,
                ..self
            }
        } else {
            self
        }
    }

    fn writable(self) -> bool {
        (1..=MAX_YEAR).contains(&self.year)
            && self.month.is_none_or(|m| (1..=12).contains(&m))
            && self.day.is_none_or(|d| (1..=31).contains(&d))
            && (self.day.is_none() || self.month.is_some())
    }
}

// ── Month names ──────────────────────────────────────────────────────────

const MONTHS_LA: [&str; 12] = [
    "Januarius",
    "Februarius",
    "Martius",
    "Aprilis",
    "Maius",
    "Junius",
    "Julius",
    "Augustus",
    "September",
    "October",
    "November",
    "December",
];
/// The genitive: "mensis Februarii", "Nonas Februarii".
const MONTHS_LA_GENITIVE: [&str; 12] = [
    "Januarii",
    "Februarii",
    "Martii",
    "Aprilis",
    "Maii",
    "Junii",
    "Julii",
    "Augusti",
    "Septembris",
    "Octobris",
    "Novembris",
    "Decembris",
];
/// Read only: the classical forms agreeing with Kalendas, Nonas, Idus
/// (accusative) and Kalendis, Nonis, Idibus (ablative).
const MONTHS_LA_OTHER: [[&str; 2]; 12] = [
    ["januarias", "januariis"],
    ["februarias", "februariis"],
    ["martias", "martiis"],
    ["apriles", "aprilibus"],
    ["maias", "maiis"],
    ["junias", "juniis"],
    ["julias", "juliis"],
    ["augustas", "augustis"],
    ["septembres", "septembribus"],
    ["octobres", "octobribus"],
    ["novembres", "novembribus"],
    ["decembres", "decembribus"],
];
/// Read only: GEDCOM's month abbreviations.
const MONTHS_GEDCOM: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

const LA_DAYS: [&str; 20] = [
    "",
    "prima",
    "secunda",
    "tertia",
    "quarta",
    "quinta",
    "sexta",
    "septima",
    "octava",
    "nona",
    "decima",
    "undecima",
    "duodecima",
    "decima tertia",
    "decima quarta",
    "decima quinta",
    "decima sexta",
    "decima septima",
    "decima octava",
    "decima nona",
];
const LA_ORDINALS: [&str; 20] = [
    "",
    "primo",
    "secundo",
    "tertio",
    "quarto",
    "quinto",
    "sexto",
    "septimo",
    "octavo",
    "nono",
    "decimo",
    "undecimo",
    "duodecimo",
    "decimo tertio",
    "decimo quarto",
    "decimo quinto",
    "decimo sexto",
    "decimo septimo",
    "decimo octavo",
    "decimo nono",
];
const LA_TENS_ORDINALS: [&str; 10] = [
    "",
    "decimo",
    "vicesimo",
    "tricesimo",
    "quadragesimo",
    "quinquagesimo",
    "sexagesimo",
    "septuagesimo",
    "octogesimo",
    "nonagesimo",
];
const LA_HUNDREDS_ORDINALS: [&str; 10] = [
    "",
    "centesimo",
    "ducentesimo",
    "trecentesimo",
    "quadringentesimo",
    "quingentesimo",
    "sexcentesimo",
    "septingentesimo",
    "octingentesimo",
    "nongentesimo",
];
const LA_THOUSANDS_ORDINALS: [&str; 4] = ["", "millesimo", "bis millesimo", "ter millesimo"];

fn la_ordinal_below_100(n: u32, feminine: bool) -> String {
    let flip = |word: &str| {
        if feminine {
            word.split(' ')
                .map(|w| format!("{}a", &w[..w.len() - 1]))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            word.to_string()
        }
    };
    match n {
        0..20 => flip(LA_ORDINALS[n as usize]),
        _ if n.is_multiple_of(10) => flip(LA_TENS_ORDINALS[(n / 10) as usize]),
        _ => flip(&format!(
            "{} {}",
            LA_TENS_ORDINALS[(n / 10) as usize],
            LA_ORDINALS[(n % 10) as usize]
        )),
    }
}

/// A year as a masculine ordinal in the ablative, after "anno": every part
/// is an ordinal ("millesimo sexcentesimo quinquagesimo").
fn la_year(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut parts = Vec::new();
    if thousands > 0 {
        parts.push(LA_THOUSANDS_ORDINALS[thousands as usize].to_string());
    }
    if hundreds > 0 {
        parts.push(LA_HUNDREDS_ORDINALS[hundreds as usize].to_string());
    }
    if rest > 0 {
        parts.push(la_ordinal_below_100(rest, false));
    }
    parts.join(" ")
}

fn la_day(n: u32) -> String {
    if n < 20 {
        LA_DAYS[n as usize].to_string()
    } else {
        la_ordinal_below_100(n, true)
    }
}

/// A number in Roman numerals, from 1 to 3999.
pub fn roman(mut n: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for (value, numeral) in NUMERALS {
        while n >= value {
            out.push_str(numeral);
            n -= value;
        }
    }
    out
}

/// The value of Roman numerals written the canonical way, else `None`.
fn from_roman(text: &str) -> Option<u32> {
    let upper = text.to_ascii_uppercase();
    let digit = |c| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let values: Option<Vec<u32>> = upper.chars().map(digit).collect();
    let values = values?;
    let mut total = 0;
    for (i, v) in values.iter().enumerate() {
        match values.get(i + 1) {
            Some(next) if next > v => total -= *v as i64,
            _ => total += *v as i64,
        }
    }
    let total = u32::try_from(total)
        .ok()
        .filter(|n| (1..=MAX_YEAR as u32).contains(n))?;
    (roman(total) == upper).then_some(total)
}

/// The Nones of a month: the 7th in March, May, July and October, else the
/// 5th. The Ides fall eight days later.
fn nones(month: u8) -> u8 {
    if matches!(month, 3 | 5 | 7 | 10) {
        7
    } else {
        5
    }
}

/// A day in the Roman reckoning, counted inclusively to the next Kalends,
/// Nones or Ides: "ante diem IV Nonas Februarii" for 2 February. In a leap
/// February the 24th is the doubled sixth day before the Kalends of March.
pub fn latin_roman_reckoning(calendar: Calendar, date: Ymd) -> Option<String> {
    let (month, day) = (date.month?, date.day?);
    if !date.writable() {
        return None;
    }
    let genitive = |m: u8| MONTHS_LA_GENITIVE[usize::from(m - 1)];
    let before = |count: u32, what: &str, m: u8| {
        if count == 2 {
            format!("pridie {what} {}", genitive(m))
        } else {
            format!("ante diem {} {what} {}", roman(count), genitive(m))
        }
    };
    let (n, i) = (nones(month), nones(month) + 8);
    Some(match day {
        1 => format!("Kalendis {}", genitive(month)),
        d if d < n => before(u32::from(n - d + 1), "Nonas", month),
        d if d == n => format!("Nonis {}", genitive(month)),
        d if d < i => before(u32::from(i - d + 1), "Idus", month),
        d if d == i => format!("Idibus {}", genitive(month)),
        d => {
            let next = if month == 12 { 1 } else { month + 1 };
            let length = days_in_month(calendar, date.year, month);
            if month == 2 && length == 29 {
                match d {
                    24 => return Some(format!("ante diem bis VI Kalendas {}", genitive(3))),
                    25.. => before(u32::from(28 - (d - 1) + 2), "Kalendas", next),
                    _ => before(u32::from(28 - d + 2), "Kalendas", next),
                }
            } else {
                before(u32::from(length - d + 2), "Kalendas", next)
            }
        }
    })
}

/// A Latin date, long or short.
pub fn latin(date: Ymd, form: Form) -> Option<String> {
    if !date.writable() {
        return None;
    }
    let year = date.year as u32;
    Some(match form {
        Form::Long => {
            let mut parts = Vec::new();
            if let Some(day) = date.day {
                parts.push(format!("die {}", la_day(u32::from(day))));
            }
            if let Some(month) = date.month {
                parts.push(format!(
                    "mensis {}",
                    MONTHS_LA_GENITIVE[usize::from(month - 1)]
                ));
            }
            parts.push(format!("anno Domini {}", la_year(year)));
            parts.join(" ")
        }
        Form::Short => {
            let mut parts = Vec::new();
            if let Some(day) = date.day {
                parts.push(roman(u32::from(day)));
            }
            if let Some(month) = date.month {
                parts.push(MONTHS_LA[usize::from(month - 1)].to_string());
            }
            parts.push(roman(year));
            parts.join(" ")
        }
    })
}

/// A Latin date part by part: the words and the numerals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatinParts {
    /// The day: its figure, its ordinal, its numeral.
    pub day: Option<(u8, String, String)>,
    /// The month: its genitive, its numeral.
    pub month: Option<(String, String)>,
    /// The year: its ordinal, its numeral.
    pub year: (String, String),
}

pub fn latin_parts(date: Ymd) -> Option<LatinParts> {
    if !date.writable() {
        return None;
    }
    Some(LatinParts {
        day: date
            .day
            .map(|d| (d, la_day(u32::from(d)), roman(u32::from(d)))),
        month: date.month.map(|m| {
            (
                MONTHS_LA_GENITIVE[usize::from(m - 1)].to_string(),
                roman(u32::from(m)),
            )
        }),
        year: (la_year(date.year as u32), roman(date.year as u32)),
    })
}

// ── Writing ──────────────────────────────────────────────────────────────

// ── Reading ──────────────────────────────────────────────────────────────

pub fn written(language: Language, date: Ymd, form: Form) -> Option<String> {
    if !date.writable() {
        return None;
    }
    let locale = language.locale();
    let dates = &locale.dates;
    let index = usize::from(date.month.is_some()) + usize::from(date.day.is_some());
    let template = match form {
        Form::Long => &dates.long[index],
        Form::Short => &dates.short[index],
    };
    let year = match form {
        Form::Long => dates.years[date.year as usize].clone(),
        Form::Short => date.year.to_string(),
    };
    let day = date
        .day
        .map(|day| match form {
            Form::Long => dates.days[usize::from(day)].clone(),
            Form::Short => day.to_string(),
        })
        .unwrap_or_default();
    let month = date
        .month
        .map(|month| {
            let months = if date.day.is_some() {
                &dates.months_with_day
            } else {
                &dates.months
            };
            months[usize::from(month - 1)].as_str()
        })
        .unwrap_or_default();
    Some(
        template
            .replace("{year}", &year)
            .replace("{day}", &day)
            .replace("{month}", month),
    )
}

struct Lexicon {
    pieces: HashMap<String, Piece>,
    months: HashMap<String, u8>,
}

impl Lexicon {
    fn load() -> Self {
        let mut lexicon = Self {
            pieces: HashMap::new(),
            months: HashMap::new(),
        };
        for locale in crate::i18n::locale::available_locales() {
            for (word, piece) in &locale.dates.reading {
                use crate::i18n::locale::ReadingPiece;
                let piece = match piece {
                    ReadingPiece::Add { value } => Piece::Add(*value),
                    ReadingPiece::Hundred => Piece::Hundred,
                    ReadingPiece::Thousand => Piece::Thousand,
                    ReadingPiece::And => Piece::And,
                };
                lexicon.pieces.insert(fold_words(word), piece);
            }
            for months in [&locale.dates.months, &locale.dates.months_with_day] {
                for (index, name) in months.iter().enumerate() {
                    lexicon.months.insert(fold_words(name), index as u8 + 1);
                }
            }
        }
        for months in [&MONTHS_LA, &MONTHS_LA_GENITIVE, &MONTHS_GEDCOM] {
            for (index, name) in months.iter().enumerate() {
                lexicon.months.insert(fold_words(name), index as u8 + 1);
            }
        }
        for (index, forms) in MONTHS_LA_OTHER.iter().enumerate() {
            for form in forms {
                lexicon.months.insert(fold_words(form), index as u8 + 1);
            }
        }
        lexicon
    }
}

static LEXICON: LazyLock<RwLock<Arc<Lexicon>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Lexicon::load())));

pub(crate) fn refresh_lexicon() {
    *LEXICON
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Arc::new(Lexicon::load());
}

fn lexicon() -> Arc<Lexicon> {
    LEXICON
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Why a text could not be read as a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadError {
    /// Nothing to read.
    Empty,
    /// No year could be found.
    NoYear,
    /// The day does not exist in that month.
    NoSuchDay,
}

impl ReadError {
    /// The i18n key of the message.
    pub fn key(self) -> &'static str {
        match self {
            Self::Empty => "tools.words.error.empty",
            Self::NoYear => "tools.words.error.no_year",
            Self::NoSuchDay => "tools.words.error.no_such_day",
        }
    }
}

/// A piece of a written number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece {
    /// A value to add: a unit, a ten, "seiscientos", an ordinal.
    Add(u32),
    /// "hundred", "cent", "hundert": multiplies what precedes.
    Hundred,
    /// "thousand", "mille", "tausend": multiplies what precedes.
    Thousand,
    /// "and", "et", "und": joins the parts of one number.
    And,
}

/// fewest pieces wins, so "tredici" stays thirteen.
fn pieces(word: &str) -> Option<Vec<Piece>> {
    let lexicon = lexicon();
    let bytes = word.as_bytes();
    let mut best: Vec<Option<(usize, usize)>> = vec![None; bytes.len() + 1];
    let mut cost: Vec<usize> = vec![usize::MAX; bytes.len() + 1];
    cost[0] = 0;
    for end in 1..=bytes.len() {
        for start in 0..end {
            if cost[start] == usize::MAX {
                continue;
            }
            if lexicon.pieces.contains_key(&word[start..end]) && cost[start] + 1 < cost[end] {
                cost[end] = cost[start] + 1;
                best[end] = Some((start, end));
            }
        }
    }
    if cost[bytes.len()] == usize::MAX {
        return None;
    }
    let mut out = Vec::new();
    let mut at = bytes.len();
    while at > 0 {
        let (start, end) = best[at]?;
        out.push(lexicon.pieces[&word[start..end]]);
        at = start;
    }
    out.reverse();
    Some(out)
}

/// The value of number pieces read in order.
fn value(pieces: &[Piece]) -> u32 {
    let (mut total, mut current) = (0, 0);
    for piece in pieces {
        match piece {
            Piece::Add(v) => current += v,
            Piece::Hundred => current = current.max(1) * 100,
            Piece::Thousand => {
                total += current.max(1) * 1000;
                current = 0;
            }
            Piece::And => {}
        }
    }
    total + current
}

/// What a word of the text is.
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(u32),
    /// Number pieces, which join their neighbours into one number.
    Words(Vec<Piece>),
    Month(u8),
    Kalends,
    Nones,
    Ides,
    Pridie,
    Bis,
    Other,
}

fn classify(word: &str) -> Token {
    if let Some(digits) = word
        .find(|c: char| !c.is_ascii_digit())
        .map_or(Some(word), |at| (at > 0).then(|| &word[..at]))
        && let Ok(n) = digits.parse::<u32>()
    {
        return Token::Number(n);
    }
    match word {
        "kalendas" | "kalendis" | "kalendae" | "calendas" | "calendis" | "kal" => {
            return Token::Kalends;
        }
        "nonas" | "nonis" | "nonae" | "non" => return Token::Nones,
        "idus" | "idibus" | "id" => return Token::Ides,
        "pridie" => return Token::Pridie,
        "bis" => return Token::Bis,
        _ => {}
    }
    if let Some(month) = lexicon().months.get(word) {
        return Token::Month(*month);
    }
    if let Some(pieces) = pieces(word) {
        return Token::Words(pieces);
    }
    Token::Other
}

/// Reads a written date: in any of the eight languages or in Latin, with
/// numbers in words, in figures or in Roman numerals, the day before or
/// after the month, or in the Roman reckoning of Kalends, Nones and Ides;
/// also figures alone, day first ("2/2/1650") or year first ("1650-02-02").
pub fn read(text: &str) -> Result<Ymd, ReadError> {
    if text.trim().is_empty() {
        return Err(ReadError::Empty);
    }
    let (numbers, marks) = numbers_and_marks(merge_compounds(tokenize(text)));

    let month_at = marks.iter().find_map(|(i, t)| match t {
        Token::Month(m) => Some((*i, *m)),
        _ => None,
    });
    let anchor = marks.iter().find_map(|(i, t)| match t {
        Token::Kalends | Token::Nones | Token::Ides => Some((*i, t.clone())),
        _ => None,
    });

    let date = match (anchor, month_at) {
        (Some(anchor), Some(month_at)) => roman_reckoning_date(anchor, month_at, &numbers, &marks)?,
        (_, Some(month_at)) => month_name_date(month_at, &numbers)?,
        (_, None) => figures_date(&numbers)?,
    };
    checked(date)
}

/// The words of `text`, classified.
fn tokenize(text: &str) -> Vec<Token> {
    // Roman numerals are only read where written in capitals, or where the
    // Roman reckoning expects one, so that "di" or "mil" stay words.
    let reckoning = fold_words(text)
        .split_whitespace()
        .any(|w| matches!(classify(w), Token::Kalends | Token::Nones | Token::Ides));
    let mut tokens: Vec<Token> = Vec::new();
    for (original, word) in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .flat_map(|w| {
            let folded = fold_words(w);
            let parts: Vec<String> = folded.split_whitespace().map(str::to_string).collect();
            parts.into_iter().map(move |p| (w, p))
        })
    {
        let token = classify(&word);
        let numeral = (original.chars().all(|c| c.is_ascii_uppercase()) || reckoning)
            .then(|| from_roman(&word))
            .flatten();
        // A number word stays a word: "DIX" in capitals is ten, not 509.
        tokens.push(match (token, numeral) {
            (Token::Other, Some(n)) => Token::Number(n),
            (token, _) => token,
        });
    }
    tokens
}

/// Joins the words that make one number between them: "quatre-vingt" is one
/// number, "bis millesimo" two thousand.
fn merge_compounds(tokens: Vec<Token>) -> Vec<Token> {
    let mut merged: Vec<Token> = Vec::new();
    for token in tokens {
        match (merged.last_mut(), token) {
            (Some(last @ Token::Bis), Token::Words(more))
                if more.first() == Some(&Piece::Thousand) =>
            {
                let mut pieces = vec![Piece::Add(2)];
                pieces.extend(more);
                *last = Token::Words(pieces);
            }
            (Some(Token::Words(pieces)), Token::Words(more))
                if pieces.last() == Some(&Piece::Add(4))
                    && more.first() == Some(&Piece::Add(20)) =>
            {
                pieces.pop();
                pieces.push(Piece::Add(80));
                pieces.extend(more.into_iter().skip(1));
            }
            (_, token) => merged.push(token),
        }
    }
    merged
}

/// Values found in a text, each with the position of the token it starts at.
type Positioned<T> = Vec<(usize, T)>;

/// The numbers of a text and its other meaningful tokens, each with the
/// position it holds. Words next to words make one number, joined by "and".
fn numbers_and_marks(merged: Vec<Token>) -> (Positioned<u32>, Positioned<Token>) {
    let mut numbers: Vec<(usize, u32)> = Vec::new();
    let mut run: Vec<Piece> = Vec::new();
    let mut run_start = 0;
    let mut marks: Vec<(usize, Token)> = Vec::new();
    let flush = |run: &mut Vec<Piece>, start: usize, numbers: &mut Vec<(usize, u32)>| {
        if run.iter().any(|p| *p != Piece::And) {
            numbers.push((start, value(run)));
        }
        run.clear();
    };
    for (i, token) in merged.into_iter().enumerate() {
        match token {
            Token::Words(pieces) => {
                if run.is_empty() {
                    run_start = i;
                }
                run.extend(pieces);
            }
            Token::Number(n) => {
                flush(&mut run, run_start, &mut numbers);
                numbers.push((i, n));
            }
            other => {
                flush(&mut run, run_start, &mut numbers);
                if other != Token::Other {
                    marks.push((i, other));
                }
            }
        }
    }
    flush(&mut run, run_start, &mut numbers);
    (numbers, marks)
}

/// A date in the Roman reckoning: a count before the Kalends, Nones or Ides
/// found at `at`, the year after the month.
fn roman_reckoning_date(
    (at, kind): (usize, Token),
    (month_pos, month): (usize, u8),
    numbers: &[(usize, u32)],
    marks: &[(usize, Token)],
) -> Result<Ymd, ReadError> {
    let pridie = marks.iter().any(|(i, t)| *t == Token::Pridie && *i < at);
    let bis = marks.iter().any(|(i, t)| *t == Token::Bis && *i < at);
    let count = if pridie {
        2
    } else {
        numbers
            .iter()
            .filter(|(i, _)| *i < at)
            .map(|(_, n)| *n)
            .next_back()
            .unwrap_or(1)
    };
    let year = numbers
        .iter()
        .find(|(i, _)| *i > month_pos)
        .map(|(_, n)| *n)
        .ok_or(ReadError::NoYear)? as i32;
    let (month, day) = match kind {
        Token::Nones => (month, i64::from(nones(month)) - i64::from(count) + 1),
        Token::Ides => (month, i64::from(nones(month) + 8) - i64::from(count) + 1),
        _ if count == 1 => (month, 1),
        _ => before_kalends(year, month, count, bis),
    };
    Ok(Ymd {
        year,
        month: Some(month),
        day: Some(
            u8::try_from(day)
                .ok()
                .filter(|d| *d > 0)
                .ok_or(ReadError::NoSuchDay)?,
        ),
    })
}

/// The month and day `count` days before the Kalends of `month`, counted
/// inclusively. `bis` names the doubled sixth day of a leap February.
fn before_kalends(year: i32, month: u8, count: u32, bis: bool) -> (u8, i64) {
    let previous = if month == 1 { 12 } else { month - 1 };
    let length = i64::from(days_in_month(Calendar::Gregorian, year, previous));
    let count = i64::from(count);
    let day = if previous == 2 && length == 29 {
        if bis {
            24
        } else if count <= 6 {
            31 - count
        } else {
            30 - count
        }
    } else {
        length - count + 2
    };
    (previous, day)
}

/// A date around a month's name: the day and year in figures or words on
/// either side of it.
fn month_name_date(
    (month_pos, month): (usize, u8),
    numbers: &[(usize, u32)],
) -> Result<Ymd, ReadError> {
    let before: Vec<u32> = numbers
        .iter()
        .filter(|(i, _)| *i < month_pos)
        .map(|(_, n)| *n)
        .collect();
    let after: Vec<u32> = numbers
        .iter()
        .filter(|(i, _)| *i > month_pos)
        .map(|(_, n)| *n)
        .collect();
    let (day, year) = match (before.last(), after.as_slice()) {
        (Some(&d), [y, ..]) if (1..=31).contains(&d) => (Some(d), Some(*y)),
        (_, [d, y, ..]) if (1..=31).contains(d) => (Some(*d), Some(*y)),
        (None, [y]) => (None, Some(*y)),
        (Some(&y), []) if y > 31 => (None, Some(y)),
        (Some(&y), [d]) if y > 31 && (1..=31).contains(d) => (Some(*d), Some(y)),
        (_, [y, ..]) => (None, Some(*y)),
        _ => (None, None),
    };
    Ok(Ymd {
        year: year.ok_or(ReadError::NoYear)? as i32,
        month: Some(month),
        day: day.map(|d| d as u8),
    })
}

/// A date in figures alone: a year, day first ("2/2/1650"), year first
/// ("1650-02-02"), or a month and a year.
fn figures_date(numbers: &[(usize, u32)]) -> Result<Ymd, ReadError> {
    let values: Vec<u32> = numbers.iter().map(|(_, n)| *n).collect();
    Ok(match values.as_slice() {
        [y] => Ymd {
            year: *y as i32,
            month: None,
            day: None,
        },
        [y, m, d] if *y > 31 => Ymd {
            year: *y as i32,
            month: u8::try_from(*m).ok(),
            day: u8::try_from(*d).ok(),
        },
        [d, m, y] => Ymd {
            year: *y as i32,
            month: u8::try_from(*m).ok(),
            day: u8::try_from(*d).ok(),
        },
        [m, y] if *m <= 12 => Ymd {
            year: *y as i32,
            month: u8::try_from(*m).ok(),
            day: None,
        },
        _ => return Err(ReadError::NoYear),
    })
}

/// `date`, once its year, month and day are known to exist.
fn checked(date: Ymd) -> Result<Ymd, ReadError> {
    if !(1..=MAX_YEAR).contains(&date.year) {
        return Err(ReadError::NoYear);
    }
    match (date.month, date.day) {
        (None, Some(_)) => Err(ReadError::NoSuchDay),
        (Some(m), _) if !(1..=12).contains(&m) => Err(ReadError::NoSuchDay),
        (Some(m), Some(d)) if d == 0 || d > days_in_month(Calendar::Gregorian, date.year, m) => {
            Err(ReadError::NoSuchDay)
        }
        _ => Ok(date),
    }
}

#[cfg(test)]
mod tests;
