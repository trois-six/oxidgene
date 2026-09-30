//! Dates written out in words, in every interface language and in Latin, and
//! written dates read back into numbers (`docs/ui-tools.md` §8).
//!
//! Every word table lives here, used both ways: the writer spells numbers
//! and months from them, and the reader recognises what they spell. A
//! sentence this module writes is its own output, not an interface string,
//! so it is not in the i18n tables; Latin is Latin whatever the interface
//! language.

use std::collections::HashMap;
use std::sync::LazyLock;

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

const MONTHS_EN: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const MONTHS_FR: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const MONTHS_DE: [&str; 12] = [
    "Januar",
    "Februar",
    "März",
    "April",
    "Mai",
    "Juni",
    "Juli",
    "August",
    "September",
    "Oktober",
    "November",
    "Dezember",
];
const MONTHS_ES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];
const MONTHS_IT: [&str; 12] = [
    "gennaio",
    "febbraio",
    "marzo",
    "aprile",
    "maggio",
    "giugno",
    "luglio",
    "agosto",
    "settembre",
    "ottobre",
    "novembre",
    "dicembre",
];
const MONTHS_NL: [&str; 12] = [
    "januari",
    "februari",
    "maart",
    "april",
    "mei",
    "juni",
    "juli",
    "augustus",
    "september",
    "oktober",
    "november",
    "december",
];
/// Polish months in the nominative, for a month named alone…
const MONTHS_PL: [&str; 12] = [
    "styczeń",
    "luty",
    "marzec",
    "kwiecień",
    "maj",
    "czerwiec",
    "lipiec",
    "sierpień",
    "wrzesień",
    "październik",
    "listopad",
    "grudzień",
];
/// …and in the genitive a day takes: "drugiego lutego".
const MONTHS_PL_GENITIVE: [&str; 12] = [
    "stycznia",
    "lutego",
    "marca",
    "kwietnia",
    "maja",
    "czerwca",
    "lipca",
    "sierpnia",
    "września",
    "października",
    "listopada",
    "grudnia",
];
const MONTHS_PT: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];
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

fn month_name(language: Language, month: u8) -> &'static str {
    let i = usize::from(month - 1);
    match language {
        Language::En => MONTHS_EN[i],
        Language::Fr => MONTHS_FR[i],
        Language::De => MONTHS_DE[i],
        Language::Es => MONTHS_ES[i],
        Language::It => MONTHS_IT[i],
        Language::Nl => MONTHS_NL[i],
        Language::Pl => MONTHS_PL[i],
        Language::Pt => MONTHS_PT[i],
    }
}

// ── English ──────────────────────────────────────────────────────────────

const EN_UNITS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const EN_TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const EN_ORDINALS: [&str; 20] = [
    "",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];
const EN_TENS_ORDINALS: [&str; 10] = [
    "",
    "",
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "sixtieth",
    "seventieth",
    "eightieth",
    "ninetieth",
];

fn en_below_100(n: u32) -> String {
    match n {
        0..20 => EN_UNITS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => EN_TENS[(n / 10) as usize].to_string(),
        _ => format!(
            "{}-{}",
            EN_TENS[(n / 10) as usize],
            EN_UNITS[(n % 10) as usize]
        ),
    }
}

fn en_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut parts = Vec::new();
    if thousands > 0 {
        parts.push(format!("{} thousand", en_below_100(thousands)));
    }
    if hundreds > 0 {
        parts.push(format!("{} hundred", EN_UNITS[hundreds as usize]));
    }
    if rest > 0 {
        if !parts.is_empty() {
            parts.push("and".to_string());
        }
        parts.push(en_below_100(rest));
    }
    if parts.is_empty() {
        return EN_UNITS[0].to_string();
    }
    parts.join(" ")
}

fn en_ordinal(n: u32) -> String {
    match n {
        0..20 => EN_ORDINALS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => EN_TENS_ORDINALS[(n / 10) as usize].to_string(),
        _ => format!(
            "{}-{}",
            EN_TENS[(n / 10) as usize],
            EN_ORDINALS[(n % 10) as usize]
        ),
    }
}

