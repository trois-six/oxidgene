//! Reading portal markup without an HTML parser.
//!
//! Portals render their search results server-side; an adapter needs a few
//! values out of that markup: an attribute, the text an element shows, the
//! rows of a table. These scans find them by their literal markers, decode
//! the character references the markup uses, and fold text for comparison.
//! They assume markup a server generated, not markup a person wrote: an
//! attribute is double-quoted, and the text an adapter reads holds no
//! nested element.

use std::sync::LazyLock;

use oxidgene_core::search::fold_words;
use serde::Deserialize;

use crate::ResolveError;

/// What an anti-bot measure answering in place of a portal's page asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Guard {
    /// A check a browser passes: a proof of work, a script that sets a
    /// cookie and reloads, or a widget the reader answers.
    Challenge,
    /// A refusal nobody can pass from this browser.
    Block,
}

/// One anti-bot page, recognized in its markup.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Signature {
    /// The vendor, for logs and reports: `cloudflare`, `anubis`, `f5`…
    pub vendor: String,
    pub guard: Guard,
    /// Lower-case fragments the markup holds, every one of them.
    pub markers: Vec<String>,
    /// Lower-case fragments that rule the signature out.
    #[serde(default)]
    pub unless: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Signatures {
    signatures: Vec<Signature>,
    widgets: Vec<String>,
}

/// The anti-bot pages portals answer in place of their own, and the
/// fragments of a widget asking the reader to answer a check, as one JSON
/// document: the adapters read answers with it, the desktop's archive
/// window classifies its pages with it (`ANTI_BOT_JSON` is handed to its
/// script), and the live checks' Playwright side reads the same file. A
/// signature matches markup, lower-cased, holding all its `markers` and none
/// of its `unless`; the first match in order wins, so a block is listed
/// before a challenge of the same vendor.
pub const ANTI_BOT_JSON: &str = include_str!("challenges.json");

static ANTI_BOT: LazyLock<Signatures> = LazyLock::new(|| {
    serde_json::from_str(ANTI_BOT_JSON)
        .unwrap_or_else(|error| panic!("the embedded anti-bot signatures: {error}"))
});

/// The anti-bot page `markup` is, if any: a page or an answer's body.
pub fn anti_bot(markup: &str) -> Option<&'static Signature> {
    let lowered = markup.to_lowercase();
    ANTI_BOT.signatures.iter().find(|signature| {
        signature
            .markers
            .iter()
            .all(|marker| lowered.contains(marker.as_str()))
            && !signature
                .unless
                .iter()
                .any(|marker| lowered.contains(marker.as_str()))
    })
}

/// Whether `markup` shows a widget asking the reader to answer a check.
pub fn shows_check_widget(markup: &str) -> bool {
    let lowered = markup.to_lowercase();
    ANTI_BOT
        .widgets
        .iter()
        .any(|widget| lowered.contains(widget.as_str()))
}

/// Whether `answer` is an anti-bot page, challenge or block, rather than
/// the portal's page.
pub(crate) fn is_challenge(answer: &str) -> bool {
    anti_bot(answer).is_some()
}

/// The error of an answer an adapter cannot read: `Challenged` when it is
/// an anti-bot page, a changed shape described by `detail` otherwise, so a
/// live check tells a challenge from drift.
pub(crate) fn unreadable(answer: &str, detail: String) -> ResolveError {
    if is_challenge(answer) {
        ResolveError::Challenged
    } else {
        ResolveError::UnexpectedResponse(detail)
    }
}

/// Folds text for comparison: case, accents and punctuation ignored, words
/// separated by single spaces. `Bourg (Le)` and `bourg le` fold alike.
pub(crate) fn fold(text: &str) -> String {
    fold_words(text)
}

/// The ASCII letters and digits of `text` once folded: `L'Étang` is `letang`.
pub(crate) fn letters(text: &str) -> Vec<char> {
    fold(text)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Whether a portal's text names one of the `wanted` [`letters`].
///
/// Some portals serve ISO-8859-1 pages. A transport that decodes them as
/// UTF-8 turns each accented letter into U+FFFD, which then stands for any
/// one letter.
pub(crate) fn is_named(portal: &str, wanted: &[Vec<char>]) -> bool {
    if !portal.contains('\u{fffd}') {
        let name = letters(portal);
        return wanted.contains(&name);
    }
    let pattern = lossy_letters(portal);
    wanted.iter().any(|form| {
        form.len() == pattern.len()
            && form
                .iter()
                .zip(&pattern)
                .all(|(letter, known)| known.is_none_or(|known| known == *letter))
    })
}

/// The letters and digits of a text decoded lossily, lower-cased: `None`
/// for each U+FFFD, which stands for a letter the decoding lost.
pub(crate) fn lossy_letters(portal: &str) -> Vec<Option<char>> {
    portal
        .split('\u{fffd}')
        .enumerate()
        .flat_map(|(index, part)| {
            let lost = (index > 0).then_some(None);
            lost.into_iter().chain(letters(part).into_iter().map(Some))
        })
        .collect()
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

/// The fragments that follow each occurrence of `marker`, each up to the
/// next one: the rows of a table opened by `<tr class="row`.
pub(crate) fn split_after<'h>(html: &'h str, marker: &str) -> Vec<&'h str> {
    html.split(marker).skip(1).collect()
}

