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
//!    also finds `Saint-Exemple-lès-le-Bourg`) or a name the place
//!    dictionary knows for it; failing any, those of a commune renamed
//!    since the citation, on the citation's evidence ([`OtherNames`]);
//! 2. narrows them by the citation's parts, in order: call number, act kind,
//!    parish, period, the act or matricule number within the numbers a
//!    register spans, image count, stopping at the first that leaves exactly
//!    one. A part the citation lacks is skipped, and so is one that would
//!    leave none, since a portal may write a parish or a period differently.
//!    The call number is the exception: a cited call number that no
//!    candidate carries means the register may not be among them, and the
//!    answer is the results rather than a guess — unless exactly one
//!    candidate both covers the cited year and has the cited image count,
//!    strong evidence of a call number the portal writes otherwise. Several
//!    candidates carrying the cited call number are parts of one register,
//!    each dated by where it starts: the cited number is tried before the
//!    period among them.
//!
//! The period helpers read the period texts portals display, segments,
//! act codes, notes and Republican years included.

use crate::citation::{
    Act, CitationGrammar, CitationParts, MONTHS, day_before, ordinal, republican_numeral,
    republican_start,
};

use super::locality::OtherNames;
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
    let others = OtherNames::new(citation, localities);
    // A name the place dictionary knows for the cited locality is the
    // cited locality.
    let mut kept: Vec<&Candidate<T>> = candidates
        .iter()
        .filter(|candidate| {
            anywhere
                || candidate.locality.as_deref().is_some_and(|locality| {
                    wanted.contains(&fold(locality)) || others.is_known(locality)
                })
        })
        .collect();
    if kept.is_empty() && !anywhere {
        kept = renamed_locality(candidates, citation, &others);
    }
    if let [only] = kept.as_slice()
        && contradicts(only, citation)
    {
        return Vec::new();
    }
    if kept.len() <= 1 {
        return kept;
    }

    let mut parts_of_register = false;
    if let Some(call_number) = &citation.call_number {
        // The rows carrying the most of the cited call numbers: a register
        // cited by its microfilm's, shared by several, and its own.
        let carried = |candidate: &Candidate<T>| {
            candidate
                .call_number
                .as_deref()
                .map_or(0, |written| call_number.matched(written))
        };
        let most = kept.iter().map(|candidate| carried(candidate)).max();
        let matching: Vec<&Candidate<T>> = kept
            .iter()
            .copied()
            .filter(|candidate| most.is_some_and(|most| most > 0 && carried(candidate) == most))
            .collect();
        match matching.len() {
            0 => return without_cited_call_number(kept, citation),
            1 => return matching,
            _ => {
                kept = matching;
                parts_of_register = true;
            }
        }
    }

    for criterion in criteria(citation, parts_of_register).iter().flatten() {
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
    nearest_image_count(kept, citation)
}

/// Whether a register shows both another call number than every cited one
/// and another image count than the cited one: a citation of another
/// digitisation (a microfilm's, since replaced by the originals, split
/// otherwise), which no period overlap makes the cited register, even when
/// the search returned it alone.
fn contradicts<T>(candidate: &Candidate<T>, citation: &CitationParts) -> bool {
    let other_call_number = citation.call_number.as_ref().is_some_and(|cited| {
        candidate
            .call_number
            .as_deref()
            .is_some_and(|written| !cited.matches(written))
    });
    let other_count = citation
        .view_count
        .is_some_and(|count| candidate.images.is_some_and(|images| images != count));
    other_call_number && other_count
}