// ── French ───────────────────────────────────────────────────────────────

const FR_UNITS: [&str; 17] = [
    "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze",
    "douze", "treize", "quatorze", "quinze", "seize",
];
const FR_TENS: [&str; 7] = [
    "",
    "",
    "vingt",
    "trente",
    "quarante",
    "cinquante",
    "soixante",
];

fn fr_below_100(n: u32) -> String {
    match n {
        0..17 => FR_UNITS[n as usize].to_string(),
        17..20 => format!("dix-{}", FR_UNITS[(n - 10) as usize]),
        20..70 => {
            let tens = FR_TENS[(n / 10) as usize];
            match n % 10 {
                0 => tens.to_string(),
                1 => format!("{tens} et un"),
                u => format!("{tens}-{}", FR_UNITS[u as usize]),
            }
        }
        70..80 if n == 71 => "soixante et onze".to_string(),
        70..80 => format!("soixante-{}", fr_below_100(n - 60)),
        80 => "quatre-vingts".to_string(),
        _ => format!("quatre-vingt-{}", fr_below_100(n - 80)),
    }
}

fn fr_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut parts = Vec::new();
    match thousands {
        0 => {}
        1 => parts.push("mille".to_string()),
        t => parts.push(format!("{} mille", fr_below_100(t))),
    }
    match (hundreds, rest) {
        (0, _) => {}
        (1, _) => parts.push("cent".to_string()),
        (h, 0) => parts.push(format!("{} cents", FR_UNITS[h as usize])),
        (h, _) => parts.push(format!("{} cent", FR_UNITS[h as usize])),
    }
    if rest > 0 || parts.is_empty() {
        parts.push(fr_below_100(rest));
    }
    parts.join(" ")
}

// ── German ───────────────────────────────────────────────────────────────

const DE_UNITS: [&str; 20] = [
    "null",
    "eins",
    "zwei",
    "drei",
    "vier",
    "fünf",
    "sechs",
    "sieben",
    "acht",
    "neun",
    "zehn",
    "elf",
    "zwölf",
    "dreizehn",
    "vierzehn",
    "fünfzehn",
    "sechzehn",
    "siebzehn",
    "achtzehn",
    "neunzehn",
];
const DE_TENS: [&str; 10] = [
    "", "", "zwanzig", "dreißig", "vierzig", "fünfzig", "sechzig", "siebzig", "achtzig", "neunzig",
];

/// Below 100; `alone` says whether the number ends there ("eins") or is
/// part of a larger one ("einhundert", "einundzwanzig").
fn de_below_100(n: u32, alone: bool) -> String {
    match n {
        1 if !alone => "ein".to_string(),
        0..20 => DE_UNITS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => DE_TENS[(n / 10) as usize].to_string(),
        _ => format!(
            "{}und{}",
            de_below_100(n % 10, false),
            DE_TENS[(n / 10) as usize]
        ),
    }
}

fn de_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut out = String::new();
    if thousands > 0 {
        out.push_str(&de_below_100(thousands, false));
        out.push_str("tausend");
    }
    if hundreds > 0 {
        out.push_str(&de_below_100(hundreds, false));
        out.push_str("hundert");
    }
    if rest > 0 || out.is_empty() {
        out.push_str(&de_below_100(rest, true));
    }
    out
}

/// A year as it is said: from 1100 to 1999 in hundreds
/// ("sechzehnhundertfünfzig").
fn de_year(n: u32) -> String {
    if (1100..2000).contains(&n) {
        let rest = n % 100;
        let mut out = format!("{}hundert", de_below_100(n / 100, false));
        if rest > 0 {
            out.push_str(&de_below_100(rest, true));
        }
        out
    } else {
        de_cardinal(n)
    }
}

/// A day as an ordinal after "am": "ersten", "dritten", "zwanzigsten".
fn de_ordinal(n: u32) -> String {
    match n {
        1 => "ersten".to_string(),
        3 => "dritten".to_string(),
        7 => "siebten".to_string(),
        8 => "achten".to_string(),
        0..20 => format!("{}ten", DE_UNITS[n as usize]),
        _ => format!("{}sten", de_below_100(n, true)),
    }
}

