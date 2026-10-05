//! Choosing the cited register among the registers a portal's search
//! returned, the same way on every platform (Archive Portals §4.3, step 3).
//!
//! An adapter reads each result into a [`Candidate`]: what the portal
//! displays of the register — locality, call number, act, parish, period,
//! image count — plus an opaque payload of its own, such as the record and
//! viewer addresses it needs once the register is chosen. [`select`] then:
//!
//! 1. keeps the candidates whose locality, folded, is one of the accepted
//!    forms (portal searches often match the locality as text: `Bourg (Le)`
//!    also finds `Saint-Exemple-lès-le-Bourg`);
//! 2. narrows them by the citation's parts, in order: call number, act kind,
//!    parish, period, the act or matricule number within the numbers a
//!    register spans, image count, stopping at the first that leaves exactly
//!    one. A part the citation lacks is skipped, and so is one that would
//!    leave none, since a portal may write a parish or a period differently.
//!    The call number is the exception: a cited call number that no
//!    candidate carries means the register is not among them, and the
//!    answer is the results rather than a guess.
//!
//! The period helpers read the period texts portals display, segments,
//! act codes, notes and Republican years included.

use crate::citation::{Act, CitationParts, republican_numeral, republican_start};

use super::markup::fold;

/// One register a search returned, as the portal displays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate<T> {
    pub(crate) locality: Option<String>,
    /// Compared with the cited one without spaces or case.
    pub(crate) call_number: Option<String>,
    /// The acts as displayed: codes (`BMS`) or words (`Baptêmes`).
    pub(crate) act: Option<String>,
    pub(crate) parish: Option<String>,
    /// The period as displayed, read by [`covers`].
    pub(crate) period: Option<String>,
    pub(crate) images: Option<u16>,
    /// The act or matricule numbers the register spans, first and last, when
    /// the portal shows them (`n° 1 à 1586`), read by [`number_range`].
    pub(crate) numbers: Option<(u32, u32)>,
    /// What the adapter needs to open the register.
    pub(crate) payload: T,
}

/// The outcome of a selection.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Selection<'c, T> {
    One(&'c Candidate<T>),
    /// No candidate, or several the citation cannot tell apart: the count.
    Many(usize),
}

/// One narrowing test of the candidates.
type Criterion<'c, T> = Box<dyn Fn(&Candidate<T>) -> bool + 'c>;

/// Selects the cited register. `localities` are the forms of the cited
/// locality a candidate may show: as cited, and as the portal writes it.
pub(crate) fn select<'c, T>(
    candidates: &'c [Candidate<T>],
    citation: &CitationParts,
    localities: &[&str],
) -> Selection<'c, T> {
    match narrow(candidates, citation, localities).as_slice() {
        [only] => Selection::One(only),
        many => Selection::Many(many.len()),
    }
}

/// The candidates the citation's parts leave: one when the citation decides,
/// otherwise those it cannot tell apart (none when the locality matches no
/// candidate). A portal whose result rows lack a part, such as the image
/// count, lets its adapter read that part from each remaining register and
/// select again.
pub(crate) fn narrow<'c, T>(
    candidates: &'c [Candidate<T>],
    citation: &CitationParts,
    localities: &[&str],
) -> Vec<&'c Candidate<T>> {
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    // A series cited without a locality, such as a department's military
    // registers, keeps every candidate.
    let anywhere = wanted.iter().all(String::is_empty);
    let mut kept: Vec<&Candidate<T>> = candidates
        .iter()
        .filter(|candidate| {
            anywhere
                || candidate
                    .locality
                    .as_deref()
                    .is_some_and(|locality| wanted.contains(&fold(locality)))
        })
        .collect();
    if kept.len() <= 1 {
        return kept;
    }

    if let Some(call_number) = &citation.call_number {
        let matching: Vec<&Candidate<T>> = kept
            .iter()
            .copied()
            .filter(|candidate| {
                candidate
                    .call_number
                    .as_deref()
                    .is_some_and(|written| call_number.matches(written))
            })
            .collect();
        match matching.len() {
            0 => return kept,
            1 => return matching,
            _ => kept = matching,
        }
    }

    for criterion in criteria(citation).iter().flatten() {
        let matching: Vec<&Candidate<T>> = kept
            .iter()
            .copied()
            .filter(|candidate| criterion(candidate))
            .collect();
        match matching.len() {
            0 => {}
            1 => return matching,
            _ => kept = matching,
        }
    }
    kept
}

