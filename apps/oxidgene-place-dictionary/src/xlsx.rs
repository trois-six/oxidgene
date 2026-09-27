//! Reads the first sheet of an Excel workbook: enough of the format for the
//! plain tables statistical offices publish, not a general reader.

use std::collections::HashMap;
use std::io::Read;

use anyhow::{Context, Result};

/// A sheet's rows, each a map from column letters to cell text.
pub type Rows = Vec<HashMap<String, String>>;

/// The first sheet's rows.
pub fn first_sheet(bytes: &[u8]) -> Result<Rows> {
    Ok(sheets(bytes)?.into_iter().next().unwrap_or_default())
}

/// Every sheet's rows, in the order of the workbook's sheet files.
pub fn sheets(bytes: &[u8]) -> Result<Vec<Rows>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut read = |name: &str| -> Result<String> {
        let mut text = String::new();
        archive
            .by_name(name)
            .with_context(|| format!("the workbook has no {name}"))?
            .read_to_string(&mut text)?;
        Ok(text)
    };
    let shared = read("xl/sharedStrings.xml").unwrap_or_default();
    let strings: Vec<String> = elements(&shared, "si")
        .map(|si| elements(si, "t").map(unescape).collect())
        .collect();
    let mut sheets = Vec::new();
    for number in 1.. {
        let Ok(sheet) = read(&format!("xl/worksheets/sheet{number}.xml")) else {
            break;
        };
        sheets.push(rows(&sheet, &strings));
    }
    if sheets.is_empty() {
        anyhow::bail!("the workbook has no sheet");
    }
    Ok(sheets)
}

fn rows(sheet: &str, strings: &[String]) -> Rows {
    let mut rows = Vec::new();
    for row in elements(sheet, "row") {
        let mut cells = HashMap::new();
        for (attributes, content) in elements_with_attributes(row, "c") {
            let Some(reference) = attribute(attributes, "r") else {
                continue;
            };
            let column: String = reference
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            let value = match attribute(attributes, "t") {
                Some("s") => elements(content, "v")
                    .next()
                    .and_then(|v| v.parse::<usize>().ok())
                    .and_then(|i| strings.get(i).cloned())
                    .unwrap_or_default(),
                Some("inlineStr") => elements(content, "t").map(unescape).collect(),
                _ => elements(content, "v")
                    .next()
                    .map(unescape)
                    .unwrap_or_default(),
            };
            cells.insert(column, value);
        }
        rows.push(cells);
    }
    rows
}

/// The contents of every `<tag …>…</tag>` in `xml`, in order.
fn elements<'a>(xml: &'a str, tag: &'a str) -> impl Iterator<Item = &'a str> {
    elements_with_attributes(xml, tag).map(|(_, content)| content)
}

/// `(attributes, content)` of every `<tag …>…</tag>` or `<tag …/>` in `xml`.
fn elements_with_attributes<'a>(
    xml: &'a str,
    tag: &'a str,
) -> impl Iterator<Item = (&'a str, &'a str)> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = xml;
    std::iter::from_fn(move || {
        loop {
            let at = rest.find(&open)?;
            let after = &rest[at + open.len()..];
            // `<t` must not match `<tbody`: the name ends at a space, `>` or `/`.
            if !after.starts_with([' ', '>', '/']) {
                rest = after;
                continue;
            }
            let end_of_tag = after.find('>')?;
            let attributes = after[..end_of_tag].trim_end_matches('/');
            if after[..end_of_tag].ends_with('/') {
                rest = &after[end_of_tag + 1..];
                return Some((attributes, ""));
            }
            let body = &after[end_of_tag + 1..];
            let end = body.find(&close)?;
            rest = &body[end + close.len()..];
            return Some((attributes, &body[..end]));
        }
    })
}

fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let at = attributes.find(&key)? + key.len();
    let end = attributes[at..].find('"')?;
    Some(&attributes[at..at + end])
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_shared_and_inline_values() {
        let shared = r#"<sst><si><t>Villa A</t></si><si><r><t>Villa </t></r><r><t>B &amp; C</t></r></si></sst>"#;
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42</v></c></row>
            <row r="2"><c r="A2" t="s"><v>1</v></c><c r="C2" s="3"/></row>
        </sheetData></worksheet>"#;
        let strings: Vec<String> = elements(shared, "si")
            .map(|si| elements(si, "t").map(unescape).collect())
            .collect();
        assert_eq!(strings, ["Villa A", "Villa B & C"]);
        let cells: Vec<_> = elements_with_attributes(sheet, "c")
            .map(|(a, _)| attribute(a, "r").unwrap().to_string())
            .collect();
        assert_eq!(cells, ["A1", "B1", "A2", "C2"]);
    }
}
