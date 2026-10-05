//! Reading portal markup without an HTML parser.
//!
//! Portals render their search results server-side; an adapter needs a few
//! values out of that markup: an attribute, the text an element shows, the
//! rows of a table. These scans find them by their literal markers, decode
//! the character references the markup uses, and fold text for comparison.
//! They assume markup a server generated, not markup a person wrote: an
//! attribute is double-quoted, and the text an adapter reads holds no
//! nested element.

use oxidgene_core::search::fold_words;

/// Folds text for comparison: case, accents and punctuation ignored, words
/// separated by single spaces. `Bourg (Le)` and `bourg le` fold alike.
pub(crate) fn fold(text: &str) -> String {
    fold_words(text)
}

/// The decoded value of the first `name="…"` attribute.
pub(crate) fn attribute(html: &str, name: &str) -> Option<String> {
    attributes(html, name).into_iter().next()
}

/// The decoded values of every `name="…"` attribute, in order.
pub(crate) fn attributes(html: &str, name: &str) -> Vec<String> {
    let marker = format!("{name}=\"");
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&marker) {
        // `data-name="…"` must not be read as `name="…"`.
        let preceded_by_name = rest[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        rest = &rest[at + marker.len()..];
        let Some(end) = rest.find('"') else {
            break;
        };
        if !preceded_by_name {
            values.push(decode_entities(&rest[..end]));
        }
        rest = &rest[end + 1..];
    }
    values
}

/// The text of every element carrying `name="…"`, with that attribute's
/// value: `(value, text)`. The text runs from the element's opening tag to
/// the next tag, decoded and trimmed; elements showing no text are left out.
pub(crate) fn labelled_texts(html: &str, name: &str) -> Vec<(String, String)> {
    let marker = format!("{name}=\"");
    let mut labelled = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&marker) {
        rest = &rest[at + marker.len()..];
        let Some(value_end) = rest.find('"') else {
            break;
        };
        let value = decode_entities(&rest[..value_end]);
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        rest = &rest[tag_end + 1..];
        let text = decode_entities(rest[..rest.find('<').unwrap_or(rest.len())].trim());
        if !text.is_empty() {
            labelled.push((value, text));
        }
    }
    labelled
}

/// The decoded, trimmed text from just after the first `marker` to the next
/// tag, such as the count in `class="nombre_images">(46 images)<`.
pub(crate) fn text_after(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    Some(decode_entities(
        rest[..rest.find('<').unwrap_or(rest.len())].trim(),
    ))
}

/// The text of a fragment of markup: tags removed, character references
/// decoded, whitespace runs collapsed to single spaces.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => {
                rest = "";
                break;
            }
        }
        // A tag separates words: `<li>a</li><li>b</li>` reads `a b`.
        text.push(' ');
    }
    text.push_str(rest);
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The fragments that follow each occurrence of `marker`, each up to the
/// next one: the rows of a table opened by `<tr class="row`.
pub(crate) fn split_after<'h>(html: &'h str, marker: &str) -> Vec<&'h str> {
    html.split(marker).skip(1).collect()
}

/// The first whole number in `text`: `46` in `(46 images)`.
pub(crate) fn first_number<N: std::str::FromStr>(text: &str) -> Option<N> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let digits = &text[start..];
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse().ok()
}

/// Decodes the character references of markup: the named ones a server
/// emits and every numeric one. Anything else is kept as written.
pub(crate) fn decode_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        decoded.push_str(&rest[..at]);
        rest = &rest[at..];
        let reference = rest
            .find(';')
            .filter(|end| *end <= 10)
            .and_then(|end| Some((entity(&rest[1..end])?, end)));
        match reference {
            Some((character, end)) => {
                decoded.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

fn entity(name: &str) -> Option<char> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_character_references_of_the_markup() {
        assert_eq!(
            decode_entities("Type d&#039;actes &amp; &quot;registres&quot; &#x2014; &lt;b&gt;"),
            "Type d'actes & \"registres\" \u{2014} <b>"
        );
        assert_eq!(decode_entities("A & B &unknown; &"), "A & B &unknown; &");
    }

    #[test]
    fn reads_labelled_texts_skipping_empty_ones() {
        let html = r#"<td><span data-champ="commune" data-type-champ="x">Bourg (Le)</span></td>
            <td><!-- Valeur vide pour le champ: paroisse --></td>
            <td><span data-champ="cote">  </span></td>
            <td><span data-champ="date">1700 &amp; 1701</span></td>"#;
        assert_eq!(
            labelled_texts(html, "data-champ"),
            [
                ("commune".to_owned(), "Bourg (Le)".to_owned()),
                ("date".to_owned(), "1700 & 1701".to_owned())
            ]
        );
    }

    #[test]
    fn reads_attributes_by_their_whole_name() {
        let html = r#"<a data-href="/other" href="/one?a=1&amp;b=2"></a><a href="/two"></a>"#;
        assert_eq!(attribute(html, "href").as_deref(), Some("/one?a=1&b=2"));
        assert_eq!(attributes(html, "href"), ["/one?a=1&b=2", "/two"]);
        assert_eq!(attribute(html, "data-href").as_deref(), Some("/other"));
        assert_eq!(attribute(html, "title"), None);
    }

    #[test]
    fn reads_texts_numbers_and_rows() {
        let html = r#"<span class="nombre_images">(46 images)</span>"#;
        assert_eq!(
            text_after(html, r#"class="nombre_images">"#).as_deref(),
            Some("(46 images)")
        );
        assert_eq!(first_number::<u16>("(46 images)"), Some(46));
        assert_eq!(first_number::<u16>("no images"), None);
        assert_eq!(
            split_after(
                "<table><tr class=\"row a\">1<tr class=\"row b\">2",
                "<tr class=\"row"
            ),
            [" a\">1", " b\">2"]
        );
        assert_eq!(
            strip_tags("<li>\n  Saint-Exemple <b>(Le)</b> &amp; co\n</li><li>2</li>"),
            "Saint-Exemple (Le) & co 2"
        );
        assert_eq!(strip_tags("a <unclosed"), "a");
        assert_eq!(fold("Bourg (Le)"), fold("bourg le"));
        assert_eq!(fold("Étival"), "etival");
    }
}
