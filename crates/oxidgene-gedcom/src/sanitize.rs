//! Repairs a GEDCOM text before `ged_io` reads it.
//!
//! `ged_io` rejects a whole file over a single value it cannot model: one
//! `2 AGE majeur` among a hundred thousand lines and nothing is imported. The
//! fix belongs upstream, and is planned there; until it ships, this pass
//! rewrites the few constructs that would otherwise fail or corrupt the
//! import, and reports each one as an import warning naming its line — never
//! the value, which is the person's data.
//!
//! What it repairs:
//!
//! - an `AGE` that would fail the whole file is written in GEDCOM's own form
//!   when it reads as an age (`1y6m`, `child`), and left out otherwise;
//! - a `NOTE @N1@` pointing at a note record (GEDCOM 5.5.1), or an `SNOTE
//!   @N1@` (7.0), which `ged_io` would keep as the literal text `@N1@`, is
//!   replaced by the text of the record it points at;
//! - the several notes of a person, or of one of their or a family's events
//!   and attributes, which `ged_io` would collapse to the last one, are
//!   joined into one with [`NOTE_SEPARATOR`] between them, for the import to
//!   split apart again;
//! - the spaces opening a `CONC` value, which GEDCOM keeps beyond the one
//!   delimiting it but `ged_io` trims — gluing together the words a writer
//!   split between, as earlier OxidGene exports did — move to the end of the
//!   line it continues, where they are read.
//!
//! The pass works one level-0 record at a time and leaves every record it does
//! not need to touch byte for byte as it was. A file with nothing to repair is
//! handed back borrowed, without a copy.

use std::borrow::Cow;
use std::collections::HashMap;

/// What joins the notes of a structure `ged_io` keeps only one note of.
///
/// A line of its own holding a control character no note is written with:
/// [`sanitize`] removes it from the notes it joins, so splitting on it gives
/// back exactly the notes the file held.
pub(crate) const NOTE_SEPARATOR: &str = "\n\u{1}\n";

/// A GEDCOM text ready for `ged_io`, and what had to change to get there.
pub(crate) struct Sanitized<'a> {
    pub text: Cow<'a, str>,
    pub warnings: Vec<String>,
}

/// Rewrites what `ged_io` cannot read; see the module documentation.
pub(crate) fn sanitize(gedcom: &str) -> Sanitized<'_> {
    let notes = note_records(gedcom);
    let mut warnings = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut record: Vec<Line<'_>> = Vec::new();
    for line in lines(gedcom) {
        if line.level == Some(0) && !record.is_empty() {
            if let Some(edit) = repair_record(gedcom, &record, &notes, &mut warnings) {
                edits.push(edit);
            }
            record.clear();
        }
        record.push(line);
    }
    if let Some(edit) = repair_record(gedcom, &record, &notes, &mut warnings) {
        edits.push(edit);
    }

    if edits.is_empty() {
        return Sanitized {
            text: Cow::Borrowed(gedcom),
            warnings,
        };
    }
    let mut text = String::with_capacity(gedcom.len());
    let mut copied = 0;
    for (start, end, replacement) in edits {
        text.push_str(&gedcom[copied..start]);
        text.push_str(&replacement);
        copied = end;
    }
    text.push_str(&gedcom[copied..]);
    Sanitized {
        text: Cow::Owned(text),
        warnings,
    }
}

/// One physical line of the file.
#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    /// One-based, as an editor shows it.
    number: usize,
    /// Byte offset of the line's first character.
    start: usize,
    /// Byte offset of the next line, past this one's terminator.
    next: usize,
    /// `None` for a line that is not `<level> [<xref>] <tag> [<value>]`.
    level: Option<u8>,
    xref: Option<&'a str>,
    tag: &'a str,
    /// What follows the one space delimiting it from the tag.
    value: Option<&'a str>,
}

