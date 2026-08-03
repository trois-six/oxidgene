//! HTML sanitizing for user-supplied rich text.
//!
//! Note bodies are rendered as HTML rather than escaped text, because the
//! formats OxidGene imports put markup in them: GeneWeb notes are wiki text
//! whose line breaks are literal `<br>` tags, and hand-written GEDCOM notes
//! sometimes carry `<b>`/`<i>` too. Rendering that markup means anything a
//! note contains reaches the DOM, so every write path has to go through
//! [`sanitize_note_html`] first.
//!
//! Sanitizing happens **on write**, not on read: notes are stored already
//! clean, so the read paths, the GraphQL layer and the UI can render
//! `Note::text` without each having to remember to filter it. The trade-off is
//! that the stored text is no longer byte-identical to the imported file, so a
//! GEDCOM export round-trip returns the sanitized body, not the original.
//! Rows written before this existed are cleaned by the
//! `m20260802_000001_sanitize_note_html` migration.
//!
//! # Line breaks
//!
//! The same note reaches us spelled three different ways depending on where it
//! came from: GEDCOM `CONT` lines arrive as `\n`, GeneWeb `.gw` notes end their
//! lines with `<br>` — usually `<br>` *and* the `\n` that follows it in the
//! file — and a note typed into the app's textarea is plain `\n`. Rendered as
//! HTML those give no break, a double break, and no break respectively, for
//! text the author meant identically.
//!
//! [`sanitize_note_html`] therefore canonicalises every break to a single `\n`
//! (see [`normalize_line_breaks`]) and the display layer turns `\n` back into
//! `<br>`. Storing the plain-text form rather than the markup one is what keeps
//! GEDCOM export writing real `CONT` lines instead of a literal `<br>`, keeps
//! the note textarea showing text instead of tags, and gives previews and any
//! future full-text index clean input for free. The cost is that a `<br>` the
//! author genuinely typed is no longer distinguishable from a newline, which
//! for genealogy notes it never usefully was.

use std::sync::LazyLock;

use ammonia::Builder;

/// Elements a note body may keep. Structure and inline formatting, plus links
/// and images — everything else is unwrapped to its text content.
const ALLOWED_TAGS: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "span",
    "strong",
    "sub",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
];

/// The shared sanitizer.
///
/// Beyond the allowlists set here, `ammonia` drops `<script>` and `<style>`
/// wholesale (contents included), strips every attribute that is not named
/// below — which is what removes `onclick` and friends, since no `on*`
/// attribute is ever allowed — and rewrites any URL whose scheme is not in
/// `url_schemes`, so `javascript:` and `data:` payloads cannot survive on an
/// `href` or `src`.
static NOTE_SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .tag_attributes(
            [
                ("a", ["href", "title"].into_iter().collect()),
                (
                    "img",
                    ["src", "alt", "title", "width", "height"]
                        .into_iter()
                        .collect(),
                ),
                ("td", ["colspan", "rowspan"].into_iter().collect()),
                ("th", ["colspan", "rowspan"].into_iter().collect()),
            ]
            .into_iter()
            .collect(),
        )
        // `data:` is deliberately absent: an SVG data URI is a script carrier
        // in any context that is not a plain `<img>`.
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        // Neutralise reverse-tabnabbing on links opened in a new tab.
        .link_rel(Some("noopener noreferrer"));
    builder
});

