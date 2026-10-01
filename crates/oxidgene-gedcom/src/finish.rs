//! Finishes the GEDCOM text `ged_io` writes.
//!
//! `ged_io` 0.16's model holds less than GEDCOM allows: a person, an event or
//! an attribute has room for one `NOTE`, where GEDCOM 5.5.1 gives each of
//! them any number. Until the upstream fix ships, the export hands the model
//! a placeholder note for each record that has notes, and this pass writes
//! every one of that record's notes in its place.

use ged_io::types::note::Note as GedNote;
use uuid::Uuid;

/// Opens the value of a placeholder note; the owner's id follows.
const NOTE_SLOT: char = '\u{1}';

/// The `NOTE` the model carries for every note of `owner`, when it has any.
pub(crate) fn note_slot(owner: Uuid, has_notes: bool) -> Option<GedNote> {
    has_notes.then(|| GedNote {
        value: Some(format!("{NOTE_SLOT}{owner}")),
        ..Default::default()
    })
}

/// Writes the notes `notes_of` gives for each placeholder in its place.
pub(crate) fn finish<'n>(gedcom: &str, notes_of: impl Fn(Uuid) -> Vec<&'n str>) -> String {
    let mut out = String::with_capacity(gedcom.len());
    for line in gedcom.lines() {
        match note_slot_of(line) {
            Some((level, owner)) => {
                for text in notes_of(owner) {
                    push_text(&mut out, level, "NOTE", text);
                }
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// The level and owner of a placeholder note line.
fn note_slot_of(line: &str) -> Option<(u8, Uuid)> {
    let (level, rest) = line.split_once(' ')?;
    let owner = rest.strip_prefix("NOTE ")?.strip_prefix(NOTE_SLOT)?;
    Some((level.parse().ok()?, Uuid::parse_str(owner).ok()?))
}

/// `text` under `tag` at `level`, one `CONT` per further line, each line
/// continued on `CONC` lines when too long for one.
///
/// A carriage return is a line break like any other: written as such inside a
/// value, a reader would take it for the end of the line.
pub(crate) fn push_text(out: &mut String, level: u8, tag: &str, text: &str) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let next = level.saturating_add(1);
    for (index, line) in text.split('\n').enumerate() {
        let prefix = if index == 0 {
            format!("{level} {tag}")
        } else {
            format!("{next} CONT")
        };
        push_wrapped(out, &prefix, line, next);
    }
}

/// The longest line GEDCOM 5.5.1 allows, terminator excluded.
const MAX_LINE: usize = 255;

/// Writes `{prefix} {value}`, continuing a value too long for one line on
/// `CONC` lines at `conc_level`.
///
/// A value is never split beside a space. GEDCOM delimits a `CONC` value by
/// one space and keeps any further one, but readers trim it — `ged_io` among
/// them — and would glue the words on either side together. A value with
/// nowhere else to split is rather left whole on one long line.
fn push_wrapped(out: &mut String, prefix: &str, value: &str, conc_level: u8) {
    let conc = format!("{conc_level} CONC");
    let (mut head, mut rest) = split_value(value, MAX_LINE.saturating_sub(prefix.len() + 1));
    push_line(out, prefix, head);
    while !rest.is_empty() {
        (head, rest) = split_value(rest, MAX_LINE.saturating_sub(conc.len() + 1));
        push_line(out, &conc, head);
    }
}

fn push_line(out: &mut String, prefix: &str, value: &str) {
    out.push_str(prefix);
    if !value.is_empty() {
        out.push(' ');
        out.push_str(value);
    }
    out.push('\n');
}

/// Splits `value` at the last point within `budget` bytes with no space on
/// either side, else at the last one with none after it; leaves it whole
/// when there is neither.
fn split_value(value: &str, budget: usize) -> (&str, &str) {
    if value.len() <= budget {
        return (value, "");
    }
    let mut at = budget;
    while !value.is_char_boundary(at) {
        at -= 1;
    }
    let mut fallback = None;
    while at > 0 {
        let before = value[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        let after = value[at..].chars().next().is_some_and(char::is_whitespace);
        match (before, after) {
            (false, false) => return value.split_at(at),
            (true, false) if fallback.is_none() => fallback = Some(at),
            _ => {}
        }
        at = value[..at].char_indices().next_back().map_or(0, |(i, _)| i);
    }
    fallback.map_or((value, ""), |at| value.split_at(at))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a reader trimming every value makes of written lines.
    fn read_back(written: &str) -> String {
        let mut text = String::new();
        for line in written.lines() {
            let mut parts = line.splitn(3, ' ');
            let (_, tag, value) = (parts.next(), parts.next(), parts.next());
            let value = value.unwrap_or_default().trim_start();
            match tag {
                Some("CONT") => {
                    text.push('\n');
                    text.push_str(value);
                }
                _ => text.push_str(value),
            }
        }
        text
    }

    #[test]
    fn a_long_note_is_continued_without_splitting_beside_a_space() {
        for text in [
            format!("{} tail", "x".repeat(300)),
            format!("{}é{}", "word ".repeat(60), "y".repeat(400)),
            "a b ".repeat(200),
        ] {
            let mut out = String::new();
            push_text(&mut out, 1, "NOTE", &text);
            assert!(out.lines().all(|line| line.len() <= MAX_LINE), "{out}");
            assert!(out.contains("2 CONC "));
            assert_eq!(read_back(&out), text);
        }
    }

    #[test]
    fn a_value_with_nowhere_safe_to_split_stays_whole() {
        let text = format!("x{}y", " ".repeat(300));
        let mut out = String::new();
        push_text(&mut out, 1, "NOTE", &text);
        assert_eq!(out, format!("1 NOTE {text}\n"));
    }

    #[test]
    fn line_breaks_become_cont_lines_whatever_their_kind() {
        let mut out = String::new();
        push_text(&mut out, 2, "NOTE", "one\r\ntwo\rthree\n\nfive");
        assert_eq!(
            out,
            "2 NOTE one\n3 CONT two\n3 CONT three\n3 CONT\n3 CONT five\n"
        );
    }
}