/// The lines of `text`, ended by LF, CRLF or a lone CR alike — `ged_io`
/// accepts all three.
fn lines(text: &str) -> impl Iterator<Item = Line<'_>> {
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut number = 0;
    std::iter::from_fn(move || {
        if start >= bytes.len() {
            return None;
        }
        let end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
            .map_or(bytes.len(), |offset| start + offset);
        let next = match bytes.get(end) {
            Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => end + 2,
            Some(_) => end + 1,
            None => end,
        };
        number += 1;
        let line = parse_line(&text[start..end], number, start, next);
        start = next;
        Some(line)
    })
}

fn parse_line(content: &str, number: usize, start: usize, next: usize) -> Line<'_> {
    let unparsed = Line {
        number,
        start,
        next,
        level: None,
        xref: None,
        tag: "",
        value: None,
    };
    let rest = content.trim_start_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let Ok(level) = rest[..digits].parse::<u8>() else {
        return unparsed;
    };
    let rest = &rest[digits..];
    if !rest.starts_with([' ', '\t']) {
        return unparsed;
    }
    let mut rest = rest.trim_start_matches([' ', '\t']);
    let mut xref = None;
    if rest.starts_with('@') {
        let xref_len = rest.find([' ', '\t']).unwrap_or(rest.len());
        xref = Some(&rest[..xref_len]);
        rest = rest[xref_len..].trim_start_matches([' ', '\t']);
    }
    let tag_len = rest.find([' ', '\t']).unwrap_or(rest.len());
    if tag_len == 0 {
        return unparsed;
    }
    let (tag, after) = rest.split_at(tag_len);
    let value = after.get(1..);
    Line {
        level: Some(level),
        xref,
        tag,
        value,
        ..unparsed
    }
}

/// The text of every note record (`0 @N1@ NOTE` in 5.5.1, `0 @N1@ SNOTE` in
/// 7.0), by xref, with its continuation lines joined.
///
/// Built only when the file points at a note somewhere: a file that does not
/// has no use for it.
fn note_records(gedcom: &str) -> HashMap<&str, String> {
    let mut records = HashMap::new();
    if !gedcom.contains("NOTE @") {
        return records;
    }
    let mut current: Option<(&str, String)> = None;
    for line in lines(gedcom) {
        match (line.level, line.tag) {
            (Some(0), _) => {
                records.extend(current.take());
                if let ("NOTE" | "SNOTE", Some(xref)) = (line.tag, line.xref) {
                    current = Some((xref, line.value.unwrap_or_default().to_string()));
                }
            }
            (Some(1), "CONT" | "CONC") => {
                if let Some((_, text)) = current.as_mut() {
                    continue_text(text, line);
                }
            }
            _ => {}
        }
    }
    records.extend(current);
    records
}

/// Appends a `CONT` or `CONC` line to the text it continues.
fn continue_text(text: &mut String, line: Line<'_>) {
    if line.tag == "CONT" {
        text.push('\n');
    }
    text.push_str(line.value.unwrap_or_default());
}

/// What happens to one line of a record.
enum Edit {
    /// Left out, with everything beneath it.
    Drop,
    /// Written as this text instead of it and everything beneath it, line
    /// terminators included.
    Replace(String),
    /// Written as this text instead of it alone, terminator included.
    Retext(String),
}