/// Elements after which a line break carries no meaning, because the element
/// already breaks the flow. A `\n` sitting against one of these is source
/// formatting, not something the note's author asked for.
const BLOCK_TAGS: &[&str] = &[
    "blockquote",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "li",
    "ol",
    "p",
    "pre",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Cap on consecutive breaks, so a deliberate `<br><br>` stays one blank line
/// and a longer run does not open a hole in the middle of a note.
const MAX_CONSECUTIVE_BREAKS: usize = 2;

/// Strip anything executable or otherwise unwanted from a note body, and
/// canonicalise its line breaks to `\n` (see the module docs).
///
/// Safe to apply repeatedly: sanitized output run through again is unchanged,
/// which is what lets the migration and the write paths both call it without
/// coordinating.
#[must_use]
pub fn sanitize_note_html(text: &str) -> String {
    normalize_line_breaks(&NOTE_SANITIZER.clean(text).to_string())
}

/// One piece of a sanitized note body.
enum Item<'a> {
    /// Text with no newline in it — newlines become [`Item::Break`].
    Text(&'a str),
    /// A tag, verbatim, minus `<br>` which is a break instead.
    Tag { raw: &'a str, block: bool },
    /// A line break, remembering whether it was written as `<br>` or as `\n`.
    Break { from_br: bool },
}

impl Item<'_> {
    /// Whether this item is whitespace that a break may be separated from its
    /// neighbours by without the two counting as unrelated.
    fn is_horizontal_space(&self) -> bool {
        matches!(self, Self::Text(t) if t.chars().all(|c| c == ' ' || c == '\t' || c == '\r'))
    }
}

/// Rewrite every way of spelling a line break into a run of plain `\n`.
///
/// The input must already be `ammonia` output, which is what makes the split
/// below sound: `<` and `>` only ever delimit a tag, since both are escaped in
/// text and in attribute values.
///
/// Counting rules for a run of adjacent breaks:
///
/// - `<br>` immediately followed (or preceded) by a newline is **one** break.
///   That is the GeneWeb shape, where the tag ends a line that also ends with
///   `\n`; taking it as two is what renders `.gw` imports double-spaced.
/// - Two `<br>` in a row are **two** breaks — a deliberate blank line.
/// - A run sitting against a block element, or against either end of the note,
///   is dropped: those positions already break the flow.
fn normalize_line_breaks(html: &str) -> String {
    let items = split_items(html);
    let mut out = String::with_capacity(html.len());
    let mut idx = 0;

    while idx < items.len() {
        match &items[idx] {
            Item::Text(text) => {
                out.push_str(text);
                idx += 1;
            }
            Item::Tag { raw, .. } => {
                out.push_str(raw);
                idx += 1;
            }
            Item::Break { .. } => {
                let (run_end, br_count, nl_count) = scan_break_run(&items, idx);
                if !break_run_is_redundant(&items, idx, run_end) {
                    // A `<br>` in the run means the newlines around it are the
                    // same break written twice, so the tags alone are counted.
                    let count = if br_count > 0 { br_count } else { nl_count };
                    for _ in 0..count.min(MAX_CONSECUTIVE_BREAKS) {
                        out.push('\n');
                    }
                }
                idx = run_end;
            }
        }
    }

    out
}

/// Split sanitized HTML into tags, text and breaks.
fn split_items(html: &str) -> Vec<Item<'_>> {
    let mut items = Vec::new();
    let mut rest = html;

    while let Some(open) = rest.find('<') {
        push_text(&mut items, &rest[..open]);
        // An unterminated `<` cannot come out of the sanitizer, but treating it
        // as running to the end keeps this total rather than panicking.
        let close = rest[open..].find('>').map_or(rest.len(), |o| open + o + 1);
        let raw = &rest[open..close];
        let name = tag_name(raw);
        if name == "br" {
            items.push(Item::Break { from_br: true });
        } else {
            items.push(Item::Tag {
                raw,
                block: BLOCK_TAGS.contains(&name.as_str()),
            });
        }
        rest = &rest[close..];
    }
    push_text(&mut items, rest);

    items
}

/// Append `text`, turning each of its newlines into a break.
fn push_text<'a>(items: &mut Vec<Item<'a>>, text: &'a str) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            items.push(Item::Break { from_br: false });
        }
        if !line.is_empty() {
            items.push(Item::Text(line));
        }
    }
}

