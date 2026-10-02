//! Raw HTML in the UI: every `dangerous_inner_html` is declared with the
//! sanitizer that makes it safe.
//!
//! Drift it prevents: markup built from user or imported text and handed to
//! the browser unescaped — a person's name, a place, a note — which is a
//! script injection in the web build and in the desktop WebView alike.
//!
//! Fixing a failure: pass the text through `escape_xml` (or `svg_title`,
//! which escapes) or, for a note body, through `note_html_for_display` on
//! text `oxidgene_db::html` sanitized on write; then add the site to
//! `ALLOWED` naming that sanitizer. A stale entry fails too: remove it.

use oxidgene_guards::{files, line_of, read, relative};

/// `(file under crates/oxidgene-ui/src, expression, sanitizer)`.
const ALLOWED: &[(&str, &str, &str)] = &[
    (
        "components/person_profile.rs",
        "note_html_for_display(&note.text)",
        "note bodies are sanitized by ammonia (oxidgene_db::html) when stored",
    ),
    (
        "components/pedigree_chart/lineage.rs",
        "\"{title}\"",
        "svg_title, which escapes with escape_xml",
    ),
    (
        "components/pedigree_chart/lineage.rs",
        "\"{svg_title(&title)}\"",
        "svg_title, which escapes with escape_xml",
    ),
    (
        "components/pedigree_chart/lineage.rs",
        "\"{svg_title(&label)}\"",
        "svg_title, which escapes with escape_xml",
    ),
    (
        "components/pedigree_chart/circular.rs",
        "\"{title}\"",
        "svg_title, which escapes with escape_xml",
    ),
    (
        "components/pedigree_chart.rs",
        "\"{date_html}\"",
        "escape_xml on the date's tooltip",
    ),
];

/// Every sanitizer the allowlist relies on must still exist.
const SANITIZERS: &[(&str, &str)] = &[
    ("crates/oxidgene-ui/src/utils.rs", "pub fn escape_xml("),
    (
        "crates/oxidgene-ui/src/utils.rs",
        "pub fn note_html_for_display(",
    ),
    (
        "crates/oxidgene-ui/src/components/pedigree_chart/ancestors.rs",
        "fn svg_title(",
    ),
    ("crates/oxidgene-db/src/html.rs", "ammonia"),
];

#[test]
fn every_raw_html_site_names_its_sanitizer() {
    let root = oxidgene_guards::root().join("crates/oxidgene-ui/src");
    let mut unknown = Vec::new();
    let mut used = vec![false; ALLOWED.len()];
    for path in files("crates/oxidgene-ui/src", "rs") {
        let file = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).unwrap();
        for (at, _) in source.match_indices("dangerous_inner_html:") {
            let rest = source[at + "dangerous_inner_html:".len()..].trim_start();
            // A string literal up to its closing quote, or an expression up
            // to the end of the attribute.
            let end = match rest.strip_prefix('"') {
                Some(literal) => literal.find('"').map_or(rest.len(), |close| close + 2),
                None => rest.find([',', '\n']).unwrap_or(rest.len()),
            };
            let expression = rest[..end].trim();
            match ALLOWED
                .iter()
                .position(|(f, e, _)| *f == file && *e == expression)
            {
                Some(index) => used[index] = true,
                None => unknown.push(format!(
                    "{}:{}: {expression}",
                    relative(&path),
                    line_of(&source, at)
                )),
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "dangerous_inner_html without a declared sanitizer:\n{}",
        unknown.join("\n")
    );
    let stale: Vec<_> = ALLOWED
        .iter()
        .zip(used)
        .filter(|(_, used)| !used)
        .map(|((file, expression, _), _)| format!("{file}: {expression}"))
        .collect();
    assert!(
        stale.is_empty(),
        "allowlist entries matching nothing: {stale:?}"
    );
}

#[test]
fn the_declared_sanitizers_exist() {
    for (file, needle) in SANITIZERS {
        assert!(read(file).contains(needle), "{file} lost {needle}");
    }
}