/// The criteria after the call number, in order; `None` for a part the
/// citation lacks.
fn criteria<'c, T>(citation: &'c CitationParts) -> [Option<Criterion<'c, T>>; 5] {
    [
        Some(
            Box::new(|candidate: &Candidate<T>| holds_act(candidate.act.as_deref(), &citation.act))
                as Criterion<'c, T>,
        ),
        citation.parish.as_deref().map(|parish| {
            let parish = fold(parish);
            Box::new(move |candidate: &Candidate<T>| {
                candidate
                    .parish
                    .as_deref()
                    .is_some_and(|written| fold(written) == parish)
            }) as Criterion<'c, T>
        }),
        citation.year.map(|year| {
            Box::new(move |candidate: &Candidate<T>| {
                candidate
                    .period
                    .as_deref()
                    .is_some_and(|period| covers(period, year))
            }) as Criterion<'c, T>
        }),
        citation.number.map(|number| {
            Box::new(move |candidate: &Candidate<T>| {
                candidate
                    .numbers
                    .is_some_and(|(first, last)| (first..=last).contains(&number))
            }) as Criterion<'c, T>
        }),
        citation.view_count.map(|count| {
            Box::new(move |candidate: &Candidate<T>| candidate.images == Some(count))
                as Criterion<'c, T>
        }),
    ]
}

/// Whether a displayed act admits the cited act. Acts written as codes
/// (`BMS`, `NMD`, `TD`, `RP`) must hold every cited kind (publications of
/// banns with the marriages), or be the cited table or series; acts in words
/// (`Baptêmes, mariages et sépultures`), with a period (`N 1903 - 1912`), or
/// not displayed are left to the portal's own filter.
pub(crate) fn holds_act(displayed: Option<&str>, act: &Act) -> bool {
    let Some(written) = displayed.and_then(|text| Act::from_code(text.trim())) else {
        return true;
    };
    match act {
        Act::Register(kinds) => kinds.iter().all(|kind| written.includes(*kind)),
        Act::Table(_) | Act::Series(_) => &written == act,
    }
}

/// The words, folded, that introduce the numbers a register spans in a
/// title: `n° 1 à 1586` (read as `no`), `nos 1 à 500`, `matricules 1 à 1586`.
const RANGE_WORDS: [&str; 6] = ["no", "nos", "numero", "numeros", "matricule", "matricules"];

/// The first and last numbers a register spans: `1 à 1586` or `1-1586`
/// after one of the [`RANGE_WORDS`] in a title or label, or anywhere in a
/// cell that holds the numbers alone (`marked` false). Years elsewhere in a
/// title (`classes 1859 à 1940`, `N 1903-1912`) are not read, since no range
/// word precedes them. One number alone, after a range word (`matricule
/// 984`) or as the whole cell (`984`), spans itself: the row of one person
/// in an index of matricules.
pub(crate) fn number_range(text: &str, marked: bool) -> Option<(u32, u32)> {
    // `n°` would fold to a bare `n`, which also stands for births.
    let folded = fold(&text.replace(['°', 'º'], "o "));
    let words: Vec<&str> = folded.split(' ').collect();
    let number = |at: usize| -> Option<u32> {
        let word = words.get(at)?;
        (word.len() <= 7 && word.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| word.parse().ok())
            .flatten()
    };
    if !marked && words.len() == 1 {
        return number(0).map(|alone| (alone, alone));
    }
    (0..words.len()).find_map(|at| {
        let start = if marked {
            RANGE_WORDS.contains(&words[at]).then_some(at + 1)?
        } else {
            at
        };
        let first = number(start)?;
        // `1 à 1586` folds to `1 a 1586`, and `1-1586` to `1 1586`.
        let last = match words.get(start + 1) {
            Some(&"a" | &"au") => number(start + 2)?,
            _ => match number(start + 1) {
                Some(last) => last,
                None if marked => first,
                None => return None,
            },
        };
        (first <= last).then_some((first, last))
    })
}

