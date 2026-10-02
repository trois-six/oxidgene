//! Dead CSS: every class a stylesheet styles is produced by the markup, and
//! every custom property a stylesheet reads is defined somewhere.
//!
//! Drift it prevents: rules left behind when a component is removed or
//! renamed (AGENTS.md: remove obsolete CSS with the UI), and a
//! `var(--name)` that resolves to nothing — a colour or size silently
//! dropped, typically after a theme token was renamed.
//!
//! The stylesheets are the `*_STYLES` constants of `oxidgene-ui`. A class
//! counts as produced when it appears as a token in the crate's code outside
//! those constants (an `rsx!` class string, a `format!`, the browser
//! scripts), or when a format string builds its family: `"tools-severity-{}"`
//! covers every `tools-severity-<word>`. A custom property counts as defined
//! when a stylesheet or inline style declares `--name:`, or a theme of
//! `assets/themes/` carries it as a colour.
//!
//! Fixing a failure: delete the rule, or the class from its selector list;
//! rename the `var()` to the token that exists. A family built some other
//! way goes in `DYNAMIC_FAMILIES` with where it is built.

use std::collections::BTreeSet;

use oxidgene_guards::{files, read, relative};

/// Class prefixes the code builds without a `prefix-{…}` literal.
const DYNAMIC_FAMILIES: &[&str] = &[];

struct Sheet {
    place: String,
    css: String,
}

/// The `*_STYLES` constants, and the code with them cut out.
fn sheets_and_code() -> (Vec<Sheet>, String) {
    let mut sheets = Vec::new();
    let mut code = String::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let source = std::fs::read_to_string(&path).unwrap();
        // Production code, without its comment lines: a comment naming a
        // class or a `var()` produces and reads nothing.
        let source: String = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default()
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .flat_map(|line| [line, "\n"])
            .collect();
        let mut rest = source.as_str();
        while let Some(at) = find_styles_const(rest) {
            code.push_str(&rest[..at]);
            let raw = &rest[at..];
            let open = raw.find("r#\"").expect("a raw string") + 3;
            let close = open + raw[open..].find("\"#").expect("the raw string ends");
            sheets.push(Sheet {
                place: relative(&path),
                css: raw[open..close].to_string(),
            });
            rest = &raw[close + 2..];
        }
        code.push_str(rest);
    }
    for path in files("crates/oxidgene-ui/src", "js") {
        code.push_str(&std::fs::read_to_string(path).unwrap());
    }
    (sheets, code)
}

/// The offset of the next `const …_STYLES: &str = r#"` in `source`.
fn find_styles_const(source: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(found) = source[search..].find("_STYLES: &str = r#\"") {
        let at = search + found;
        let start = source[..at].rfind("const ")?;
        if !source[start..at].contains('\n') {
            return Some(start);
        }
        search = at + 1;
    }
    None
}

/// The CSS without comments.
fn uncommented(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        rest = rest[open..]
            .find("*/")
            .map_or("", |close| &rest[open + close + 2..]);
    }
    out.push_str(rest);
    out
}

/// The class names a stylesheet's selectors name.
fn styled_classes(css: &str) -> BTreeSet<String> {
    let css = uncommented(css);
    let mut classes = BTreeSet::new();
    // Selectors are what precedes a `{` since the last `}`, `{` or `;`.
    for block in css.split('{') {
        let selector = block.rsplit(['}', ';']).next().unwrap_or_default();
        if selector.trim_start().starts_with('@') {
            continue;
        }
        let bytes: Vec<char> = selector.chars().collect();
        for (i, c) in bytes.iter().enumerate() {
            let starts_class = *c == '.'
                && bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_alphabetic() || *n == '_' || *n == '-')
                && !bytes
                    .get(i.wrapping_sub(1))
                    .is_some_and(char::is_ascii_digit);
            if starts_class {
                let name: String = bytes[i + 1..]
                    .iter()
                    .take_while(|n| n.is_ascii_alphanumeric() || **n == '-' || **n == '_')
                    .collect();
                classes.insert(name);
            }
        }
    }
    classes
}

fn tokens(code: &str) -> BTreeSet<&str> {
    code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .filter(|t| !t.is_empty())
        .collect()
}

/// `prefix-` of every `"…prefix-{…"` the code formats.
fn built_families(code: &str) -> BTreeSet<String> {
    let mut families: BTreeSet<String> = DYNAMIC_FAMILIES.iter().map(|f| f.to_string()).collect();
    for (at, _) in code.match_indices("-{") {
        let start = code[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .map_or(0, |i| i + 1);
        let prefix = &code[start..=at];
        if prefix.len() > 2 && prefix.starts_with(|c: char| c.is_ascii_alphabetic()) {
            families.insert(prefix.to_string());
        }
    }
    families
}

#[test]
fn every_styled_class_is_produced() {
    let (sheets, code) = sheets_and_code();
    assert!(sheets.len() >= 5, "the *_STYLES constants were not found");
    let produced = tokens(&code);
    let families = built_families(&code);
    let mut dead = Vec::new();
    for sheet in &sheets {
        for class in styled_classes(&sheet.css) {
            let used = produced.contains(class.as_str())
                || families
                    .iter()
                    .any(|family| class.starts_with(family.as_str()));
            if !used {
                dead.push(format!("{}: .{class}", sheet.place));
            }
        }
    }
    assert!(
        dead.is_empty(),
        "classes no markup produces:\n{}",
        dead.join("\n")
    );
}

#[test]
fn every_custom_property_read_is_defined() {
    let (sheets, code) = sheets_and_code();
    let mut text = code.clone();
    for sheet in &sheets {
        text.push_str(&sheet.css);
    }
    let mut defined = BTreeSet::new();
    for (at, _) in text.match_indices("--") {
        let name: String = text[at + 2..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !name.is_empty() && text[at + 2 + name.len()..].trim_start().starts_with(':') {
            defined.insert(name);
        }
    }
    for path in files("assets/themes", "json") {
        let theme = std::fs::read_to_string(path).unwrap();
        let colors = theme.split("\"colors\"").nth(1).unwrap_or_default();
        let colors = &colors[..colors.find('}').unwrap_or(colors.len())];
        for key in colors.split('"').skip(1).step_by(4) {
            defined.insert(key.to_string());
        }
    }
    let mut undefined = BTreeSet::new();
    for (at, _) in text.match_indices("var(--") {
        let name: String = text[at + 6..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        // `var(--{token})` is built from the theme's own keys.
        if !name.is_empty() && !defined.contains(&name) {
            undefined.insert(name);
        }
    }
    assert!(
        undefined.is_empty(),
        "custom properties read but never defined: {undefined:?}"
    );
    // The theme reader still emits the colours as properties.
    assert!(read("crates/oxidgene-ui/src/theme.rs").contains("out.push_str(\"    --\");"));
}

#[test]
fn selectors_are_read() {
    let classes =
        styled_classes(".a .b-c:hover, div.d > .e_f { x: 1.5em; } @media (x) { .g { } } /* .h */");
    assert_eq!(
        classes.into_iter().collect::<Vec<_>>(),
        ["a", "b-c", "d", "e_f", "g"]
    );
}