/// The candidates of a locality the portal names otherwise than the
/// citation, when none bears the cited name: a commune renamed since the
/// citation was written, its new name extending the old one at a word
/// (`Exampleville` become `Exampleville-en-Plaine`) or the old one without
/// its qualifier (`Exampleville-sous-Bois` become `Exampleville`), or a name
/// the portal qualifies (`Exampleville (Department, France)`) — see
/// [`OtherNames::renamed`]. The portal's own locality filter returned them,
/// or a search for the other name; each must also agree with every part of
/// the citation it shows, and carry a cited call number or the cited image
/// count, since such a name may as well be another commune's
/// (`Exampleville-la-Forêt`, an `Exampleville` elsewhere).
fn renamed_locality<'c, T>(
    candidates: &'c [Candidate<T>],
    citation: &CitationParts,
    others: &OtherNames,
) -> Vec<&'c Candidate<T>> {
    let cited_call_number = |candidate: &Candidate<T>| {
        citation.call_number.as_ref().is_some_and(|cited| {
            candidate
                .call_number
                .as_deref()
                .is_some_and(|written| cited.matches(written))
        })
    };
    let cited_count = |candidate: &Candidate<T>| {
        citation.view_count.is_some() && candidate.images == citation.view_count
    };
    let agrees = |candidate: &Candidate<T>| {
        let call_number = citation.call_number.is_none()
            || candidate.call_number.is_none()
            || cited_call_number(candidate);
        let period = citation.year.is_none_or(|year| {
            candidate
                .period
                .as_deref()
                .is_none_or(|period| covers(period, year))
        });
        let number = citation.number.is_none_or(|number| {
            candidate
                .numbers
                .is_none_or(|(first, last)| (first..=last).contains(&number))
        });
        // A register carrying the cited call number may count other images
        // since (§7, renumbering).
        let count = citation.view_count.is_none()
            || candidate.images.is_none()
            || candidate.images == citation.view_count
            || cited_call_number(candidate);
        call_number
            && period
            && number
            && count
            && holds_act(candidate.act.as_deref(), &citation.act)
    };
    candidates
        .iter()
        .filter(|candidate| {
            candidate
                .locality
                .as_deref()
                .is_some_and(|locality| others.renamed(locality).is_some())
        })
        .filter(|candidate| agrees(candidate))
        .filter(|candidate| cited_call_number(candidate) || cited_count(candidate))
        .collect()
}

/// The candidates left once every criterion has run: the one whose image
/// count is nearest the cited view count, when exactly one is and within a
/// tenth of it — a portal may have added or removed a few images since the
/// citation was written (429 for a cited 425) —, all of them otherwise.
fn nearest_image_count<'c, T>(
    kept: Vec<&'c Candidate<T>>,
    citation: &CitationParts,
) -> Vec<&'c Candidate<T>> {
    let Some(count) = citation.view_count.filter(|_| kept.len() > 1) else {
        return kept;
    };
    let distance = |candidate: &Candidate<T>| {
        candidate
            .images
            .map(|images| images.abs_diff(count))
            .filter(|distance| *distance <= (count / 10).max(1))
    };
    let Some(nearest) = kept
        .iter()
        .filter_map(|candidate| distance(candidate))
        .min()
    else {
        return kept;
    };
    let nearest: Vec<&Candidate<T>> = kept
        .iter()
        .copied()
        .filter(|candidate| distance(candidate) == Some(nearest))
        .collect();
    if nearest.len() == 1 { nearest } else { kept }
}

/// The candidates left when no candidate carries the cited call number,
/// which a portal may write otherwise than the citation: the one register
/// whose period covers the cited year and whose image count is the cited
/// view count, when exactly one does — both are strong evidence —, and all
/// of them otherwise, the results rather than a guess.
fn without_cited_call_number<'c, T>(
    kept: Vec<&'c Candidate<T>>,
    citation: &CitationParts,
) -> Vec<&'c Candidate<T>> {
    let (Some(year), Some(count)) = (citation.year, citation.view_count) else {
        return kept;
    };
    let evidence: Vec<&Candidate<T>> = kept
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.images == Some(count)
                && candidate
                    .period
                    .as_deref()
                    .is_some_and(|period| covers(period, year))
        })
        .collect();
    if evidence.len() == 1 { evidence } else { kept }
}