// ── Spanish ──────────────────────────────────────────────────────────────

const ES_UNITS: [&str; 30] = [
    "cero",
    "uno",
    "dos",
    "tres",
    "cuatro",
    "cinco",
    "seis",
    "siete",
    "ocho",
    "nueve",
    "diez",
    "once",
    "doce",
    "trece",
    "catorce",
    "quince",
    "dieciséis",
    "diecisiete",
    "dieciocho",
    "diecinueve",
    "veinte",
    "veintiuno",
    "veintidós",
    "veintitrés",
    "veinticuatro",
    "veinticinco",
    "veintiséis",
    "veintisiete",
    "veintiocho",
    "veintinueve",
];
const ES_TENS: [&str; 10] = [
    "",
    "",
    "veinte",
    "treinta",
    "cuarenta",
    "cincuenta",
    "sesenta",
    "setenta",
    "ochenta",
    "noventa",
];
const ES_HUNDREDS: [&str; 10] = [
    "",
    "ciento",
    "doscientos",
    "trescientos",
    "cuatrocientos",
    "quinientos",
    "seiscientos",
    "setecientos",
    "ochocientos",
    "novecientos",
];

fn es_below_100(n: u32) -> String {
    match n {
        0..30 => ES_UNITS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => ES_TENS[(n / 10) as usize].to_string(),
        _ => format!(
            "{} y {}",
            ES_TENS[(n / 10) as usize],
            ES_UNITS[(n % 10) as usize]
        ),
    }
}

fn es_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut parts = Vec::new();
    match thousands {
        0 => {}
        1 => parts.push("mil".to_string()),
        t => parts.push(format!("{} mil", es_below_100(t))),
    }
    match (hundreds, rest) {
        (0, _) => {}
        (1, 0) => parts.push("cien".to_string()),
        (h, _) => parts.push(ES_HUNDREDS[h as usize].to_string()),
    }
    if rest > 0 || parts.is_empty() {
        parts.push(es_below_100(rest));
    }
    parts.join(" ")
}

// ── Italian ──────────────────────────────────────────────────────────────

const IT_UNITS: [&str; 20] = [
    "zero",
    "uno",
    "due",
    "tre",
    "quattro",
    "cinque",
    "sei",
    "sette",
    "otto",
    "nove",
    "dieci",
    "undici",
    "dodici",
    "tredici",
    "quattordici",
    "quindici",
    "sedici",
    "diciassette",
    "diciotto",
    "diciannove",
];
const IT_TENS: [&str; 10] = [
    "",
    "",
    "venti",
    "trenta",
    "quaranta",
    "cinquanta",
    "sessanta",
    "settanta",
    "ottanta",
    "novanta",
];

fn it_below_100(n: u32) -> String {
    if n < 20 {
        return IT_UNITS[n as usize].to_string();
    }
    let tens = IT_TENS[(n / 10) as usize];
    match n % 10 {
        0 => tens.to_string(),
        // "ventuno", "ventotto": the tens lose their vowel before one
        // starting with a vowel.
        u @ (1 | 8) => format!("{}{}", &tens[..tens.len() - 1], IT_UNITS[u as usize]),
        u => format!("{tens}{}", IT_UNITS[u as usize]),
    }
}

fn it_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut out = String::new();
    match thousands {
        0 => {}
        1 => out.push_str("mille"),
        t => {
            out.push_str(&it_below_100(t));
            out.push_str("mila");
        }
    }
    if hundreds > 0 {
        if hundreds > 1 {
            out.push_str(IT_UNITS[hundreds as usize]);
        }
        // "centottanta", "centotto".
        if rest == 8 || (80..90).contains(&rest) {
            out.push_str("cent");
        } else {
            out.push_str("cento");
        }
    }
    if rest > 0 || out.is_empty() {
        out.push_str(&it_below_100(rest));
    }
    // A compound ending in three takes the accent: "ventitré", "milletré".
    if n > 10 && n % 10 == 3 && n % 100 != 13 && out.ends_with("tre") {
        out.truncate(out.len() - 3);
        out.push_str("tré");
    }
    out
}