/// The lowercase element name of a tag, e.g. `"a"` for both `<a href=…>` and
/// `</a>`.
fn tag_name(raw: &str) -> String {
    raw.trim_start_matches('<')
        .trim_start_matches('/')
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

/// Measure the run of breaks starting at `start`, returning where it ends and
/// how many of each kind it holds. Horizontal whitespace between two breaks is
/// part of the run and is dropped with it.
fn scan_break_run(items: &[Item<'_>], start: usize) -> (usize, usize, usize) {
    let (mut br_count, mut nl_count) = (0, 0);
    let mut end = start;
    let mut cursor = start;

    while cursor < items.len() {
        match &items[cursor] {
            Item::Break { from_br } => {
                if *from_br {
                    br_count += 1;
                } else {
                    nl_count += 1;
                }
                cursor += 1;
                end = cursor;
            }
            // Only kept if another break follows — otherwise it is the note's
            // own text and stays out of the run.
            item if item.is_horizontal_space() => cursor += 1,
            _ => break,
        }
    }

    (end, br_count, nl_count)
}

/// Whether the run in `items[start..end]` sits somewhere a break shows nothing:
/// against a block element, or against either end of the note.
fn break_run_is_redundant(items: &[Item<'_>], start: usize, end: usize) -> bool {
    let before = items[..start]
        .iter()
        .rev()
        .find(|item| !item.is_horizontal_space());
    let after = items[end..].iter().find(|item| !item.is_horizontal_space());

    matches!(before, None | Some(Item::Tag { block: true, .. }))
        || matches!(after, None | Some(Item::Tag { block: true, .. }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_formatting_links_and_images() {
        let clean = sanitize_note_html(
            r#"<p>Ne <b>en 1802</b><br>a <a href="https://example.org/x">la source</a></p>
<img src="https://example.org/p.jpg" alt="portrait">"#,
        );
        assert!(clean.contains("<b>en 1802</b>"), "got: {clean}");
        assert!(clean.contains("1802</b>\na "), "got: {clean}");
        assert!(
            clean.contains(r#"href="https://example.org/x""#),
            "got: {clean}"
        );
        assert!(
            clean.contains(r#"src="https://example.org/p.jpg""#),
            "got: {clean}"
        );
        assert!(clean.contains(r#"alt="portrait""#), "got: {clean}");
    }

    #[test]
    fn drops_scripts_and_event_handlers() {
        let clean = sanitize_note_html(
            r#"<p onclick="steal()">hello</p><script>alert(1)</script><img src=x onerror="alert(1)">"#,
        );
        assert!(!clean.contains("onclick"), "got: {clean}");
        assert!(!clean.contains("onerror"), "got: {clean}");
        assert!(!clean.contains("alert"), "got: {clean}");
        assert!(clean.contains("hello"), "got: {clean}");
    }

    #[test]
    fn drops_javascript_and_data_urls() {
        let clean = sanitize_note_html(
            r#"<a href="javascript:alert(1)">x</a><img src="data:image/svg+xml;base64,PHN2Zz4=">"#,
        );
        assert!(!clean.contains("javascript:"), "got: {clean}");
        assert!(!clean.contains("data:image"), "got: {clean}");
    }

    #[test]
    fn drops_framing_and_form_elements() {
        let clean = sanitize_note_html(
            r#"<iframe src="https://evil.example"></iframe><form action="https://evil.example"><input name="p"></form><object data="x.swf"></object>"#,
        );
        assert!(!clean.contains("<iframe"), "got: {clean}");
        assert!(!clean.contains("<form"), "got: {clean}");
        assert!(!clean.contains("<input"), "got: {clean}");
        assert!(!clean.contains("<object"), "got: {clean}");
    }

    #[test]
    fn adds_rel_to_links() {
        let clean = sanitize_note_html(r#"<a href="https://example.org">x</a>"#);
        assert!(clean.contains("noopener"), "got: {clean}");
        assert!(clean.contains("noreferrer"), "got: {clean}");
    }

    #[test]
    fn plain_text_survives_unchanged() {
        assert_eq!(
            sanitize_note_html("Ne a Paris en 1802"),
            "Ne a Paris en 1802"
        );
    }

    #[test]
    fn escapes_stray_angle_brackets() {
        // A note that was never HTML must not lose text to a half-open tag.
        let clean = sanitize_note_html("2 < 3 et 4 > 1");
        assert!(clean.contains('3') && clean.contains('4'), "got: {clean}");
    }

    #[test]
    fn is_idempotent() {
        let once = sanitize_note_html(r#"<p onclick="x()">a<br><a href="https://e.org">l</a></p>"#);
        assert_eq!(sanitize_note_html(&once), once);
    }

    #[test]
    fn br_becomes_a_newline() {
        assert_eq!(sanitize_note_html("un<br>deux"), "un\ndeux");
        assert_eq!(sanitize_note_html("un<br/>deux"), "un\ndeux");
        assert_eq!(sanitize_note_html("un<br />deux"), "un\ndeux");
    }

    #[test]
    fn a_newline_stays_a_single_break() {
        assert_eq!(sanitize_note_html("un\ndeux"), "un\ndeux");
    }

    #[test]
    fn br_glued_to_a_newline_is_one_break() {
        // The GeneWeb shape: `.gw` note lines end with the tag *and* the file's
        // own newline. Counting both is what renders those imports double-spaced.
        assert_eq!(sanitize_note_html("un<br/>\ndeux"), "un\ndeux");
        assert_eq!(sanitize_note_html("un<br/> \n deux"), "un\n deux");
        assert_eq!(sanitize_note_html("un\n<br/>deux"), "un\ndeux");
    }

    #[test]
    fn a_doubled_br_stays_a_blank_line() {
        assert_eq!(sanitize_note_html("un<br /><br />\ndeux"), "un\n\ndeux");
        assert_eq!(sanitize_note_html("un\n\ndeux"), "un\n\ndeux");
    }

    #[test]
    fn long_break_runs_are_capped() {
        assert_eq!(sanitize_note_html("un<br><br><br><br>deux"), "un\n\ndeux");
        assert_eq!(sanitize_note_html("un\n\n\n\ndeux"), "un\n\ndeux");
    }

    #[test]
    fn breaks_against_a_block_element_are_dropped() {
        // Source formatting, not something the note's author asked for.
        assert_eq!(
            sanitize_note_html("<p>un</p>\n<p>deux</p>"),
            "<p>un</p><p>deux</p>"
        );
        assert_eq!(
            sanitize_note_html("<ul>\n<li>un</li>\n<li>deux</li>\n</ul>"),
            "<ul><li>un</li><li>deux</li></ul>"
        );
        assert_eq!(sanitize_note_html("\nun<br>"), "un");
    }

    #[test]
    fn breaks_inside_a_block_element_survive() {
        assert_eq!(sanitize_note_html("<p>un<br>deux</p>"), "<p>un\ndeux</p>");
    }

    #[test]
    fn the_same_note_normalizes_the_same_from_both_formats() {
        // `samples/juesce_2026-08-01.{ged,gw}` hold this note in both spellings.
        let from_gedcom = sanitize_note_html("Capitaine de réserve\nRésistant: cote GR 16 P.");
        let from_geneweb =
            sanitize_note_html("Capitaine de réserve<br/>\nRésistant: cote GR 16 P.");
        assert_eq!(from_gedcom, from_geneweb);
        assert_eq!(
            from_gedcom,
            "Capitaine de réserve\nRésistant: cote GR 16 P."
        );
    }

    #[test]
    fn break_normalization_is_idempotent() {
        for input in [
            "un<br/>\ndeux<br /><br />trois",
            "<p>un</p>\n<p>deux<br>trois</p>",
            "un\n\n\ndeux",
        ] {
            let once = sanitize_note_html(input);
            assert_eq!(sanitize_note_html(&once), once, "input: {input}");
        }
    }
}