/// The criteria after the call number, in order; `None` for a part the
/// citation lacks. After the cited year come, for a cited period other than
/// one whole year — the register's own period rather than the act's year —,
/// its bounds, to the day where both the citation and the row write dates:
/// a row holding the whole period, then one spanning exactly it, then one
/// overlapping it (a register of half a month among a year's). Among the
/// parts of one register, `parts_of_register`, the cited number comes before
/// the year and the period: a part's period dates where it starts, its
/// numbers bound it.
fn criteria<'c, T>(
    citation: &'c CitationParts,
    parts_of_register: bool,
) -> [Option<Criterion<'c, T>>; 8] {
    let cited = citation
        .period
        .as_deref()
        .map(period_spans)
        .and_then(|spans| {
            let first = spans.iter().map(|span| span.0).min()?;
            let last = spans.iter().map(|span| span.1).max()?;
            Some((first, last))
        })
        .filter(|&(first, last)| !(first % 10_000 == 101 && last == first + 1130));
    let spanning = move |test: fn((u32, u32), (u32, u32)) -> bool| {
        cited.map(|cited| {
            Box::new(move |candidate: &Candidate<T>| {
                candidate.period.as_deref().is_some_and(|period| {
                    period_spans(period)
                        .into_iter()
                        .any(|span| test(span, cited))
                })
            }) as Criterion<'c, T>
        })
    };
    let mut criteria = [
        Some(
            Box::new(|candidate: &Candidate<T>| holds_act(candidate.act.as_deref(), &citation.act))
                as Criterion<'c, T>,
        ),
        citation.parish.as_deref().map(|parish| {
            let parish = parish_key(parish);
            Box::new(move |candidate: &Candidate<T>| {
                candidate
                    .parish
                    .as_deref()
                    .is_some_and(|written| parish_key(written) == parish)
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
        spanning(|span, cited| span.0 <= cited.0 && cited.1 <= span.1),
        spanning(|span, cited| span == cited),
        spanning(|span, cited| span.0 <= cited.1 && cited.0 <= span.1),
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
    ];
    if parts_of_register {
        // The number, after the year and the three period bounds, first.
        criteria[2..7].rotate_right(1);
    }
    criteria
}

/// A parish as compared: folded, and a section of a large commune's
/// registers read as its number whichever way it is written (`3e section`,
/// `Section 3`, `3ème section` are `section 3`).
pub(crate) fn parish_key(text: &str) -> String {
    let folded = fold(text);
    let words: Vec<&str> = folded.split(' ').collect();
    let section = match words.as_slice() {
        [number, "section"] => ordinal(number),
        ["section", number] => number.parse::<u32>().ok(),
        _ => None,
    };
    section.map_or(folded, |number| format!("section {number}"))
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

/// The act of a register as the codes `select` reads: the series its text
/// names in the citation vocabulary (`RP` for `Recensement de population`),
/// the kinds its text names (`BMS`), or `TD` for decennial tables. With
/// `codes`, a word made of act letters (`N`, `B, M, S`, `BMS`) names kinds
/// too; a title would take an initial for one.
pub(crate) fn act_code(text: &str, codes: bool) -> Option<String> {
    if let Some(series) = CitationGrammar::default().series_of(text) {
        return Some(series.code().to_owned());
    }
    let folded = fold(text);
    let words: Vec<&str> = folded.split(' ').collect();
    if words.iter().any(|word| word.starts_with("decennal")) {
        return Some("TD".to_owned());
    }
    let mut kinds = String::new();
    let mut add = |letter: char| {
        if !kinds.contains(letter) {
            kinds.push(letter);
        }
    };
    for word in &words {
        match *word {
            word if word.starts_with("baptem") => add('B'),
            word if word.starts_with("mariage") => add('M'),
            word if word.starts_with("naissance") => add('N'),
            "deces" => add('D'),
            word if word.starts_with("sepultur") => add('S'),
            _ => {}
        }
    }
    if codes {
        for token in text.split(|c: char| !c.is_alphanumeric()) {
            if !token.is_empty() && token.chars().all(|c| "NBMDS".contains(c)) {
                token.chars().for_each(&mut add);
            }
        }
    }
    if !kinds.is_empty() {
        return Some(kinds);
    }
    words.iter().any(|word| word.starts_with("tabl")).then(|| {
        if words.iter().any(|word| word.starts_with("annuel")) {
            "TA".to_owned()
        } else {
            "TD".to_owned()
        }
    })
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
    period_spans(text)
        .into_iter()
        .map(|(first, last)| (year_of(first), year_of(last)))
        .collect()
}

/// The year of a date written `yyyymmdd`.
fn year_of(date: u32) -> u16 {
    u16::try_from(date / 10_000).unwrap_or(u16::MAX)
}

/// The date ranges of a displayed period, as [`period_ranges`] reads its
/// years, each bound a `yyyymmdd` number: to the day where the text writes
/// the day and month before the year (`05/03/1871`, `26 juillet 1849`), to
/// the month where it writes the month alone (`juillet 1849`), and otherwise
/// from the first day of the first year to the last day of the last.
pub(crate) fn period_spans(text: &str) -> Vec<(u32, u32)> {
    let tokens = year_tokens(text);
    let date = |token: &YearToken, end: bool| {
        let year = u32::from(if end { token.last } else { token.first });
        let (month, day) = match day_before(&text[..token.start]) {
            Some((month, Some(day))) => (month, day),
            Some((month, None)) => (month, if end { 31 } else { 1 }),
            None if end => (12, 31),
            None => (1, 1),
        };
        year * 10_000 + month * 100 + day
    };
    let mut spans = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let mut last = date(token, true);
        if let Some(next) = tokens.get(index + 1)
            && joins_range(&text[token.end..next.start])
        {
            last = date(next, true);
            index += 1;
        }
        let first = date(token, false);
        spans.push((first, last.max(first)));
        index += 1;
    }
    spans
}

/// Whether the text between two years joins them into one range: a dash, a
/// slash, an ellipsis, `à` or `au` (`1683/1750`, `1621...1687`, `1833 à
/// 1852`), or nothing but spaces
/// (`1841 1860`), with the days and months of full dates written around the
/// years (`13/11/1697 - 06/11/1707`, `26 juillet 1849-20 février 1850`). A
/// comma, a word or a note separates two periods.
fn joins_range(between: &str) -> bool {
    between
        .replace("...", " ")
        .replace(['-', '/', '\u{2013}', '\u{2026}'], " ")
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
    use crate::citation::{CallNumber, CitationGrammar};

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
        assert_eq!(period_ranges("1793/1802"), [(1793, 1802)]);
        assert_eq!(period_ranges("BMS 1621...1687"), [(1621, 1687)]);
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

    #[test]
    fn a_call_number_written_otherwise_gives_way_to_period_and_image_count() {
        let candidates = [
            candidate("Bourg (Le)", "9Mi 9_371", "M", "M 1877-1879", 580, 1),
            candidate("Bourg (Le)", "9Mi 9_374", "M", "M 1880-1882", 567, 2),
            candidate("Bourg (Le)", "9Mi 9_377", "M", "M 1880-1882", 600, 3),
        ];
        // The cited call number is on no row, but one register covers the
        // year with the cited image count.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - M - 1881 - 9Mi 9 R374 - vue 289d/567"
            ),
            Selection::Many(102)
        );
        // Without both, or with two that fit, the results.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - M - 1881 - 9Mi 9 R374"
            ),
            Selection::Many(3)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Le Bourg - (aucun) - M - 1878 - 9Mi 9 R374 - vue 3/600"
            ),
            Selection::Many(3)
        );
    }
    /// A register cited by its microfilm's call number, which the register
    /// before it shares, and by its own: the row carrying both, written in a
    /// cell joining them with an internal reference.
    #[test]
    fn several_cited_call_numbers_single_out_the_row_carrying_them_all() {
        let candidates = [
            candidate(
                "Exampleville",
                "4E 9926 / 5Mi 999 BIS [9999999/1]",
                "Naissances, mariages, décès",
                "1793-1805",
                391,
                1,
            ),
            candidate(
                "Exampleville",
                "4E 9927 / 5Mi 999 BIS [9999999/2]",
                "Naissances, mariages, décès",
                "1805-1821",
                429,
                2,
            ),
        ];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1805-1821 - 5MI999BIS - 4E 9927 - 9999999/2 - vue 55d/425"
            ),
            Selection::Many(102)
        );
        // The shared call number alone: the cited period, which the first
        // register only touches.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1805-1821 - 5MI999BIS - vue 55d/425"
            ),
            Selection::Many(102)
        );
        // The year alone: the image count nearest the cited one, within a
        // tenth of it.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1805 - 5MI999BIS - vue 55d/425"
            ),
            Selection::Many(102)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1805 - 5MI999BIS - vue 55d/300"
            ),
            Selection::Many(2)
        );
    }

    /// A commune renamed since the citation was written, its new name
    /// extending the cited one, is the cited one's when its row agrees with
    /// the citation and carries a cited call number or image count.
    #[test]
    fn a_renamed_locality_is_kept_on_the_citation_s_evidence() {
        let candidates = [
            candidate(
                "Exampleville-en-Plaine (Department, France)",
                "4E 9332 / 5Mi 956 BIS [9915874/2]",
                "Naissances, mariages, décès",
                "1832-1851",
                276,
                1,
            ),
            candidate(
                "Exampleville-la-Forêt",
                "4E 9400",
                "Naissances, mariages, décès",
                "1833-1850",
                150,
                2,
            ),
        ];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1832-1851 - 5MI956BIS - 4E 9332 - 9915874/2 - acte 11 - vue 10d/276"
            ),
            Selection::Many(101)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1840 - vue 10d/276"
            ),
            Selection::Many(101)
        );
        // Nothing but the year: either may be another commune.
        assert_eq!(
            chosen(&candidates, "AB12 - Exampleville - (aucun) - NMD - 1840"),
            Selection::Many(0)
        );
        // A row contradicting the citation is not taken.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - NMD - 1860 - vue 10d/276"
            ),
            Selection::Many(0)
        );
        // Not a name extending the cited one at a word.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Example - (aucun) - NMD - 1840 - vue 10d/276"
            ),
            Selection::Many(0)
        );
    }

    /// A commune renamed to a shorter name, or to another the portal shares
    /// with a commune elsewhere: its register is the cited one only on the
    /// citation's evidence; a name the place dictionary knows for the
    /// locality is the locality itself.
    #[test]
    fn a_shortened_or_known_name_stands_for_the_cited_locality() {
        let candidates = [
            candidate("Exampleville", "EXV 1E39", "N", "1903", 201, 1),
            candidate("Exampleville", "EXV 1E38", "N", "1902", 198, 2),
            candidate("Exampleville-la-Forêt", "EXF 1E12", "N", "1903", 201, 3),
        ];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville-sous-Bois - (aucun) - N - 1903 - EXV 1E39 - acte 10 - vue 4d/201"
            ),
            Selection::Many(101)
        );
        // The register renumbered since: its call number decides.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville-sous-Bois - (aucun) - N - 1903 - EXV 1E39 - vue 4d/190"
            ),
            Selection::Many(101)
        );
        // The image count alone: both communes' registers.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville-sous-Bois - (aucun) - N - 1903 - vue 4d/201"
            ),
            Selection::Many(2)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville-sous-Bois - (aucun) - N - 1903"
            ),
            Selection::Many(0)
        );

        // A name the dictionary knows: no further evidence asked.
        let mut citation = CitationParts::parse(
            "AB12 - Ancienville - (aucun) - N - 1903",
            &CitationGrammar::default(),
        )
        .unwrap();
        assert_eq!(
            select(&candidates, &citation, &["Ancienville"]),
            Selection::Many(0)
        );
        citation.alternate_localities = vec!["Exampleville".to_owned()];
        let Selection::One(chosen) = select(&candidates, &citation, &["Ancienville"]) else {
            panic!("one register");
        };
        assert_eq!(chosen.payload, 1);
    }

    /// Registers of half a month, one call number for the year: the cited
    /// period's days, then the image count.
    #[test]
    fn a_period_cited_to_the_day_is_compared_to_the_day() {
        let half = |first: &str, last: &str, images: u16, payload: u8| {
            candidate(
                "Exampleville",
                "4 E 999 45",
                "Décès",
                &format!("{first}/1871 - {last}/1871"),
                images,
                payload,
            )
        };
        let candidates = [
            half("01/01", "15/01", 30, 1),
            half("16/01", "31/01", 30, 2),
            half("01/03", "15/03", 30, 3),
            half("16/03", "31/03", 28, 4),
            half("01/04", "15/04", 30, 5),
        ];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - D - 05/03/1871-28/03/1871 - 4E999/45 - acte 246 - vue 4/30"
            ),
            Selection::Many(103)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - D - 18/03/1871-28/03/1871 - 4E999/45"
            ),
            Selection::Many(104)
        );
        // The year alone tells none apart.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - D - 1871 - 4E999/45"
            ),
            Selection::Many(5)
        );
    }

    #[test]
    fn reads_the_days_of_a_period() {
        let date = |year: u32, month: u32, day: u32| year * 10_000 + month * 100 + day;
        assert_eq!(
            period_spans("05/03/1871 - 28/03/1871"),
            [(date(1871, 3, 5), date(1871, 3, 28))]
        );
        assert_eq!(
            period_spans("26 juillet 1849-1er février 1850"),
            [(date(1849, 7, 26), date(1850, 2, 1))]
        );
        assert_eq!(
            period_spans("juillet 1849"),
            [(date(1849, 7, 1), date(1849, 7, 31))]
        );
        assert_eq!(
            period_spans("1805-1821"),
            [(date(1805, 1, 1), date(1821, 12, 31))]
        );
        assert_eq!(
            period_spans("1683/1750"),
            [(date(1683, 1, 1), date(1750, 12, 31))]
        );
        assert_eq!(
            period_spans("1598-1613 , 1656-1667"),
            [
                (date(1598, 1, 1), date(1613, 12, 31)),
                (date(1656, 1, 1), date(1667, 12, 31))
            ]
        );
    }
    /// A citation of an older digitisation (its microfilm's call number and
    /// view count) whose search returns one register of the originals: no
    /// part but the period agrees, so it is not the cited register.
    #[test]
    fn a_lone_register_contradicting_call_number_and_count_is_not_chosen() {
        let candidates = [candidate(
            "Exampleville",
            "3 E 999 19",
            "Mariages",
            "An XI-1812",
            278,
            1,
        )];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - M - an VI-1815 - 1 MI EC 999/10 - acte 14 - vue 400d/510"
            ),
            Selection::Many(0)
        );
        // Either part agreeing keeps it.
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - M - an VI-1815 - 1 MI EC 999/10 - vue 4/278"
            ),
            Selection::Many(101)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - (aucun) - M - an XI - 3 E 999 19 - vue 4/510"
            ),
            Selection::Many(101)
        );
    }
    /// A reel's suffix (`BIS`, `TER`) names another reel: `5MI999TER` is
    /// neither `5Mi 999` nor `5Mi 999 BIS`.
    #[test]
    fn a_reel_suffix_names_another_register() {
        let candidates = [
            candidate(
                "Exampleville",
                "61E-Dépôt 99 / 1Mi-EC 999",
                "Baptêmes, mariages, sépultures",
                "1711-1720",
                69,
                1,
            ),
            candidate(
                "Exampleville",
                "4E 9995 / 5Mi 999",
                "Baptêmes, mariages, sépultures",
                "1717-1744",
                245,
                2,
            ),
            candidate(
                "Exampleville",
                "4E 9996 / 5Mi 999 TER",
                "Baptêmes, mariages, sépultures",
                "1717-1757",
                394,
                3,
            ),
            candidate(
                "Exampleville",
                "4E 9997 / 5Mi 999 BIS",
                "Baptêmes, mariages, sépultures",
                "1745-1757",
                150,
                4,
            ),
        ];
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - Saint-Exemple - BMS - 1717-1757 - 5MI999TER - vue 28d/394"
            ),
            Selection::Many(103)
        );
        assert_eq!(
            chosen(
                &candidates,
                "AB12 - Exampleville - Saint-Exemple - BMS - 1745 - 5MI999BIS - vue 28d/150"
            ),
            Selection::Many(104)
        );
        // Without the reel on the portal, its period and image count.
        let without = [
            candidates[0].clone(),
            candidates[1].clone(),
            Candidate {
                call_number: Some("4E 9996".to_owned()),
                ..candidates[2].clone()
            },
        ];
        assert_eq!(
            chosen(
                &without,
                "AB12 - Exampleville - Saint-Exemple - BMS - 1717-1757 - 5MI999TER - vue 28d/394"
            ),
            Selection::Many(103)
        );
        assert!(!CallNumber::new("5MI999TER").matches("5Mi 999"));
        assert!(!CallNumber::new("5MI999TER").matches("5Mi 999 BIS"));
        assert!(CallNumber::new("5MI999TER").matches("5Mi 999 TER"));
    }
}
