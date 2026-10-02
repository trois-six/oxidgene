//! Layering of `oxidgene-api`: the surfaces call services, services never
//! call the surfaces.
//!
//! Drift it prevents: a REST handler or a GraphQL mutation writing through a
//! repository or opening its own transaction — the business rule (history,
//! projection refresh, validation) then lives in one surface only, and the
//! other drifts away from it; and a service importing `crate::rest` or
//! `crate::graphql`, which ties the shared logic to one surface's types.
//!
//! Fixing a failure: move the write, with its transaction, into a function
//! of `service/` (or `profile/` for projections) that both surfaces call. A
//! new read of a repository from a surface is fine: add its method name to
//! `READS` when it matches none of the read prefixes.

use oxidgene_guards::{files, line_of, read, relative, without_test_modules};

/// Repository methods a surface may call: reads. Every other `…Repo::name`
/// call from a surface is a write that belongs in a service.
const READ_PREFIXES: &[&str] = &[
    "get", "list", "find", "count", "exists", "resolve", "search", "load", "fetch",
];

/// Reads whose names start with none of the prefixes above.
const READS: &[&str] = &[
    "occupations",
    "family_names",
    "places_with_usage",
    "source_usage_person_ids",
    "place_usage_person_ids",
    "occupation_usage_person_ids",
    "family_name_usage_person_ids",
];

/// What opens a transaction.
const TRANSACTIONS: &[&str] = &["begin_tx(", "commit_tx(", ".begin()", ".commit()"];

#[test]
fn services_do_not_depend_on_the_surfaces() {
    let mut violations = Vec::new();
    for dir in ["service", "profile"] {
        for path in files(format!("crates/oxidgene-api/src/{dir}"), "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            for needle in [
                "crate::rest",
                "crate::graphql",
                "super::rest",
                "super::graphql",
            ] {
                for (at, _) in source.match_indices(needle) {
                    violations.push(format!(
                        "{}:{}: {needle}",
                        relative(&path),
                        line_of(&source, at)
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "service/ and profile/ must not import a surface:\n{}",
        violations.join("\n")
    );
}

#[test]
fn surfaces_write_through_services() {
    let mut surfaces = files("crates/oxidgene-api/src/rest", "rs");
    surfaces.push(oxidgene_guards::root().join("crates/oxidgene-api/src/graphql/mutation.rs"));
    let mut violations = Vec::new();
    for path in surfaces {
        let full = std::fs::read_to_string(&path).unwrap();
        let source = without_test_modules(&full);
        for (at, call) in repository_calls(source) {
            if !is_read(call) {
                violations.push(format!(
                    "{}:{}: repository write {call}",
                    relative(&path),
                    line_of(source, at)
                ));
            }
        }
        for needle in TRANSACTIONS {
            for (at, _) in source.match_indices(needle) {
                violations.push(format!(
                    "{}:{}: transaction {needle}",
                    relative(&path),
                    line_of(source, at)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "REST handlers and GraphQL mutations must write through a service:\n{}",
        violations.join("\n")
    );
}

/// Every `SomethingRepo::method` in `source`, with its offset.
fn repository_calls(source: &str) -> Vec<(usize, &str)> {
    let mut calls = Vec::new();
    for (at, _) in source.match_indices("Repo::") {
        let method_start = at + "Repo::".len();
        let method_len = source[method_start..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(0);
        let type_start = source[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        let is_type = source[type_start..at]
            .chars()
            .next()
            .is_some_and(char::is_uppercase);
        if is_type && method_len > 0 {
            calls.push((type_start, &source[method_start..method_start + method_len]));
        }
    }
    calls
}

fn is_read(method: &str) -> bool {
    READ_PREFIXES
        .iter()
        .any(|prefix| method.starts_with(prefix))
        || READS.contains(&method)
}

#[test]
fn the_scanner_sees_a_write() {
    let source = "let x = PersonRepo::create(db).await; let y = TreeRepo::get(db);";
    let calls = repository_calls(source);
    assert_eq!(
        calls.iter().map(|(_, m)| *m).collect::<Vec<_>>(),
        ["create", "get"]
    );
    assert!(!is_read("create"));
    assert!(is_read("get"));
    // The router is where both surfaces meet; it must exist for the scan to
    // mean anything.
    assert!(read("crates/oxidgene-api/src/router.rs").contains("fn build_router"));
}
