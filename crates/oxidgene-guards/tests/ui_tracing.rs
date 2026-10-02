//! UI tracing coverage: every routed page opens its load trace, and every
//! resource it loads is a span of that trace.
//!
//! Drift it prevents: a new page whose requests reach the backend with no
//! trace of the screen that made them, and a plain `use_resource` whose
//! request is filed under no page at all (docs/cross-cutting.md, logs and
//! traces).
//!
//! Fixing a failure: give the page a `UiPage` variant and call
//! `use_ui_load_trace(UiPage::…)` at the top of its component; load data
//! with `use_traced_resource` (or `use_ui_resource` in a component below a
//! page). A resource that owns a trace of its own, like a user action's,
//! goes in `PLAIN_RESOURCES` with the reason.

use oxidgene_guards::{files, line_of, read, relative};

/// The `UiPage` of a route whose page is not named after it.
const PAGE_NAMES: &[(&str, &str)] = &[("TreeDetail", "Pedigree")];

/// Files allowed a plain `use_resource(`, with why.
const PLAIN_RESOURCES: &[(&str, &str)] = &[
    (
        "crates/oxidgene-ui/src/ui_observability.rs",
        "the traced wrappers themselves",
    ),
    (
        "crates/oxidgene-ui/src/components/import_modal.rs",
        "the homonym lookup after a Geneanet import is a step of that import's own action trace",
    ),
];

/// The variants of `enum name` in `source`.
fn variants(source: &str, name: &str) -> Vec<String> {
    let start = source
        .find(&format!("enum {name} {{"))
        .unwrap_or_else(|| panic!("enum {name} not found"));
    let body = &source[start..];
    let body = &body[body.find('{').unwrap() + 1..];
    let mut depth = 0;
    let mut found = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if depth == 0 && trimmed.starts_with('}') {
            break;
        }
        if depth == 0
            && trimmed.chars().next().is_some_and(char::is_uppercase)
            && !trimmed.starts_with("//")
        {
            let ident: String = trimmed
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            found.push(ident);
        }
        depth += line.matches('{').count();
        depth -= line.matches('}').count().min(depth);
    }
    found
}

#[test]
fn every_route_opens_the_load_trace_of_its_page() {
    let routes = variants(&read("crates/oxidgene-ui/src/router.rs"), "Route");
    assert!(routes.len() > 5, "the Route enum was not read: {routes:?}");
    let pages = variants(
        &read("crates/oxidgene-ui/src/ui_observability.rs"),
        "UiPage",
    );
    let sources: Vec<_> = files("crates/oxidgene-ui/src/pages", "rs")
        .into_iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect();
    let mut problems = Vec::new();
    for route in &routes {
        let page = PAGE_NAMES
            .iter()
            .find(|(r, _)| r == route)
            .map_or(route.as_str(), |(_, p)| p);
        if !pages.iter().any(|p| p == page) {
            problems.push(format!("Route::{route} has no UiPage::{page}"));
            continue;
        }
        let body = sources.iter().find_map(|source| {
            let at = source.find(&format!("pub fn {route}("))?;
            let rest = &source[at..];
            Some(&rest[..rest.find("\n}\n").unwrap_or(rest.len())])
        });
        match body {
            None => problems.push(format!("no `pub fn {route}(` component under pages/")),
            Some(body) if !body.contains(&format!("use_ui_load_trace(UiPage::{page})")) => {
                problems.push(format!(
                    "{route} does not call use_ui_load_trace(UiPage::{page})"
                ));
            }
            Some(_) => {}
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn resources_are_traced() {
    let mut plain = Vec::new();
    for path in files("crates/oxidgene-ui/src", "rs") {
        let name = relative(&path);
        if PLAIN_RESOURCES.iter().any(|(file, _)| *file == name) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for (at, _) in source.match_indices("use_resource(") {
            let before = source[..at].chars().last();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            plain.push(format!("{name}:{}", line_of(&source, at)));
        }
    }
    assert!(
        plain.is_empty(),
        "plain use_resource( — use use_traced_resource or use_ui_resource:\n{}",
        plain.join("\n")
    );
}
