//! Test targets: every crate builds one integration test binary at most, and
//! that binary declares every file of its `tests/` directory.
//!
//! Drift it prevents: a new `tests/*.rs` file in a crate that leaves Cargo's
//! test discovery on, which Cargo builds as an executable of its own, linking
//! the crate and all its dependencies again (a binary of `oxidgene-api` is
//! about 200 MB, and CI links each one on every change to the crate); and, in
//! a crate that turned discovery off, a file of `tests/` no module declares,
//! which then never compiles nor runs while every test still passes.
//!
//! Fixing a failure: fold the crate's test files into one binary. Set
//! `autotests = false` under `[package]`, declare one `[[test]]` named
//! `integration` with `path = "tests/integration.rs"`, and declare every
//! other file of `tests/` there as a module (`mod rest_test;`), replacing a
//! file's own `mod common;` with `use crate::common;`. In a crate already
//! folded, add the missing `mod` line to its root.

use std::collections::BTreeSet;
use std::path::Path;

use oxidgene_guards::{read, relative, root, workspace_members};

/// The module names a source declares from a file: `mod name;`, with any
/// visibility and any attribute written before it on the same line.
fn declared_modules(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let mut rest = line.trim();
            while rest.starts_with("#[") {
                rest = rest[rest.find(']')? + 1..].trim_start();
            }
            for visibility in ["pub(crate) ", "pub(super) ", "pub "] {
                rest = rest.strip_prefix(visibility).unwrap_or(rest);
            }
            let name = rest.strip_prefix("mod ")?.strip_suffix(';')?.trim();
            Some(name.to_string())
        })
        .collect()
}

/// The `path` of every `[[test]]` table of a manifest, `None` for a table
/// that names none.
fn declared_tests(manifest: &str) -> Vec<Option<String>> {
    manifest
        .split("[[test]]")
        .skip(1)
        .map(|table| {
            let table = table.split("\n[").next().unwrap_or(table);
            table.lines().find_map(|line| {
                let value = line.trim().strip_prefix("path")?.trim_start();
                let value = value.strip_prefix('=')?.trim();
                Some(value.trim_matches('"').to_string())
            })
        })
        .collect()
}

/// Whether a manifest turns Cargo's test discovery off.
fn discovery_off(manifest: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.replace(' ', "") == "autotests=false")
}

/// The top-level `.rs` files of `tests/` by stem, and its directories holding
/// a `main.rs` and a `mod.rs`.
struct TestsDir {
    files: BTreeSet<String>,
    with_main: BTreeSet<String>,
    with_mod: BTreeSet<String>,
}

fn tests_dir(dir: &Path) -> TestsDir {
    let mut found = TestsDir {
        files: BTreeSet::new(),
        with_main: BTreeSet::new(),
        with_mod: BTreeSet::new(),
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if path.join("main.rs").is_file() {
                found.with_main.insert(name.clone());
            }
            if path.join("mod.rs").is_file() {
                found.with_mod.insert(name);
            }
        } else if let Some(stem) = name.strip_suffix(".rs") {
            found.files.insert(stem.to_string());
        }
    }
    found
}

/// What is wrong with one member's integration tests.
fn member_problems(member: &str) -> Vec<String> {
    let manifest = read(format!("{member}/Cargo.toml"));
    let tests = root().join(member).join("tests");
    let found = tests_dir(&tests);
    if !discovery_off(&manifest) {
        let targets = found.files.len() + found.with_main.len();
        return if targets > 1 {
            vec![format!(
                "{member}: {targets} integration test binaries; fold them into one \
                 (`autotests = false`, one `[[test]]` with path `tests/integration.rs` \
                 declaring the others as modules)"
            )]
        } else {
            Vec::new()
        };
    }
    let declared = declared_tests(&manifest);
    match declared.as_slice() {
        [] => Vec::new(),
        [Some(path)] => undeclared_files(member, &tests, path, &found),
        [None] => vec![format!(
            "{member}: its `[[test]]` names no path; give it `path = \"tests/integration.rs\"`"
        )],
        _ => vec![format!(
            "{member}: {} `[[test]]` targets; fold them into one binary",
            declared.len()
        )],
    }
}

/// The files of `tests/` the root at `path` does not declare as modules.
fn undeclared_files(member: &str, tests: &Path, path: &str, found: &TestsDir) -> Vec<String> {
    let root_file = root().join(member).join(path);
    let Ok(source) = std::fs::read_to_string(&root_file) else {
        return vec![format!(
            "{member}: its `[[test]]` path `{path}` does not exist"
        )];
    };
    let modules = declared_modules(&source);
    let root_stem = root_file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    found
        .files
        .iter()
        .filter(|stem| root_file.parent() != Some(tests) || Some(*stem) != root_stem.as_ref())
        .chain(&found.with_mod)
        .filter(|name| !modules.contains(*name))
        .map(|name| {
            format!(
                "{}: `{name}` is never declared, so it neither compiles nor runs; \
                 add `mod {name};`",
                relative(&root_file)
            )
        })
        .collect()
}

#[test]
fn every_crate_builds_one_integration_test_binary_that_declares_its_files() {
    let violations: Vec<String> = workspace_members()
        .iter()
        .flat_map(|member| member_problems(member))
        .collect();
    assert!(
        violations.is_empty(),
        "integration test targets:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_readers_find_modules_and_test_paths() {
    let modules = declared_modules(
        "//! mod commented;\nmod plain;\n#[cfg(feature = \"x\")] mod gated;\n\
         pub(crate) mod shared;\nmod inline {\n}\n    mod indented;\n",
    );
    let expected = ["gated", "indented", "plain", "shared"];
    assert_eq!(modules, expected.iter().map(ToString::to_string).collect());

    let manifest = "[package]\nautotests = false\n\n[[test]]\nname = \"a\"\npath = \"tests/a.rs\"\n\n\
                    [[test]]\nname = \"b\"\n\n[dev-dependencies]\npath = \"no\"\n";
    assert!(discovery_off(manifest));
    assert_eq!(
        declared_tests(manifest),
        vec![Some("tests/a.rs".to_string()), None]
    );
    assert!(!discovery_off("[package]\nname = \"x\"\n"));
}