// ── Dutch ────────────────────────────────────────────────────────────────

const NL_UNITS: [&str; 20] = [
    "nul",
    "een",
    "twee",
    "drie",
    "vier",
    "vijf",
    "zes",
    "zeven",
    "acht",
    "negen",
    "tien",
    "elf",
    "twaalf",
    "dertien",
    "veertien",
    "vijftien",
    "zestien",
    "zeventien",
    "achttien",
    "negentien",
];
const NL_TENS: [&str; 10] = [
    "", "", "twintig", "dertig", "veertig", "vijftig", "zestig", "zeventig", "tachtig", "negentig",
];

fn nl_below_100(n: u32) -> String {
    match n {
        0..20 => NL_UNITS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => NL_TENS[(n / 10) as usize].to_string(),
        _ => {
            let unit = NL_UNITS[(n % 10) as usize];
            // "tweeëntwintig": a diaeresis parts two vowels.
            let and = if unit.ends_with('e') { "ën" } else { "en" };
            format!("{unit}{and}{}", NL_TENS[(n / 10) as usize])
        }
    }
}

fn nl_cardinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut out = String::new();
    if thousands > 1 {
        out.push_str(&nl_below_100(thousands));
    }
    if thousands > 0 {
        out.push_str("duizend");
    }
    if hundreds > 1 {
        out.push_str(NL_UNITS[hundreds as usize]);
    }
    if hundreds > 0 {
        out.push_str("honderd");
    }
    if rest > 0 || out.is_empty() {
        out.push_str(&nl_below_100(rest));
    }
    out
}

/// A year as it is said: from 1100 to 1999 in hundreds
/// ("zestienhonderdvijftig").
fn nl_year(n: u32) -> String {
    if (1100..2000).contains(&n) {
        let mut out = format!("{}honderd", nl_below_100(n / 100));
        if !n.is_multiple_of(100) {
            out.push_str(&nl_below_100(n % 100));
        }
        out
    } else {
        nl_cardinal(n)
    }
}

fn nl_ordinal(n: u32) -> String {
    match n {
        1 => "eerste".to_string(),
        3 => "derde".to_string(),
        8 => "achtste".to_string(),
        0..20 => format!("{}de", NL_UNITS[n as usize]),
        _ => format!("{}ste", nl_below_100(n)),
    }
}

// ── Polish ───────────────────────────────────────────────────────────────

const PL_UNITS: [&str; 20] = [
    "zero",
    "jeden",
    "dwa",
    "trzy",
    "cztery",
    "pięć",
    "sześć",
    "siedem",
    "osiem",
    "dziewięć",
    "dziesięć",
    "jedenaście",
    "dwanaście",
    "trzynaście",
    "czternaście",
    "piętnaście",
    "szesnaście",
    "siedemnaście",
    "osiemnaście",
    "dziewiętnaście",
];
const PL_TENS: [&str; 10] = [
    "",
    "",
    "dwadzieścia",
    "trzydzieści",
    "czterdzieści",
    "pięćdziesiąt",
    "sześćdziesiąt",
    "siedemdziesiąt",
    "osiemdziesiąt",
    "dziewięćdziesiąt",
];
const PL_HUNDREDS: [&str; 10] = [
    "",
    "sto",
    "dwieście",
    "trzysta",
    "czterysta",
    "pięćset",
    "sześćset",
    "siedemset",
    "osiemset",
    "dziewięćset",
];
/// Ordinals in the masculine genitive a date takes: "drugiego (dnia)
/// lutego tysiąc sześćset pięćdziesiątego (roku)".
const PL_ORDINALS: [&str; 20] = [
    "",
    "pierwszego",
    "drugiego",
    "trzeciego",
    "czwartego",
    "piątego",
    "szóstego",
    "siódmego",
    "ósmego",
    "dziewiątego",
    "dziesiątego",
    "jedenastego",
    "dwunastego",
    "trzynastego",
    "czternastego",
    "piętnastego",
    "szesnastego",
    "siedemnastego",
    "osiemnastego",
    "dziewiętnastego",
];
const PL_TENS_ORDINALS: [&str; 10] = [
    "",
    "",
    "dwudziestego",
    "trzydziestego",
    "czterdziestego",
    "pięćdziesiątego",
    "sześćdziesiątego",
    "siedemdziesiątego",
    "osiemdziesiątego",
    "dziewięćdziesiątego",
];
const PL_HUNDREDS_ORDINALS: [&str; 10] = [
    "",
    "setnego",
    "dwusetnego",
    "trzechsetnego",
    "czterechsetnego",
    "pięćsetnego",
    "sześćsetnego",
    "siedemsetnego",
    "osiemsetnego",
    "dziewięćsetnego",
];
const PL_THOUSANDS_ORDINALS: [&str; 4] = ["", "tysięcznego", "dwutysięcznego", "trzytysięcznego"];

