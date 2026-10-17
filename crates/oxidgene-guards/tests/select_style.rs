//! Select styling: every `<select>` is drawn by the page, from the global
//! `select` rule of `components/layout.rs`, never by the engine.
//!
//! Drift it prevents: a select left to its native appearance takes the
//! engine's own metrics whatever its padding says — the desktop's WebKitGTK
//! draws one markedly taller than a text field — a bug that came back with
//! nearly every new form. The global rule drops the native appearance and
//! paints the arrow as a background image in the padding on the right; a
//! rule restyling a select then breaks it with the `background` shorthand,
//! which wipes the arrow, or with a `padding` that takes its room back, so
//! the text runs under it.
//!
//! The selects are the `select` element selectors of the `*_STYLES`
//! constants, and the classes the markup puts on a `select { … }` element.
//! The shared rule of every text field (`input…, select, textarea`) is the
//! one the global select rule follows, so it is not held to these rules.
//!
//! Fixing a failure: write `background-color` instead of `background`; give
//! a `padding` shorthand four values whose second, the right one, is
//! `var(--space-15)`, or set `padding-left`/`padding-top`/… alone; set
//! `appearance` only in the global rule.

use std::collections::BTreeSet;

use oxidgene_guards::{files, find_styles_const, relative, styles_at, uncommented};

/// The arrow's room, the right padding of the global select rule.
const ARROW_ROOM: &str = "var(--space-15)";

struct Rule {
    place: String,
    selector: String,
    body: String,
}

/// Every rule of every stylesheet, and the code outside them.
fn rules_and_code() -> (Vec<Rule>, String) {
    let mut rules = Vec::new();
    let mut code = String::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let source = std::fs::read_to_string(&path).unwrap();
        let source = source.split("#[cfg(test)]").next().unwrap_or_default();
        let mut rest = source;
        while let Some(at) = find_styles_const(rest) {
            code.push_str(&rest[..at]);
            let (css, end) = styles_at(rest, at);
            rules.extend(parse(&relative(&path), &uncommented(css)));
            rest = &rest[end..];
        }
        code.push_str(rest);
    }
    (rules, code)
}

/// The rules of a stylesheet: each selector with its declarations, the
/// at-rules' own blocks entered rather than taken for rules.
fn parse(place: &str, css: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut start = 0;
    for (at, c) in css.char_indices() {
        match c {
            '{' => {
                let selector = css[start..at].trim();
                if !selector.starts_with('@') {
                    let close = css[at..].find('}').map_or(css.len(), |close| at + close);
                    rules.push(Rule {
                        place: place.to_string(),
                        selector: selector.to_string(),
                        body: css[at + 1..close].to_string(),
                    });
                }
                start = at + 1;
            }
            '}' | ';' => start = at + 1,
            _ => {}
        }
    }
    rules
}

/// The classes the markup puts on a `select { … }` element: the `class`
/// attribute among its attributes, before its first child.
fn select_classes(code: &str) -> BTreeSet<String> {
    let mut classes = BTreeSet::new();
    for (at, _) in code.match_indices("select {") {
        let preceded_by_word = code[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.');
        if preceded_by_word {
            continue;
        }
        let attributes = &code[at..];
        let end = attributes.find("option").unwrap_or(attributes.len());
        if let Some(class) = attributes[..end].find("class: \"") {
            let value = &attributes[class + 8..];
            let value = &value[..value.find('"').unwrap_or(value.len())];
            classes.extend(
                value
                    .split_whitespace()
                    .filter(|name| !name.contains('{'))
                    .map(str::to_string),
            );
        }
    }
    classes
}

/// Whether one selector of a list names a select: as its element, or by a
/// class the markup puts on one.
fn names_select(selector: &str, classes: &BTreeSet<String>) -> bool {
    let subject = selector.split_whitespace().last().unwrap_or_default();
    let element = subject.split([':', '.', '[']).next().unwrap_or_default();
    element == "select"
        || subject
            .split('.')
            .skip(1)
            .map(|class| class.split([':', '[']).next().unwrap_or_default())
            .any(|class| classes.contains(class))
}

/// The value of each declaration of `property` in `body`.
fn declarations<'a>(body: &'a str, property: &str) -> Vec<&'a str> {
    body.split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            (name.trim() == property).then(|| value.trim())
        })
        .collect()
}

/// What is wrong with a rule styling a select.
fn faults(rule: &Rule) -> Vec<String> {
    let mut faults = Vec::new();
    if !declarations(&rule.body, "background").is_empty() {
        faults.push("the `background` shorthand wipes the arrow: use `background-color`".into());
    }
    for padding in declarations(&rule.body, "padding") {
        let values: Vec<&str> = padding.split_whitespace().collect();
        if values.len() != 4 || values[1] != ARROW_ROOM {
            faults.push(format!(
                "`padding: {padding}` takes back the arrow's room: give four values, the right one {ARROW_ROOM}"
            ));
        }
    }
    faults
}

#[test]
fn every_select_keeps_the_global_arrow_and_box() {
    let (rules, code) = rules_and_code();
    let classes = select_classes(&code);
    let mut broken = Vec::new();
    for rule in &rules {
        let selectors: Vec<&str> = rule.selector.split(',').map(str::trim).collect();
        let shared_field_rule = selectors
            .iter()
            .any(|s| s.starts_with("input") || s.starts_with("textarea"));
        let global = selectors == ["select"];
        if global || shared_field_rule || !selectors.iter().any(|s| names_select(s, &classes)) {
            continue;
        }
        for fault in faults(rule) {
            broken.push(format!("{}: `{}`: {fault}", rule.place, rule.selector));
        }
        if !declarations(&rule.body, "appearance").is_empty() {
            broken.push(format!(
                "{}: `{}`: only the global select rule sets `appearance`",
                rule.place, rule.selector
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "selects restyled against the global rule:\n{}",
        broken.join("\n")
    );
}

#[test]
fn the_global_rule_draws_every_select() {
    let (rules, _) = rules_and_code();
    let global: Vec<&Rule> = rules
        .iter()
        .filter(|rule| rule.selector == "select")
        .collect();
    assert_eq!(global.len(), 1, "one global `select` rule");
    let body = &global[0].body;
    assert_eq!(declarations(body, "appearance"), ["none"]);
    assert_eq!(declarations(body, "padding-right"), [ARROW_ROOM]);
    assert_eq!(
        declarations(body, "background-image"),
        ["var(--select-arrow)"]
    );
    // It follows the shared field rule, whose `background` and `padding`
    // shorthands it then completes.
    let field = rules
        .iter()
        .position(|rule| rule.selector.starts_with("input") && rule.selector.contains("select"))
        .expect("the shared field rule");
    let select = rules
        .iter()
        .position(|rule| rule.selector == "select")
        .unwrap();
    assert!(
        field < select,
        "the global select rule follows the shared field rule"
    );
}

#[test]
fn the_reader_finds_selects_and_their_faults() {
    let rules = parse(
        "sheet",
        "@media (x) { .a select { background: red; padding: 1px 2px; } }\n.b { color: red; }",
    );
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].selector, ".a select");
    assert_eq!(faults(&rules[0]).len(), 2);
    let classes = select_classes("rsx! { select {\n class: \"sort-select wide\",\n option {} } }");
    assert!(classes.contains("sort-select"));
    assert!(names_select(".bar .sort-select:focus", &classes));
    assert!(!names_select(".homonym-select", &classes));
}
