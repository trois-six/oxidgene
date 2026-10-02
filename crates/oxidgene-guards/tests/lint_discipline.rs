//! Lint discipline: every crate takes the workspace lints, no crate lowers
//! them wholesale, and every opt-in test says how to run it.
//!
//! Drift it prevents: a crate added without `[lints] workspace = true` (it
//! would build without the cognitive complexity limit or the ban on
//! `#[allow]`), a crate-wide `#![allow(…)]` or `--cap-lints` that hides
//! warnings, the `allow_attributes` bans removed from the root manifest, and
//! an `#[ignore]`d test nobody can find the command for.
//!
//! Fixing a failure: add `[lints]\nworkspace = true` to the crate's
//! manifest; replace a crate-wide allow with an `#[expect(lint, reason =
//! "…")]` on the one item that needs it; give the ignored test a reason
//! naming the `just` recipe that runs it (`#[ignore = "…: run by `just
//! bench`"]`), adding a recipe when none fits.

use oxidgene_guards::{files, just_recipes, line_of, read, relative, root, workspace_members};

#[test]
fn every_member_takes_the_workspace_lints() {
    let mut missing = Vec::new();
    for member in workspace_members() {
        let manifest = read(format!("{member}/Cargo.toml"));
        let opted_in = manifest
            .split("[lints]")
            .nth(1)
            .is_some_and(|rest| rest.trim_start().starts_with("workspace = true"));
        if !opted_in {
            missing.push(format!("{member}: no `[lints] workspace = true`"));
        }
        for table in ["[lints.rust]", "[lints.clippy]", "[lints.rustdoc]"] {
            if manifest.contains(table) {
                missing.push(format!(
                    "{member}: overrides the workspace lints with {table}"
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn the_workspace_bans_allow_attributes() {
    let manifest = read("Cargo.toml");
    for lint in [
        "allow_attributes = \"deny\"",
        "allow_attributes_without_reason = \"deny\"",
        "cognitive_complexity = \"warn\"",
    ] {
        assert!(
            manifest.contains(lint),
            "the root manifest's [workspace.lints.clippy] lost `{lint}`"
        );
    }
}

#[test]
fn no_crate_lowers_its_lints_wholesale() {
    let mut roots = Vec::new();
    for member in workspace_members() {
        for file in ["src/lib.rs", "src/main.rs", "build.rs"] {
            let path = root().join(&member).join(file);
            if path.exists() {
                roots.push(path);
            }
        }
        roots.extend(files(format!("{member}/src/bin"), "rs"));
    }
    let mut violations = Vec::new();
    for path in roots {
        let source = std::fs::read_to_string(&path).unwrap();
        for needle in ["#![allow(", "#![expect(", "#![cap_lints"] {
            for (at, _) in source.match_indices(needle) {
                violations.push(format!(
                    "{}:{}: {needle}",
                    relative(&path),
                    line_of(&source, at)
                ));
            }
        }
    }
    for config in [".cargo/config.toml", ".cargo/config"] {
        let path = root().join(config);
        if let Ok(text) = std::fs::read_to_string(&path) {
            for needle in ["cap-lints", "\"-A", "\"-Aclippy", "--allow"] {
                if text.contains(needle) {
                    violations.push(format!("{config}: {needle}"));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "crate-wide lint exceptions:\n{}",
        violations.join("\n")
    );
}

#[test]
fn every_ignored_test_names_the_recipe_that_runs_it() {
    let recipes = just_recipes();
    let mut violations = Vec::new();
    for dir in ["crates", "apps"] {
        for path in files(dir, "rs") {
            let source = std::fs::read_to_string(&path).unwrap();
            for (at, _) in source.match_indices("#[ignore") {
                // An attribute, not a mention in a comment or a string.
                let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
                if !source[line_start..at].trim().is_empty() {
                    continue;
                }
                let attribute =
                    &source[at..source[at..].find(']').map_or(source.len(), |e| at + e)];
                let recipe = attribute
                    .split("`just ")
                    .nth(1)
                    .and_then(|rest| rest.split(['`', ' ']).next());
                let problem = match recipe {
                    None => "names no `just <recipe>`".to_string(),
                    Some(name) if !recipes.iter().any(|r| r == name) => {
                        format!("names `just {name}`, which the justfile lacks")
                    }
                    Some(_) => continue,
                };
                violations.push(format!(
                    "{}:{}: {problem}",
                    relative(&path),
                    line_of(&source, at)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "ignored tests must name an existing recipe:\n{}",
        violations.join("\n")
    );
}
