//! Choosing the cited register among a search's rows.
//!
//! The engines match the locality as text — `Bourg (Le)` also finds
//! `Saint-Exemple-lès-le-Bourg` — so rows are first kept by their locality cell,
//! folded. The citation's other parts then narrow them, in order: call
//! number, act kind, parish, period, image count. Selection stops at the
//! first criterion that leaves exactly one row; a criterion the citation
//! lacks is skipped, and so is one that would leave none, since the portal
//! may write a parish or a period differently. The call number is the
//! exception: a cited call number that no row carries means the register is
//! not among them, and the answer is the results rather than a guess.

use oxidgene_core::search::fold_words;

use super::Settings;
use super::page::Row;
use crate::citation::{Act, CitationParts, republican_numeral, republican_start};

/// The outcome of a selection.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Selection<'r> {
    One(&'r Row),
    /// No row, or several rows the citation cannot tell apart.
    Many(usize),
}

/// One narrowing test of the rows.
type Criterion<'c> = Box<dyn Fn(&Row) -> bool + 'c>;

pub(super) fn register<'r>(
    rows: &'r [Row],
    citation: &CitationParts,
    settings: &Settings,
) -> Selection<'r> {
    let cells = &settings.cells;
    let wanted = [
        fold_words(&settings.locality(citation)),
        fold_words(&citation.locality),
    ];
    let mut candidates: Vec<&Row> = rows
        .iter()
        .filter(|row| {
            row.cell(&cells.locality)
                .is_some_and(|locality| wanted.contains(&fold_words(locality)))
        })
        .collect();
    if let [only] = candidates.as_slice() {
        return Selection::One(only);
    }
    if candidates.is_empty() {
        return Selection::Many(0);
    }

    if let Some(call_number) = &citation.call_number {
        let matching: Vec<&Row> = candidates
            .iter()
            .copied()
            .filter(|row| call_number.matches(&row.call_number))
            .collect();
        match matching.as_slice() {
            [] => return Selection::Many(candidates.len()),
            [only] => return Selection::One(only),
            _ => candidates = matching,
        }
    }

    let act_cell = cells.act.as_deref();
    let criteria: [Option<Criterion<'_>>; 4] = [
        act_cell.map(|cell| {
            Box::new(move |row: &Row| holds(row.cell(cell), &citation.act)) as Criterion<'_>
        }),
        cells
            .parish
            .as_deref()
            .zip(citation.parish.as_deref())
            .map(|(cell, parish)| {
                let parish = fold_words(parish);
                Box::new(move |row: &Row| {
                    row.cell(cell)
                        .is_some_and(|text| fold_words(text) == parish)
                }) as Criterion<'_>
            }),
        cells
            .period
            .as_deref()
            .zip(citation.year)
            .map(|(cell, year)| {
                Box::new(move |row: &Row| row.cell(cell).is_some_and(|text| covers(text, year)))
                    as Criterion<'_>
            }),
        citation
            .view_count
            .map(|count| Box::new(move |row: &Row| row.images == Some(count)) as Criterion<'_>),
    ];
    for criterion in criteria.iter().flatten() {
        let matching: Vec<&Row> = candidates
            .iter()
            .copied()
            .filter(|row| criterion(row))
            .collect();
        match matching.as_slice() {
            [] => {}
            [only] => return Selection::One(only),
            _ => candidates = matching,
        }
    }
    Selection::Many(candidates.len())
}

/// Whether a row's act cell admits the cited act. A cell written as act
/// codes (`BMS`, `NMD`, `TD`) must hold every cited kind, or be the cited
/// table; a cell in words (`Baptêmes, mariages et sépultures`) or with a
/// period (`N 1903 - 1912`) is left to the engine's act filter.
fn holds(cell: Option<&str>, act: &Act) -> bool {
    let Some(written) = cell.and_then(|text| Act::from_code(text.trim())) else {
        return true;
    };
    match act {
        Act::Register(kinds) => kinds.iter().all(|kind| written.kinds().contains(kind)),
        Act::Table(_) => &written == act,
    }
}

/// Whether a period cell covers `year`. The text may hold several segments
/// with act codes and notes between them — `1598-1613 , 1656-1667`,
/// `NMD 1857-1859, N 1853-1872`, `N 1903 - 1912`, `NM an II` — so every year
/// or range in it is read.
pub(super) fn covers(text: &str, year: u16) -> bool {
    ranges(text)
        .into_iter()
        .any(|(first, last)| (first..=last).contains(&year))
}

/// A year written in a period cell: its byte span and the Gregorian years
/// it covers (a Republican year spans two).
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

/// The year ranges of a period cell: two years joined by a dash form one.
fn ranges(text: &str) -> Vec<(u16, u16)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_segment_of_a_period() {
        assert_eq!(ranges("1658-1668"), [(1658, 1668)]);
        assert_eq!(
            ranges("1598-1613 , 1656-1667"),
            [(1598, 1613), (1656, 1667)]
        );
        assert_eq!(ranges("1700-1701 (janvier)"), [(1700, 1701)]);
        assert_eq!(
            ranges("NMD 1857-1859, N 1853-1872, MD 1853-1856,1860-1872"),
            [(1857, 1859), (1853, 1872), (1853, 1856), (1860, 1872)]
        );
        assert_eq!(ranges("N 1903 - 1912"), [(1903, 1912)]);
        assert_eq!(ranges("NM an II"), [(1793, 1794)]);
        assert_eq!(ranges("1792-an II"), [(1792, 1794)]);
        assert_eq!(
            ranges("BMS 1595-1692 (consulter le détail)"),
            [(1595, 1692)]
        );
        assert_eq!(ranges("acte 12345, Mansan III"), []);

        assert!(!covers("1598-1613 , 1656-1667", 1650));
        assert!(covers("1613-1655", 1650));
    }

    #[test]
    fn reads_act_cells_written_as_codes_only() {
        let birth = Act::from_code("N").unwrap();
        assert!(holds(Some("NMD"), &birth));
        assert!(!holds(Some("BMS"), &birth));
        assert!(holds(Some("Naissances"), &birth));
        assert!(holds(Some("N 1903 - 1912"), &birth));
        assert!(holds(None, &birth));
        let table = Act::from_code("TD").unwrap();
        assert!(holds(Some("TD"), &table));
        assert!(!holds(Some("NMD"), &table));
    }
}
