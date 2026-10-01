//! Repairs a GEDCOM text before `ged_io` reads it.
//!
//! `ged_io` rejects a whole file over a single value it cannot model: one
//! `2 AGE majeur` among a hundred thousand lines and nothing is imported. The
//! fix belongs upstream, and is planned there; until it ships, this pass
//! rewrites the few constructs that would otherwise fail or corrupt the
//! import, and reports each one as an import warning naming its line — never
//! the value, which is the person's data.
//!
//! The pass works one level-0 record at a time and leaves every record it does
//! not need to touch byte for byte as it was. A file with nothing to repair is
//! handed back borrowed, without a copy.

use std::borrow::Cow;

/// A GEDCOM text ready for `ged_io`, and what had to change to get there.
pub(crate) struct Sanitized<'a> {
    pub text: Cow<'a, str>,
    pub warnings: Vec<String>,
}

/// Rewrites what `ged_io` cannot read; see the module documentation.
pub(crate) fn sanitize(gedcom: &str) -> Sanitized<'_> {
    let mut warnings = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut record: Vec<Line<'_>> = Vec::new();
    for line in lines(gedcom) {
        if line.level == Some(0) && !record.is_empty() {
            if let Some(edit) = repair_record(gedcom, &record, &mut warnings) {
                edits.push(edit);
            }
            record.clear();
        }
        record.push(line);
    }
    if let Some(edit) = repair_record(gedcom, &record, &mut warnings) {
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
    if rest.starts_with('@') {
        let xref_len = rest.find([' ', '\t']).unwrap_or(rest.len());
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
        tag,
        value,
        ..unparsed
    }
}

/// The repaired text of one record, as `(start, end, replacement)` over the
/// whole file, or `None` when the record is fine as it is.
fn repair_record(
    gedcom: &str,
    record: &[Line<'_>],
    warnings: &mut Vec<String>,
) -> Option<(usize, usize, String)> {
    let (first, last) = (record.first()?, record.last()?);
    let mut dropped = vec![false; record.len()];
    let mut changed = false;
    for (index, line) in record.iter().enumerate() {
        if line.tag == "AGE" && line.level.is_some() && !is_readable_age(line.value) {
            dropped[index] = true;
            changed = true;
            warnings.push(format!(
                "Line {}: an AGE that is not a GEDCOM age was left out",
                line.number
            ));
        }
    }
    if !changed {
        return None;
    }

    let mut text = String::with_capacity(last.next - first.start);
    let mut skip_below: Option<u8> = None;
    for (index, line) in record.iter().enumerate() {
        if let Some(level) = skip_below {
            if line.level.is_none_or(|l| l > level) {
                continue;
            }
            skip_below = None;
        }
        if dropped[index] {
            skip_below = line.level;
            continue;
        }
        text.push_str(&gedcom[line.start..line.next]);
    }
    Some((first.start, last.next, text))
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
}
