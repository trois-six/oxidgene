//! Finishes the GEDCOM text `ged_io` writes.
//!
//! `ged_io` 0.16's model holds less than GEDCOM allows: a person, an event or
//! an attribute has room for one `NOTE`, where GEDCOM 5.5.1 gives each of
//! them any number. Until the upstream fix ships, the export hands the model
//! a placeholder note for each record that has notes, and this pass writes
//! every one of that record's notes in its place.
//!
//! It also continues the long lines. `ged_io` splits a value at its 255th
//! byte wherever that falls, and when it falls beside a space the space is
//! lost to every reader that trims a `CONC` value, `ged_io` included. The
//! export asks `ged_io` not to split at all, and this pass splits each line a
//! reader continues — notes, texts, causes, pages and a source's title,
//! author, publication and abbreviation — between two non-space characters.
//!
//! And it writes what `ged_io` drops from a record it does write: a source's
//! `PUBL`, which its writer leaves out, goes in as an [`Addition`] after the
//! line opening the record.

use std::collections::HashMap;

use ged_io::types::note::Note as GedNote;
use uuid::Uuid;

/// A level-1 structure to write right after the line opening a record.
pub(crate) struct Addition {
    pub tag: &'static str,
    pub text: String,
}

/// Opens the value of a placeholder note; the owner's id follows.
const NOTE_SLOT: char = '\u{1}';

/// The `NOTE` the model carries for every note of `owner`, when it has any.
pub(crate) fn note_slot(owner: Uuid, has_notes: bool) -> Option<GedNote> {
    has_notes.then(|| GedNote {
        value: Some(format!("{NOTE_SLOT}{owner}")),
        ..Default::default()
    })
}

/// Writes the notes `notes_of` gives for each placeholder in its place, the
/// `additions` of each record by its xref, and continues every line too long
/// for GEDCOM that a reader continues.
pub(crate) fn finish<'n>(
    gedcom: &str,
    notes_of: impl Fn(Uuid) -> Vec<&'n str>,
    additions: &HashMap<String, Vec<Addition>>,
) -> String {
    let mut out = String::with_capacity(gedcom.len() + gedcom.len() / 64);
    let mut record = "";
    for line in gedcom.lines() {
        if let Some((level, owner)) = note_slot_of(line) {
            for text in notes_of(owner) {
                push_text(&mut out, level, "NOTE", text);
            }
            continue;
        }
        let (level, xref, tag, value) = split_line(line);
        if level == Some(0) {
            record = tag;
            push_line(&mut out, line, "");
            for addition in xref
                .and_then(|xref| additions.get(xref))
                .into_iter()
                .flatten()
            {
                push_text(&mut out, 1, addition.tag, &addition.text);
            }
            continue;
        }
        match (level, value) {
            (Some(level), Some(value)) if continued(level, tag, record) => {
                let prefix = &line[..line.len() - value.len() - 1];
                let conc_level = if matches!(tag, "CONT" | "CONC") {
                    level
                } else {
                    level.saturating_add(1)
                };
                push_wrapped(&mut out, prefix, value, conc_level);
            }
            _ => push_line(&mut out, line, ""),
        }
    }
    out
}

/// The level, xref, tag and value of a line as `ged_io` writes it.
fn split_line(line: &str) -> (Option<u8>, Option<&str>, &str, Option<&str>) {
    let Some((level, rest)) = line.split_once(' ') else {
        return (None, None, "", None);
    };
    let (xref, rest) = if rest.starts_with('@') {
        match rest.split_once(' ') {
            Some((xref, tag)) => (Some(xref), tag),
            None => (Some(rest), ""),
        }
    } else {
        (None, rest)
    };
    let (tag, value) = match rest.split_once(' ') {
        Some((tag, value)) => (tag, Some(value)),
        None => (rest, None),
    };
    (level.parse().ok(), xref, tag, value)
}

/// Whether a reader joins the `CONC` lines following a `tag` line — and so
/// whether it may be continued on them. Elsewhere a long value stays whole on
/// its line: `ged_io` reads a long line, but not a continuation it does not
/// expect.
fn continued(level: u8, tag: &str, record: &str) -> bool {
    match tag {
        "NOTE" | "CONT" | "CONC" | "TEXT" | "CAUS" | "PAGE" => true,
        "TITL" | "AUTH" | "PUBL" | "ABBR" => level == 1 && record == "SOUR",
        _ => false,
    }
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
    fn only_the_lines_a_reader_continues_are_continued() {
        let long = format!("{} end", "z".repeat(300));
        let gedcom = format!(
            "0 @S1@ SOUR\n1 TITL {long}\n1 NOTE {long}\n2 CONT {long}\n\
             0 @I1@ INDI\n1 TITL {long}\n1 DEAT\n2 CAUS {long}\n2 PLAC {long}\n0 TRLR"
        );
        let out = finish(&gedcom, |_| Vec::new(), &HashMap::new());
        let continued: Vec<&str> = out
            .lines()
            .filter(|line| line.contains(" CONC "))
            .map(|line| line.split(' ').next().unwrap_or_default())
            .collect();
        assert_eq!(continued, ["2", "2", "2", "3"], "{out}");
        assert!(out.contains(&format!("1 TITL {long}\n1 DEAT")));
        assert!(out.contains(&format!("2 PLAC {long}\n0 TRLR\n")));
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