/// Whether a displayed period covers `year`. The text may hold several
/// segments with act codes and notes between them — `1598-1613 , 1656-1667`,
/// `NMD 1857-1859, N 1853-1872`, `N 1903 - 1912`, `NM an II` — so every year
/// or range in it is read.
pub(crate) fn covers(text: &str, year: u16) -> bool {
    period_ranges(text)
        .into_iter()
        .any(|(first, last)| (first..=last).contains(&year))
}

/// The Gregorian year ranges of a displayed period: every four-digit year
/// and every Republican `an <numeral>` (which spans two Gregorian years),
/// two of them joined into one range as [`joins_range`] reads the text
/// between them.
pub(crate) fn period_ranges(text: &str) -> Vec<(u16, u16)> {
    let tokens = year_tokens(text);
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let mut last = token.last;
        if let Some(next) = tokens.get(index + 1)
            && joins_range(&text[token.end..next.start])
        {
            last = next.last;
            index += 1;
        }
        ranges.push((token.first, last.max(token.first)));
        index += 1;
    }
    ranges
}

/// The French month names, folded, which full dates write between the years
/// of a period.
const MONTHS: [&str; 12] = [
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

/// Whether the text between two years joins them into one range: a dash, a
/// slash, `à` or `au` (`1683/1750`, `1833 à 1852`), or nothing but spaces
/// (`1841 1860`), with the days and months of full dates written around the
/// years (`13/11/1697 - 06/11/1707`, `26 juillet 1849-20 février 1850`). A
/// comma, a word or a note separates two periods.
fn joins_range(between: &str) -> bool {
    between
        .replace(['-', '/', '\u{2013}'], " ")
        .split_whitespace()
        .all(|word| {
            let folded = fold(word);
            let is_day =
                (1..=2).contains(&folded.len()) && folded.bytes().all(|byte| byte.is_ascii_digit());
            is_day
                || ["a", "au", "1er"].contains(&folded.as_str())
                || MONTHS.contains(&folded.as_str())
        })
}

/// A year written in a period: its byte span and the Gregorian years it
/// covers.
struct YearToken {
    start: usize,
    end: usize,
    first: u16,
    last: u16,
}

fn is_word_byte(text: &str, at: usize) -> bool {
    text.as_bytes()
        .get(at)
        .is_some_and(u8::is_ascii_alphanumeric)
}

/// Four digits standing alone, from `at`.
fn gregorian_token(text: &str, at: usize) -> Option<YearToken> {
    let digits = text[at..].bytes().take_while(u8::is_ascii_digit).count();
    let end = at + digits;
    if digits != 4 || (at > 0 && is_word_byte(text, at - 1)) || is_word_byte(text, end) {
        return None;
    }
    let year = text[at..end].parse().ok()?;
    Some(YearToken {
        start: at,
        end,
        first: year,
        last: year,
    })
}

/// `an <numeral>`, from `at`.
fn republican_token(text: &str, at: usize) -> Option<YearToken> {
    if (at > 0 && is_word_byte(text, at - 1)) || !text.get(at..at + 3)?.eq_ignore_ascii_case("an ")
    {
        return None;
    }
    let numeral_start = at + 3;
    let length = text[numeral_start..]
        .bytes()
        .take_while(u8::is_ascii_alphanumeric)
        .count();
    let end = numeral_start + length;
    let first = republican_start(republican_numeral(&text[numeral_start..end])?)?;
    Some(YearToken {
        start: at,
        end,
        first,
        last: first + 1,
    })
}

fn year_tokens(text: &str) -> Vec<YearToken> {
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if !text.is_char_boundary(at) {
            at += 1;
            continue;
        }
        let token = if text.as_bytes()[at].is_ascii_digit() {
            gregorian_token(text, at)
        } else {
            republican_token(text, at)
        };
        match token {
            Some(token) => {
                at = token.end;
                tokens.push(token);
            }
            None if text.as_bytes()[at].is_ascii_digit() => {
                at += text[at..].bytes().take_while(u8::is_ascii_digit).count();
            }
            None => at += 1,
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citation::CitationGrammar;

    #[test]
    fn reads_every_segment_of_a_period() {
        assert_eq!(period_ranges("1658-1668"), [(1658, 1668)]);
        assert_eq!(
            period_ranges("1598-1613 , 1656-1667"),
            [(1598, 1613), (1656, 1667)]
        );
        assert_eq!(period_ranges("1700-1701 (janvier)"), [(1700, 1701)]);
        assert_eq!(
            period_ranges("NMD 1857-1859, N 1853-1872, MD 1853-1856,1860-1872"),
            [(1857, 1859), (1853, 1872), (1853, 1856), (1860, 1872)]
        );
        assert_eq!(period_ranges("N 1903 - 1912"), [(1903, 1912)]);
        assert_eq!(period_ranges("NM an II"), [(1793, 1794)]);
        assert_eq!(period_ranges("1792-an II"), [(1792, 1794)]);
        assert_eq!(
            period_ranges("BMS 1595-1692 (consulter le détail)"),
            [(1595, 1692)]
        );
        assert_eq!(period_ranges("acte 12345, Exemplan III"), []);

        assert!(!covers("1598-1613 , 1656-1667", 1650));
        assert!(covers("1613-1655", 1650));
    }

    #[test]
    fn reads_ranges_written_with_a_slash_a_word_spaces_or_full_dates() {
        for (text, range) in [
            ("1683/1750", (1683, 1750)),
            ("1833 à 1852", (1833, 1852)),
            ("1841 1860", (1841, 1860)),
            ("13/11/1697 - 06/11/1707", (1697, 1707)),
            ("01/01/1700-31/12/1766", (1700, 1766)),
            ("26 juillet 1849-20 février 1850", (1849, 1850)),
            ("Juin 1732 - Mars 1762", (1732, 1762)),
        ] {
            assert_eq!(period_ranges(text), [range], "{text}");
        }
        assert_eq!(
            period_ranges("Baptêmes (1512-1569 (incomplet), 1597-1673)"),
            [(1512, 1569), (1597, 1673)]
        );
        assert_eq!(period_ranges("1851, 1856"), [(1851, 1851), (1856, 1856)]);
    }

    #[test]
    fn reads_the_numbers_a_register_spans() {
        for (text, marked, expected) in [
            (
                "Bureau de Exampleville n° 1 à 1586 (1870)",
                true,
                Some((1, 1586)),
            ),
            (
                "Registre matricule, classe 1870, N°501-1000",
                true,
                Some((501, 1000)),
            ),
            ("matricules 1 au 520", true, Some((1, 520))),
            ("nos 1 à 500", true, Some((1, 500))),
            // Years and act codes are no range of numbers.
            ("Registres matricules des classes 1859 à 1940", true, None),
            ("N 1903 - 1912", true, None),
            ("n° 900 à 12", true, None),
            ("1 à 500", true, None),
            // A cell holding the numbers alone needs no range word.
            ("1 à 500", false, Some((1, 500))),
            ("1-500", false, Some((1, 500))),
            ("classe 1870, 1 à 500", false, Some((1, 500))),
            // One person's matricule spans itself.
            ("984", false, Some((984, 984))),
            ("Bureau 984", false, None),
            ("ACHARD Louis (matricule 984)", true, Some((984, 984))),
            ("Matricule n°1", true, Some((1, 1))),
        ] {
            assert_eq!(number_range(text, marked), expected, "{text}");
        }
    }

    #[test]
    fn a_series_without_a_locality_keeps_every_candidate() {
        let mut candidates = [
            candidate("Exampleville", "1 R 1", "RM", "1871", 196, 1),
            candidate("Elsewhere", "1 R 2", "RM", "1871", 200, 2),
            candidate("Elsewhere", "1 R 3", "RM", "1872", 210, 3),
        ];
        candidates[1].numbers = Some((1, 500));
        candidates[2].numbers = Some((1, 500));
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Registres matricules des classes 1859 à 1940 - 1871 - vue 5/200"
            ),
            Selection::Many(102)
        );
        // The matricule leaves the one register spanning it.
        candidates[0].numbers = Some((501, 900));
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Registres matricules des classes 1859 à 1940 - 1871 - matricule 640"
            ),
            Selection::Many(101)
        );
        assert_eq!(
            chosen(&candidates, "AB12 - Exampleville - Recensement - 1871"),
            Selection::Many(101)
        );
        // A register of another kind written as a code is not a census.
        assert!(!holds_act(Some("RM"), &Act::from_code("RP").unwrap()));
        assert!(holds_act(Some("NMD"), &Act::from_code("PM").unwrap()));
    }

    #[test]
    fn reads_acts_written_as_codes_only() {
        let birth = Act::from_code("N").unwrap();
        assert!(holds_act(Some("NMD"), &birth));
        assert!(!holds_act(Some("BMS"), &birth));
        assert!(holds_act(Some("Naissances"), &birth));
        assert!(holds_act(Some("N 1903 - 1912"), &birth));
        assert!(holds_act(None, &birth));
        let table = Act::from_code("TD").unwrap();
        assert!(holds_act(Some("TD"), &table));
        assert!(!holds_act(Some("NMD"), &table));
    }

    fn candidate(
        locality: &str,
        call_number: &str,
        act: &str,
        period: &str,
        images: u16,
        payload: u8,
    ) -> Candidate<u8> {
        Candidate {
            locality: Some(locality.to_owned()),
            call_number: Some(call_number.to_owned()).filter(|text| !text.is_empty()),
            act: Some(act.to_owned()),
            parish: None,
            period: Some(period.to_owned()),
            images: Some(images),
            numbers: None,
            payload,
        }
    }

    /// The payload of the chosen candidate plus 100, or the count of those
    /// left. The portal writes `Le Bourg` as `Bourg (Le)`.
    fn chosen(candidates: &[Candidate<u8>], title: &str) -> Selection<'static, u8> {
        let citation = CitationParts::parse(title, &CitationGrammar::default()).unwrap();
        let styled = match citation.locality.strip_prefix("Le ") {
            Some(name) => format!("{name} (Le)"),
            None => citation.locality.clone(),
        };
        match select(candidates, &citation, &[&citation.locality, &styled]) {
            Selection::One(candidate) => Selection::Many(usize::from(candidate.payload) + 100),
            Selection::Many(count) => Selection::Many(count),
        }
    }

    #[test]
    fn narrows_in_order_and_refuses_to_guess_a_call_number() {
        let candidates = [
            candidate("Bourg (Le)", "1MI 9 R1", "BMS", "BMS 1692-1729", 89, 1),
            candidate("Bourg (Le)", "1MI 9 R1", "BMS", "BMS 1730-1764", 115, 2),
            candidate("Bourg (Le)", "1MI 9 R2", "NMD", "NMD 1793-1802", 301, 3),
            candidate(
                "Saint-Exemple-lès-le-Bourg",
                "2 Mi 1",
                "BMS",
                "BMS 1700",
                10,
                4,
            ),
        ];
        // Only the locality's own rows count, whatever the text match.
        assert_eq!(
            chosen(&candidates, "AB12 - Le Bourg - (aucun) - N - acte 1"),
            Selection::Many(103)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - B - 1700 - 1 MI 9 R1"
            ),
            Selection::Many(101)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - B - acte 1 - vue 3/115"
            ),
            Selection::Many(102)
        );
        // A call number no row carries: the results, not a guess.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - B - 1700 - 1MI 9 R9"
            ),
            Selection::Many(3)
        );
        // Nothing tells the two BMS registers apart.
        assert_eq!(
            chosen(&candidates, "AB12 - Le Bourg - (aucun) - B - acte 1"),
            Selection::Many(2)
        );
        assert_eq!(
            chosen(&candidates, "AB12 - Elsewhere - (aucun) - B - 1700"),
            Selection::Many(0)
        );
    }
}
