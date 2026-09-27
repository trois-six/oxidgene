//! The little CSV the sources need: RFC 4180 quoting, one header row, and a
//! decoder for files that mix UTF-8 with Windows-1252.

use anyhow::{Context, Result, bail};

/// A delimited file read with its header, so columns are looked up by name.
pub struct Table {
    header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn parse(text: &str, delimiter: char) -> Result<Self> {
        let mut records = parse_records(text.trim_start_matches('\u{feff}'), delimiter)?;
        if records.is_empty() {
            bail!("the file has no header row");
        }
        // A blank line is no record, wherever a source leaves one.
        records.retain(|r| !(r.len() == 1 && r[0].trim().is_empty()));
        let header = records.remove(0);
        if let Some(line) = records.iter().position(|r| r.len() != header.len()) {
            bail!(
                "record {} has {} fields where the header has {}",
                line + 2,
                records[line].len(),
                header.len()
            );
        }
        Ok(Self {
            header,
            rows: records,
        })
    }

    /// Index of the column named `prefix`, two digits, then `suffix`: the
    /// ONS stamps the edition's year into column names (`place23nm`).
    pub fn dated_column(&self, prefix: &str, suffix: &str) -> Result<usize> {
        self.header
            .iter()
            .position(|h| {
                h.strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .is_some_and(|year| year.len() == 2 && year.bytes().all(|b| b.is_ascii_digit()))
            })
            .with_context(|| format!("no column `{prefix}NN{suffix}` (have {:?})", self.header))
    }

    /// Index of the first column whose header satisfies `wanted`.
    pub fn column_where(&self, wanted: impl Fn(&str) -> bool) -> Option<usize> {
        self.header.iter().position(|h| wanted(h))
    }

    /// Index of a named column, so a source that renames one fails loudly.
    pub fn column(&self, name: &str) -> Result<usize> {
        self.header
            .iter()
            .position(|h| h == name)
            .with_context(|| format!("column `{name}` is missing (have {:?})", self.header))
    }
}

fn parse_records(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == delimiter {
            record.push(std::mem::take(&mut field));
        } else if c == '\n' || c == '\r' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            record.push(std::mem::take(&mut field));
            records.push(std::mem::take(&mut record));
        } else {
            field.push(c);
        }
    }
    if quoted {
        bail!("unterminated quoted field");
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    Ok(records)
}

/// Quotes every field, as Geneanet's dictionaries do.
pub fn quote(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// Decodes text that is mostly Windows-1252 but carries some UTF-8 sequences,
/// as the ONS Index of Place Names does: valid UTF-8 is kept, and every byte
/// that is not part of it is read as Windows-1252.
pub fn decode_mixed(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        out.extend(chunk.invalid().iter().map(|&b| windows_1252(b)));
    }
    out
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9f => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_quoted_fields_with_delimiters_and_escaped_quotes() {
        let table = Table::parse(
            "\"A\",\"B\"\r\n\"x, y\",\"say \"\"hi\"\"\"\r\nplain,\n",
            ',',
        )
        .unwrap();
        assert_eq!(table.column("B").unwrap(), 1);
        assert_eq!(
            table.rows,
            vec![
                vec!["x, y".to_string(), "say \"hi\"".to_string()],
                vec!["plain".to_string(), String::new()],
            ]
        );
    }

    #[test]
    fn blank_lines_are_not_records() {
        let table = Table::parse("A;B\r\n1;2\r\n\r\n", ';').unwrap();
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn a_truncated_record_is_an_error() {
        // What Wikidata sends when a query times out halfway through.
        assert!(Table::parse("A,B\n1,2\njava.lang.Exception\n", ',').is_err());
    }

    #[test]
    fn finds_a_column_whatever_the_edition_year() {
        let table = Table::parse("place23nm,place23cd\n", ',').unwrap();
        assert_eq!(table.dated_column("place", "cd").unwrap(), 1);
        assert!(table.dated_column("ctry", "nm").is_err());
    }

    #[test]
    fn a_missing_column_is_an_error() {
        let table = Table::parse("A\n1\n", ',').unwrap();
        assert!(table.column("B").is_err());
    }

    #[test]
    fn quoting_doubles_inner_quotes() {
        assert_eq!(quote("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn mixed_encodings_decode_to_the_same_letters() {
        // "é" once in UTF-8, once in Windows-1252, then a Windows-1252 "’".
        let bytes = b"caf\xc3\xa9 caf\xe9 l\x92eau";
        assert_eq!(decode_mixed(bytes), "café café l’eau");
    }
}