/// The repaired text of one record, as `(start, end, replacement)` over the
/// whole file, or `None` when the record is fine as it is.
fn repair_record(
    gedcom: &str,
    record: &[Line<'_>],
    notes: &HashMap<&str, String>,
    warnings: &mut Vec<String>,
) -> Option<(usize, usize, String)> {
    let (first, last) = (record.first()?, record.last()?);
    let mut edits: Vec<Option<Edit>> = record.iter().map(|_| None).collect();
    repair_ages(gedcom, record, &mut edits, warnings);
    rewrite_notes(record, notes, &mut edits, warnings);
    shift_conc_spaces(gedcom, record, &mut edits);
    if edits.iter().all(Option::is_none) {
        return None;
    }

    let mut text = String::with_capacity(last.next - first.start);
    let mut skip_below: Option<u8> = None;
    for (line, edit) in record.iter().zip(&edits) {
        if let Some(level) = skip_below {
            if line.level.is_none_or(|l| l > level) {
                continue;
            }
            skip_below = None;
        }
        match edit {
            None => text.push_str(&gedcom[line.start..line.next]),
            Some(Edit::Drop) => skip_below = line.level,
            Some(Edit::Replace(replacement)) => {
                text.push_str(replacement);
                skip_below = line.level;
            }
            Some(Edit::Retext(replacement)) => text.push_str(replacement),
        }
    }
    Some((first.start, last.next, text))
}

/// Moves the spaces opening each `CONC` value to the end of the line it
/// continues, where `ged_io` reads them rather than trims them.
///
/// Only when that line is the one just before, at the level a `CONC`
/// continues, and holds a value of its own — trailing spaces after a bare tag
/// would be trimmed all the same.
fn shift_conc_spaces(gedcom: &str, record: &[Line<'_>], edits: &mut [Option<Edit>]) {
    let mut stripped = vec![0usize; record.len()];
    let mut appended: Vec<&str> = vec![""; record.len()];
    for index in 1..record.len() {
        let (previous, line) = (record[index - 1], record[index]);
        let (Some(level), Some(value), Some(previous_level)) =
            (line.level, line.value, previous.level)
        else {
            continue;
        };
        let spaces = value.len() - value.trim_start_matches([' ', '\t']).len();
        let continued = previous_level.saturating_add(1) == level
            || (previous_level == level && matches!(previous.tag, "CONT" | "CONC"));
        let previous_value = previous.value.unwrap_or_default();
        if line.tag != "CONC" || spaces == 0 || !continued {
            continue;
        }
        if previous_value.len() <= stripped[index - 1] {
            continue;
        }
        stripped[index] = spaces;
        appended[index - 1] = &value[..spaces];
    }
    for (index, line) in record.iter().enumerate() {
        if edits[index].is_some() || (stripped[index] == 0 && appended[index].is_empty()) {
            continue;
        }
        let raw = &gedcom[line.start..line.next];
        let content = raw.trim_end_matches(['\r', '\n']);
        let value = line.value.unwrap_or_default();
        let prefix = &content[..content.len() - value.len()];
        edits[index] = Some(Edit::Retext(format!(
            "{prefix}{}{}{}",
            &value[stripped[index]..],
            appended[index],
            &raw[content.len()..]
        )));
    }
}

/// Repairs every `AGE` `ged_io` would fail the file over: one OxidGene reads
/// as an age (`1y6m`, `child`) is rewritten in GEDCOM's own form, which
/// loses nothing; anything else is left out with a warning.
fn repair_ages(
    gedcom: &str,
    record: &[Line<'_>],
    edits: &mut [Option<Edit>],
    warnings: &mut Vec<String>,
) {
    for (line, edit) in record.iter().zip(edits) {
        if line.tag != "AGE" || line.level.is_none() || is_readable_age(line.value) {
            continue;
        }
        let canonical = line
            .value
            .and_then(|value| value.parse::<oxidgene_core::types::AgeAtEvent>().ok());
        match canonical {
            Some(age) => {
                let raw = &gedcom[line.start..line.next];
                let content = raw.trim_end_matches(['\r', '\n']);
                let value = line.value.unwrap_or_default();
                let prefix = &content[..content.len() - value.len()];
                *edit = Some(Edit::Retext(format!(
                    "{prefix}{age}{}",
                    &raw[content.len()..]
                )));
            }
            None => {
                *edit = Some(Edit::Drop);
                warnings.push(format!(
                    "Line {}: an AGE that is not a GEDCOM age was left out",
                    line.number
                ));
            }
        }
    }
}

/// Rewrites the notes `ged_io` would misread: a pointer becomes the text of
/// the record it points at, and the notes of a structure that keeps only one
/// are joined into one.
fn rewrite_notes(
    record: &[Line<'_>],
    notes: &HashMap<&str, String>,
    edits: &mut [Option<Edit>],
    warnings: &mut Vec<String>,
) {
    for (parent, group) in note_groups(record) {
        if group.len() > 1 && keeps_one_note(record, parent) {
            merge_notes(record, &group, notes, edits, warnings);
            continue;
        }
        for index in group {
            let (line, level) = (record[index], record[index].level.unwrap_or(1));
            if pointer(line.value).is_some() {
                edits[index] = Some(match note_text(record, index, notes, warnings) {
                    Some(text) => Edit::Replace(note_lines(level, &text)),
                    None => Edit::Drop,
                });
            }
        }
    }
}

/// The `NOTE` and `SNOTE` lines of a record below its first line, grouped by
/// the line they hang off, in file order.
fn note_groups(record: &[Line<'_>]) -> Vec<(usize, Vec<usize>)> {
    let mut open: Vec<(u8, usize)> = Vec::new();
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for (index, line) in record.iter().enumerate() {
        let Some(level) = line.level else {
            continue;
        };
        while open
            .last()
            .is_some_and(|&(open_level, _)| open_level >= level)
        {
            open.pop();
        }
        if let (true, Some(&(_, parent))) = (matches!(line.tag, "NOTE" | "SNOTE"), open.last()) {
            match groups.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, group)) => group.push(index),
                None => groups.push((parent, vec![index])),
            }
        }
        open.push((level, index));
    }
    groups
}

/// Whether `ged_io` keeps a single note of the structure at `parent`: a
/// person, and the events and attributes of a person or a family.
///
/// A family, a source and a media record keep every note already. An inline
/// `OBJE` is left alone too: its note is the media's description, not a
/// note.
fn keeps_one_note(record: &[Line<'_>], parent: usize) -> bool {
    let record_tag = record.first().map_or("", |line| line.tag);
    match record[parent].level {
        Some(0) => record_tag == "INDI",
        Some(1) => matches!(record_tag, "INDI" | "FAM") && record[parent].tag != "OBJE",
        _ => false,
    }
}

/// Joins the notes of one structure into the first of them.
fn merge_notes(
    record: &[Line<'_>],
    group: &[usize],
    notes: &HashMap<&str, String>,
    edits: &mut [Option<Edit>],
    warnings: &mut Vec<String>,
) {
    let texts: Vec<String> = group
        .iter()
        .filter_map(|&index| note_text(record, index, notes, warnings))
        .map(|text| text.replace('\u{1}', ""))
        .collect();
    let level = record[group[0]].level.unwrap_or(1);
    edits[group[0]] = Some(if texts.is_empty() {
        Edit::Drop
    } else {
        Edit::Replace(note_lines(level, &texts.join(NOTE_SEPARATOR)))
    });
    for &index in &group[1..] {
        edits[index] = Some(Edit::Drop);
    }
}

/// The text of the note at `index`: its own, continuation lines included, or
/// that of the record it points at — `None`, with a warning, when the file
/// has no such record.
fn note_text(
    record: &[Line<'_>],
    index: usize,
    notes: &HashMap<&str, String>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let line = record[index];
    if let Some(xref) = pointer(line.value) {
        let text = notes.get(xref).cloned();
        if text.is_none() {
            warnings.push(format!(
                "Line {}: a NOTE points at {xref}, which the file does not hold; it was left out",
                line.number
            ));
        }
        return text;
    }
    let level = line.level.unwrap_or(0);
    let mut text = line.value.unwrap_or_default().to_string();
    for next in record[index + 1..]
        .iter()
        .take_while(|next| next.level.is_none_or(|l| l > level))
    {
        if next.level == Some(level.saturating_add(1)) && matches!(next.tag, "CONT" | "CONC") {
            continue_text(&mut text, *next);
        }
    }
    Some(text)
}

/// The xref a value is, if it is nothing but one.
fn pointer(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    let inner = value.strip_prefix('@')?.strip_suffix('@')?;
    (!inner.is_empty() && !inner.contains(['@', ' ', '\t'])).then_some(value)
}

/// A `NOTE` holding `text` at `level`, one `CONT` per further line.
fn note_lines(level: u8, text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for (index, line) in text.split('\n').enumerate() {
        if index == 0 {
            out.push_str(&format!("{level} NOTE"));
        } else {
            out.push_str(&format!("{} CONT", level.saturating_add(1)));
        }
        if !line.is_empty() {
            out.push(' ');
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// Whether `ged_io` 0.16 reads this `AGE` value rather than failing the whole
/// file over it.
///
/// Mirrors its parser exactly, including what it tolerates: a token with an
/// unknown unit is ignored, and a later token overwrites an earlier one for
/// the same unit. A non-ASCII value is refused outright, as `ged_io` splits
/// each token at its last byte and would panic on a multi-byte character.
fn is_readable_age(value: Option<&str>) -> bool {
    let value = value
        .unwrap_or("")
        .trim_start_matches(|c: char| c == '\u{feff}' || c.is_whitespace());
    if !value.is_ascii() {
        return false;
    }
    if matches!(value, "CHILD" | "INFANT" | "STILLBORN") {
        return true;
    }
    let remaining = match value.strip_prefix(['<', '>']) {
        Some(rest) => rest.trim_start(),
        None => value,
    };
    let (mut years, mut months, mut weeks, mut days) = (None, None, None, None);
    for token in remaining.split_whitespace() {
        let (number, unit) = token.split_at(token.len() - 1);
        match unit {
            "y" => years = number.parse::<u16>().ok(),
            "m" => months = number.parse::<u8>().ok().map(u16::from),
            "w" => weeks = number.parse::<u8>().ok().map(u16::from),
            "d" => days = number.parse::<u8>().ok().map(u16::from),
            _ if token.bytes().all(|b| b.is_ascii_digit()) => years = token.parse::<u16>().ok(),
            _ => {}
        }
    }
    years.is_some() || months.is_some() || weeks.is_some() || days.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_nothing_to_repair_is_handed_back_as_it_was() {
        let gedcom = "0 HEAD\r\n0 @I1@ INDI\r\n1 BIRT\r\n2 AGE 30y\r\n0 TRLR";
        let sanitized = sanitize(gedcom);
        assert!(matches!(sanitized.text, Cow::Borrowed(_)));
        assert!(sanitized.warnings.is_empty());
    }

    #[test]
    fn the_ages_ged_io_reads_are_kept() {
        for age in [
            "30y",
            "< 8y",
            ">1y 2m",
            "6m 2w 3d",
            "CHILD",
            "INFANT",
            "STILLBORN",
            "45",
        ] {
            assert!(is_readable_age(Some(age)), "{age}");
        }
    }

    #[test]
    fn the_ages_ged_io_rejects_are_recognised() {
        for age in [
            "",
            "majeur",
            "adult",
            "1y6m",
            "about thirty",
            "CHILD ",
            "30 ans é",
            "999m",
        ] {
            assert!(!is_readable_age(Some(age)), "{age}");
        }
        assert!(!is_readable_age(None));
    }

    /// An age `ged_io` refuses but OxidGene reads is written in GEDCOM's form
    /// rather than lost, without a warning.
    #[test]
    fn an_age_spelled_loosely_is_rewritten_rather_than_dropped() {
        let gedcom =
            "0 HEAD\r\n0 @I1@ INDI\r\n1 DEAT\r\n2 AGE 1y6m\r\n1 BURI\r\n2 AGE child\r\n0 TRLR";
        let sanitized = sanitize(gedcom);
        assert_eq!(
            sanitized.text,
            "0 HEAD\r\n0 @I1@ INDI\r\n1 DEAT\r\n2 AGE 1y 6m\r\n1 BURI\r\n2 AGE CHILD\r\n0 TRLR"
        );
        assert!(sanitized.warnings.is_empty(), "{:?}", sanitized.warnings);
    }

    #[test]
    fn an_unreadable_age_and_what_hangs_off_it_are_dropped_with_a_warning() {
        let gedcom =
            "0 HEAD\n0 @I1@ INDI\n1 DEAT\n2 AGE majeur\n3 PHRASE x\n2 PLAC Somewhere\n0 TRLR\n";
        let sanitized = sanitize(gedcom);
        assert_eq!(
            sanitized.text,
            "0 HEAD\n0 @I1@ INDI\n1 DEAT\n2 PLAC Somewhere\n0 TRLR\n"
        );
        assert_eq!(sanitized.warnings.len(), 1);
        assert!(sanitized.warnings[0].starts_with("Line 4:"));
        assert!(!sanitized.warnings[0].contains("majeur"));
    }

    #[test]
    fn a_note_pointer_is_replaced_by_the_record_it_points_at() {
        let gedcom = "0 HEAD\n0 @I1@ INDI\n1 NOTE @N1@\n1 BIRT\n2 SNOTE @N2@\n\
                      0 @N1@ NOTE First line\n1 CONT second\n1 CONC  half\n\
                      0 @N2@ SNOTE Shared\n0 TRLR\n";
        let sanitized = sanitize(gedcom);
        assert_eq!(
            sanitized.text,
            "0 HEAD\n0 @I1@ INDI\n1 NOTE First line\n2 CONT second half\n1 BIRT\n\
             2 NOTE Shared\n0 @N1@ NOTE First line\n1 CONT second \n1 CONC half\n\
             0 @N2@ SNOTE Shared\n0 TRLR\n"
        );
        assert!(sanitized.warnings.is_empty());
    }

    #[test]
    fn a_pointer_to_a_note_the_file_lacks_is_dropped_with_a_warning() {
        let gedcom = "0 HEAD\n0 @I1@ INDI\n1 NOTE @N9@\n1 NOTE Kept\n0 TRLR\n";
        let sanitized = sanitize(gedcom);
        assert_eq!(sanitized.text, "0 HEAD\n0 @I1@ INDI\n1 NOTE Kept\n0 TRLR\n");
        assert_eq!(sanitized.warnings.len(), 1);
        assert!(sanitized.warnings[0].starts_with("Line 3:"));
    }

    #[test]
    fn the_notes_of_a_person_or_an_event_are_joined_into_one() {
        let gedcom = "0 @I1@ INDI\n1 NOTE First\n2 CONC  half\n1 BIRT\n2 NOTE A\n\
                      2 SOUR @S1@\n2 NOTE @N1@\n1 NOTE Second\n2 CONT line\n\
                      0 @N1@ NOTE B\n";
        let sanitized = sanitize(gedcom);
        assert_eq!(
            sanitized.text,
            "0 @I1@ INDI\n1 NOTE First half\n2 CONT \u{1}\n2 CONT Second\n2 CONT line\n\
             1 BIRT\n2 NOTE A\n3 CONT \u{1}\n3 CONT B\n2 SOUR @S1@\n0 @N1@ NOTE B\n"
        );
    }

    #[test]
    fn the_spaces_opening_a_conc_value_move_to_the_line_it_continues() {
        let gedcom = "0 @I1@ INDI\r\n1 NOTE abc\r\n2 CONC  def\r\n2 CONC   ghi\r\n\
                      2 CONT jkl\r\n2 CONC \tmno\r\n0 @S1@ SOUR\n1 TITL A\n2 CONC  B\n\
                      1 NOTE\n2 CONC  bare\n";
        let sanitized = sanitize(gedcom);
        assert_eq!(
            sanitized.text,
            "0 @I1@ INDI\r\n1 NOTE abc \r\n2 CONC def  \r\n2 CONC ghi\r\n\
             2 CONT jkl\t\r\n2 CONC mno\r\n0 @S1@ SOUR\n1 TITL A \n2 CONC B\n\
             1 NOTE\n2 CONC  bare\n"
        );
    }

    #[test]
    fn a_family_a_source_and_a_media_keep_their_notes_apart() {
        let gedcom = "0 @F1@ FAM\n1 NOTE A\n1 NOTE B\n0 @S1@ SOUR\n1 NOTE A\n1 NOTE B\n\
                      0 @I1@ INDI\n1 OBJE\n2 FILE x.jpg\n2 NOTE A\n2 NOTE B\n";
        let sanitized = sanitize(gedcom);
        assert!(matches!(sanitized.text, Cow::Borrowed(_)));
    }
}
