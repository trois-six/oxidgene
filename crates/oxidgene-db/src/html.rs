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

/// Strip anything executable or otherwise unwanted from a note body.
///
/// Safe to apply repeatedly: sanitized output run through again is unchanged,
/// which is what lets the migration and the write paths both call it without
/// coordinating.
#[must_use]
pub fn sanitize_note_html(text: &str) -> String {
    NOTE_SANITIZER.clean(text).to_string()
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
        assert!(clean.contains("<br>"), "got: {clean}");
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
}