fn pl_thousands(t: u32) -> String {
    match t {
        1 => "tysiąc".to_string(),
        2..=4 => format!("{} tysiące", PL_UNITS[t as usize]),
        _ => format!("{} tysięcy", PL_UNITS[t as usize]),
    }
}

fn pl_ordinal_below_100(n: u32) -> String {
    match n {
        0..20 => PL_ORDINALS[n as usize].to_string(),
        _ if n.is_multiple_of(10) => PL_TENS_ORDINALS[(n / 10) as usize].to_string(),
        _ => format!(
            "{} {}",
            PL_TENS_ORDINALS[(n / 10) as usize],
            PL_ORDINALS[(n % 10) as usize]
        ),
    }
}

/// An ordinal in the genitive: only its last parts become ordinals, the
/// thousands and hundreds before them stay cardinal.
fn pl_ordinal(n: u32) -> String {
    let (thousands, hundreds, rest) = (n / 1000, n / 100 % 10, n % 100);
    let mut parts = Vec::new();
    if rest > 0 {
        if thousands > 0 {
            parts.push(pl_thousands(thousands));
        }
        if hundreds > 0 {
            parts.push(PL_HUNDREDS[hundreds as usize].to_string());
        }
        parts.push(pl_ordinal_below_100(rest));
    } else if hundreds > 0 {
        if thousands > 0 {
            parts.push(pl_thousands(thousands));
        }
        parts.push(PL_HUNDREDS_ORDINALS[hundreds as usize].to_string());
    } else {
        parts.push(PL_THOUSANDS_ORDINALS[thousands as usize].to_string());
    }
    parts.join(" ")
}

// ── Portuguese ───────────────────────────────────────────────────────────

const PT_UNITS: [&str; 20] = [
    "zero",
    "um",
    "dois",
    "três",
    "quatro",
    "cinco",
    "seis",
    "sete",
    "oito",
    "nove",
    "dez",
    "onze",
    "doze",
    "treze",
    "catorze",
    "quinze",
    "dezasseis",
    "dezassete",
    "dezoito",
    "dezanove",
];
const PT_TENS: [&str; 10] = [
    "",
    "",
    "vinte",
    "trinta",
    "quarenta",
    "cinquenta",
    "sessenta",
    "setenta",
    "oitenta",
    "noventa",
];
const PT_HUNDREDS: [&str; 10] = [
    "",
    "cento",
    "duzentos",
    "trezentos",
    "quatrocentos",
    "quinhentos",
    "seiscentos",
    "setecentos",
    "oitocentos",
    "novecentos",
];

fn pt_below_1000(n: u32) -> String {
    let (hundreds, rest) = (n / 100, n % 100);
    let below_100 = match rest {
        0..20 => PT_UNITS[rest as usize].to_string(),
        _ if rest.is_multiple_of(10) => PT_TENS[(rest / 10) as usize].to_string(),
        _ => format!(
            "{} e {}",
            PT_TENS[(rest / 10) as usize],
            PT_UNITS[(rest % 10) as usize]
        ),
    };
    match (hundreds, rest) {
        (0, _) => below_100,
        (1, 0) => "cem".to_string(),
        (h, 0) => PT_HUNDREDS[h as usize].to_string(),
        (h, _) => format!("{} e {below_100}", PT_HUNDREDS[h as usize]),
    }
}