/// The text of `html` without its tags: character references decoded,
/// whitespace runs collapsed to one space, ends trimmed. A block-level tag
/// (`br`, `div`, `p`, `li`, `td`, `tr`, `th`) separates words, an inline one
/// (`mark`, `span`, `a`) does not, so a highlighted part of a name stays in
/// it. Elements are not interpreted, so a `<script>` body would be kept: use
/// it on cells that hold text and links.
pub(crate) fn strip_tags(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = "";
            break;
        };
        let name: String = rest[open + 1..open + close]
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        if ["br", "div", "p", "li", "td", "tr", "th"]
            .iter()
            .any(|block| name.eq_ignore_ascii_case(block))
        {
            text.push(' ');
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    fn tells_an_anti_bot_challenge_from_a_changed_shape() {
        for page in [
            "<script>window.location.href='/redirect_0000/search'</script>",
            "<title>Making sure you&#39;re not a bot!</title> Anubis",
            "<html><body>Request Rejected</body></html>",
        ] {
            assert!(is_challenge(page), "{page}");
            assert_eq!(unreadable(page, String::new()), ResolveError::Challenged);
        }
        assert_eq!(
            unreadable("{\"other\": 1}", "changed".to_owned()),
            ResolveError::UnexpectedResponse("changed".to_owned())
        );
    }

    fn verdict(markup: &str) -> Option<(&'static str, Guard)> {
        anti_bot(markup).map(|signature| (signature.vendor.as_str(), signature.guard))
    }

    #[test]
    fn classifies_each_vendor_s_challenge_and_block() {
        let challenge = Some(Guard::Challenge);
        let block = Some(Guard::Block);
        for (page, vendor, guard) in [
            (
                "<html><head><title>Just a moment...</title></head><body><script>window._cf_chl_opt={cType:'managed'};</script></body></html>",
                "cloudflare",
                challenge,
            ),
            (
                "<title>Attention Required! | Cloudflare</title><div id=\"cf-error-details\"><h1>Sorry, you have been blocked</h1></div>",
                "cloudflare",
                block,
            ),
            (
                "<title>V\u{e9}rification que vous n'\u{ea}tes pas un robot !</title><script id=\"anubis_challenge\" type=\"application/json\">{}</script><script src=\"/.within.website/x/cmd/anubis/static/js/main.mjs\"></script>",
                "anubis",
                challenge,
            ),
            (
                "<title>Oh non !</title><link href=\"/.within.website/x/xess/xess.css\"><p>Acc\u{e8}s refus\u{e9} : code d'erreur 0f3a</p>",
                "anubis",
                block,
            ),
            (
                "<script>window[\"bobcmn\"] = \"10111\";</script><script src=\"/TSPD/0000?type=11\"></script>",
                "f5",
                challenge,
            ),
            (
                "<script src=\"/TSPD/0000?type=9\"></script><body>Please enable JavaScript to view the page content.</body>",
                "f5",
                challenge,
            ),
            (
                "<html><head><title>Request Rejected</title></head><body>The requested URL was rejected.</body></html>",
                "f5",
                block,
            ),
            (
                "<html><body><script>window.location.href='/redirect_0000/search'</script></body></html>",
                "bot-mitigation",
                challenge,
            ),
            (
                "<form><altcha-widget challengeurl=\"/altcha\" auto=\"onload\"></altcha-widget></form>",
                "altcha",
                challenge,
            ),
        ] {
            assert_eq!(verdict(page).map(|(v, _)| v), Some(vendor), "{page}");
            assert_eq!(verdict(page).map(|(_, g)| g), guard, "{page}");
        }
    }

    #[test]
    fn a_portal_page_carrying_a_vendor_s_scripts_is_not_a_challenge() {
        // F5 and Cloudflare inject their scripts into the pages a browser
        // reaches once it has passed: only the check's own page is one.
        for page in [
            "<title>L'\u{e9}tat civil | Archives</title><script src=\"/TSPD/0000?type=17\"></script><main>Registres</main>",
            "<title>Recherche</title><script>window.__CF$cv$params={r:'0'};a.src='/cdn-cgi/challenge-platform/scripts/jsd/main.js'</script>",
            "<frameset><frame src=\"FrmSommaire.asp\"></frameset>",
        ] {
            assert_eq!(anti_bot(page), None, "{page}");
        }
    }

    #[test]
    fn tells_a_widget_asking_the_reader() {
        assert!(shows_check_widget(
            "<div class=\"cf-turnstile\" data-sitekey=\"0x0\"></div>"
        ));
        assert!(shows_check_widget(
            "<iframe src=\"https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/turnstile/if/\"></iframe>"
        ));
        // The check's own security policy names the host without a widget.
        assert!(!shows_check_widget(
            "<meta http-equiv=\"content-security-policy\" content=\"script-src https://challenges.cloudflare.com\">"
        ));
    }

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
    fn strips_tags_and_collapses_whitespace() {
        assert_eq!(
            strip_tags(
                "<td>\n <mark class=\"m\">Exemple</mark>-sur-Mer &amp; <b>Co</b><br/>x </td>"
            ),
            "Exemple-sur-Mer & Co x"
        );
        assert_eq!(strip_tags("plain"), "plain");
        assert_eq!(strip_tags("cut <a href"), "cut");
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
