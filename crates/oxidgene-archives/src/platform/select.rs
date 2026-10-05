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
//!    parish, period, image count, stopping at the first that leaves exactly
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
    let wanted: Vec<String> = localities.iter().map(|locality| fold(locality)).collect();
    let mut kept: Vec<&Candidate<T>> = candidates
        .iter()
        .filter(|candidate| {
            candidate
                .locality
                .as_deref()
                .is_some_and(|locality| wanted.contains(&fold(locality)))
        })
        .collect();
    if let [only] = kept.as_slice() {
        return Selection::One(only);
    }
    if kept.is_empty() {
        return Selection::Many(0);
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
        match matching.as_slice() {
            [] => return Selection::Many(kept.len()),
            [only] => return Selection::One(only),
            _ => kept = matching,
        }
    }

    for criterion in criteria(citation).iter().flatten() {
        let matching: Vec<&Candidate<T>> = kept
            .iter()
            .copied()
            .filter(|candidate| criterion(candidate))
            .collect();
        match matching.as_slice() {
            [] => {}
            [only] => return Selection::One(only),
            _ => kept = matching,
        }
    }
    Selection::Many(kept.len())
}

/// The criteria after the call number, in order; `None` for a part the
/// citation lacks.
fn criteria<'c, T>(citation: &'c CitationParts) -> [Option<Criterion<'c, T>>; 4] {
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
        citation.view_count.map(|count| {
            Box::new(move |candidate: &Candidate<T>| candidate.images == Some(count))
                as Criterion<'c, T>
        }),
    ]
}

/// Whether a displayed act admits the cited act. Acts written as codes
/// (`BMS`, `NMD`, `TD`) must hold every cited kind, or be the cited table;
/// acts in words (`Baptêmes, mariages et sépultures`), with a period
/// (`N 1903 - 1912`), or not displayed are left to the portal's own filter.
pub(crate) fn holds_act(displayed: Option<&str>, act: &Act) -> bool {
    let Some(written) = displayed.and_then(|text| Act::from_code(text.trim())) else {
        return true;
    };
    match act {
        Act::Register(kinds) => kinds.iter().all(|kind| written.kinds().contains(kind)),
        Act::Table(_) => &written == act,
    }
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
/// two of them joined by a dash forming one range.
pub(crate) fn period_ranges(text: &str) -> Vec<(u16, u16)> {
    let tokens = year_tokens(text);
    let mut ranges = Vec::new();
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let mut last = token.last;
        if let Some(next) = tokens.get(index + 1)
            && text[token.end..next.start].trim() == "-"
        {
            last = next.last;
            index += 1;
        }
        ranges.push((token.first, last.max(token.first)));
        index += 1;
    }
    ranges
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