fn pt_cardinal(n: u32) -> String {
    let (thousands, rest) = (n / 1000, n % 1000);
    let head = match thousands {
        0 => return pt_below_1000(rest),
        1 => "mil".to_string(),
        t => format!("{} mil", pt_below_1000(t)),
    };
    match rest {
        0 => head,
        // "mil e quinhentos", "mil e cinquenta", but "mil seiscentos e
        // cinquenta".
        r if r < 100 || r.is_multiple_of(100) => format!("{head} e {}", pt_below_1000(r)),
        r => format!("{head} {}", pt_below_1000(r)),
    }
}

// ── Latin ────────────────────────────────────────────────────────────────

/// Days as feminine ordinals in the ablative: "die secunda".
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

/// `date` written in `language`, long or short.
pub fn written(language: Language, date: Ymd, form: Form) -> Option<String> {
    if !date.writable() {
        return None;
    }
    let year = date.year as u32;
    let figures = || date.year.to_string();
    let text = match (form, date.month, date.day) {
        (Form::Short, month, day) => {
            let (month, day) = (month.map(|m| month_name(language, m)), day);
            match language {
                Language::Es | Language::Pt => [
                    day.map(|d| format!("{d} de")),
                    month.map(|m| format!("{m} de")),
                    Some(figures()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
                Language::De => [
                    day.map(|d| format!("{d}.")),
                    month.map(str::to_string),
                    Some(figures()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
                Language::Pl => [
                    day.map(|d| d.to_string()),
                    date.month.map(|m| {
                        if day.is_some() {
                            MONTHS_PL_GENITIVE[usize::from(m - 1)].to_string()
                        } else {
                            MONTHS_PL[usize::from(m - 1)].to_string()
                        }
                    }),
                    Some(figures()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
                _ => [
                    day.map(|d| d.to_string()),
                    month.map(str::to_string),
                    Some(figures()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" "),
            }
        }
        (Form::Long, month, day) => long(language, year, month, day.map(u32::from)),
    };
    Some(text)
}

fn long(language: Language, year: u32, month: Option<u8>, day: Option<u32>) -> String {
    let m = month.map(|m| month_name(language, m));
    match language {
        Language::En => {
            let y = en_cardinal(year);
            match (m, day) {
                (Some(m), Some(d)) => format!("the {} of {m}, {y}", en_ordinal(d)),
                (Some(m), None) => format!("{m} {y}"),
                _ => y,
            }
        }
        Language::Fr => {
            let y = fr_cardinal(year);
            match (m, day) {
                (Some(m), Some(1)) => format!("le premier {m} {y}"),
                (Some(m), Some(d)) => format!("le {} {m} {y}", fr_cardinal(d)),
                (Some(m), None) => format!("{m} {y}"),
                _ => y,
            }
        }
        Language::De => {
            let y = de_year(year);
            match (m, day) {
                (Some(m), Some(d)) => format!("am {} {m} {y}", de_ordinal(d)),
                (Some(m), None) => format!("im {m} {y}"),
                _ => format!("im Jahr {y}"),
            }
        }
        Language::Es => {
            let y = es_cardinal(year);
            match (m, day) {
                (Some(m), Some(1)) => format!("primero de {m} de {y}"),
                (Some(m), Some(d)) => format!("{} de {m} de {y}", es_cardinal(d)),
                (Some(m), None) => format!("{m} de {y}"),
                _ => y,
            }
        }
        Language::It => {
            let y = it_cardinal(year);
            match (m, day) {
                (Some(m), Some(1)) => format!("primo {m} {y}"),
                (Some(m), Some(d)) => format!("{} {m} {y}", it_cardinal(d)),
                (Some(m), None) => format!("{m} {y}"),
                _ => y,
            }
        }
        Language::Nl => {
            let y = nl_year(year);
            match (m, day) {
                (Some(m), Some(d)) => format!("de {} {m} {y}", nl_ordinal(d)),
                (Some(m), None) => format!("{m} {y}"),
                _ => y,
            }
        }
        Language::Pl => {
            let y = pl_ordinal(year);
            match (month, day) {
                (Some(m), Some(d)) => format!(
                    "{} {} {y} roku",
                    pl_ordinal(d),
                    MONTHS_PL_GENITIVE[usize::from(m - 1)]
                ),
                (Some(m), None) => format!("{} {y} roku", MONTHS_PL[usize::from(m - 1)]),
                _ => format!("{y} roku"),
            }
        }
        Language::Pt => {
            let y = pt_cardinal(year);
            match (m, day) {
                (Some(m), Some(1)) => format!("primeiro de {m} de {y}"),
                (Some(m), Some(d)) => format!("{} de {m} de {y}", pt_cardinal(d)),
                (Some(m), None) => format!("{m} de {y}"),
                _ => y,
            }
        }
    }
}

// ── Reading ──────────────────────────────────────────────────────────────

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

/// Every word the writers use for a number, and the variants they do not
/// write but a text may hold, folded.
static PIECES: LazyLock<HashMap<String, Piece>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    add_tabled_numbers(&mut map);
    add_derived_numbers(&mut map);
    add_irregular_words(&mut map);
    map
});

/// Add `word`, folded, as `piece`.
fn add_piece(map: &mut HashMap<String, Piece>, word: &str, piece: Piece) {
    map.insert(fold_words(word), piece);
}

/// The numbers the writers' tables spell out: units, ordinals, tens and
/// hundreds.
fn add_tabled_numbers(map: &mut HashMap<String, Piece>) {
    let mut add = |word: &str, piece: Piece| add_piece(map, word, piece);
    let tables: [&[&str]; 12] = [
        &EN_UNITS,
        &FR_UNITS,
        &DE_UNITS,
        &ES_UNITS,
        &IT_UNITS,
        &NL_UNITS,
        &PL_UNITS,
        &PT_UNITS,
        &EN_ORDINALS,
        &PL_ORDINALS,
        &LA_ORDINALS,
        &LA_DAYS,
    ];
    for table in tables {
        for (i, word) in table.iter().enumerate() {
            // Two-word ordinals ("decimo tertio") are read word by word.
            if !word.is_empty() && !word.contains(' ') {
                add(word, Piece::Add(i as u32));
            }
        }
    }
    let tens: [&[&str]; 11] = [
        &EN_TENS,
        &FR_TENS,
        &DE_TENS,
        &ES_TENS,
        &IT_TENS,
        &NL_TENS,
        &PL_TENS,
        &PT_TENS,
        &EN_TENS_ORDINALS,
        &PL_TENS_ORDINALS,
        &LA_TENS_ORDINALS,
    ];
    for table in tens {
        for (i, word) in table.iter().enumerate() {
            if !word.is_empty() {
                add(word, Piece::Add(i as u32 * 10));
            }
        }
    }
    for table in [
        &ES_HUNDREDS,
        &PT_HUNDREDS,
        &PL_HUNDREDS,
        &PL_HUNDREDS_ORDINALS,
        &LA_HUNDREDS_ORDINALS,
    ] {
        // "ciento", "cento" and "sto" are added below.
        for (i, word) in table.iter().enumerate().skip(1) {
            add(word, Piece::Add(i as u32 * 100));
        }
    }
    for (i, word) in PL_THOUSANDS_ORDINALS.iter().enumerate().skip(2) {
        add(word, Piece::Add(i as u32 * 1000));
    }
}

/// The forms of numbers the tables do not spell out but a text may hold:
/// inflected Latin ordinals, elided Italian tens, and the ordinals German and
/// Dutch form by rule.
fn add_derived_numbers(map: &mut HashMap<String, Piece>) {
    let mut add = |word: &str, piece: Piece| add_piece(map, word, piece);
    // Latin feminine forms of every ordinal, and the Italian tens that lose
    // their vowel.
    for table in [&LA_ORDINALS[..], &LA_TENS_ORDINALS[..]] {
        for (i, word) in table.iter().enumerate() {
            let value = if table.len() == 10 {
                i as u32 * 10
            } else {
                i as u32
            };
            if !word.is_empty() && !word.contains(' ') {
                let stem = &word[..word.len() - 1];
                for ending in ["a", "us", "um", "ae", "i"] {
                    add(&format!("{stem}{ending}"), Piece::Add(value));
                }
            }
        }
    }
    for (i, word) in IT_TENS.iter().enumerate().skip(2) {
        add(&word[..word.len() - 1], Piece::Add(i as u32 * 10));
    }
    // Ordinals the writers form by rule.
    for n in 1..=31 {
        add(&de_ordinal(n), Piece::Add(n));
        let stem = de_ordinal(n);
        let stem = &stem[..stem.len() - 1];
        for ending in ["", "r", "s"] {
            add(&format!("{stem}{ending}"), Piece::Add(n));
        }
        add(&nl_ordinal(n), Piece::Add(n));
    }
}

/// Irregular number words, and the words that multiply or join numbers.
fn add_irregular_words(map: &mut HashMap<String, Piece>) {
    let mut add = |word: &str, piece: Piece| add_piece(map, word, piece);
    for (word, value) in [
        ("premier", 1),
        ("premiere", 1),
        ("primero", 1),
        ("primer", 1),
        ("primo", 1),
        ("primeiro", 1),
        ("second", 2),
        ("seconde", 2),
        ("un", 1),
        ("une", 1),
        ("ein", 1),
        ("eine", 1),
        ("uno", 1),
        ("una", 1),
        ("um", 1),
        ("uma", 1),
        ("duas", 2),
        ("veinti", 20),
        ("tre", 3),
        ("bis", 2),
        ("ter", 3),
        ("quatrevingt", 80),
        ("quatrevingts", 80),
        ("vingts", 20),
        ("dwadziescia", 20),
        ("sto", 100),
    ] {
        add(word, Piece::Add(value));
    }
    for word in [
        "hundred", "cent", "cents", "hundert", "cento", "honderd", "ciento", "cien", "cem",
    ] {
        add(word, Piece::Hundred);
    }
    for word in [
        "thousand",
        "mille",
        "mil",
        "tausend",
        "mila",
        "duizend",
        "tysiac",
        "tysiace",
        "tysiecy",
        "millesimo",
        "millesima",
        "millesimus",
        "tysiecznego",
    ] {
        add(word, Piece::Thousand);
    }
    for word in ["and", "et", "und", "y", "e", "en"] {
        add(word, Piece::And);
    }
}

/// A folded word cut into number pieces, when it is made of nothing else:
/// "sechzehnhundertfunfzig" is sechzehn, hundert, funfzig. The cut with the
/// fewest pieces wins, so "tredici" stays thirteen.
fn pieces(word: &str) -> Option<Vec<Piece>> {
    let bytes = word.as_bytes();
    let mut best: Vec<Option<(usize, usize)>> = vec![None; bytes.len() + 1];
    let mut cost: Vec<usize> = vec![usize::MAX; bytes.len() + 1];
    cost[0] = 0;
    for end in 1..=bytes.len() {
        for start in 0..end {
            if cost[start] == usize::MAX {
                continue;
            }
            if PIECES.contains_key(&word[start..end]) && cost[start] + 1 < cost[end] {
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
        out.push(PIECES[&word[start..end]]);
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

/// Every name a month is known by, folded.
static MONTH_NAMES: LazyLock<HashMap<String, u8>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    let tables: [&[&str; 12]; 12] = [
        &MONTHS_EN,
        &MONTHS_FR,
        &MONTHS_DE,
        &MONTHS_ES,
        &MONTHS_IT,
        &MONTHS_NL,
        &MONTHS_PL,
        &MONTHS_PL_GENITIVE,
        &MONTHS_PT,
        &MONTHS_LA,
        &MONTHS_LA_GENITIVE,
        &MONTHS_GEDCOM,
    ];
    for table in tables {
        for (i, name) in table.iter().enumerate() {
            map.insert(fold_words(name), i as u8 + 1);
        }
    }
    for (i, forms) in MONTHS_LA_OTHER.iter().enumerate() {
        for form in forms {
            map.insert(fold_words(form), i as u8 + 1);
        }
    }
    map
});

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
    if let Some(month) = MONTH_NAMES.get(word) {
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
