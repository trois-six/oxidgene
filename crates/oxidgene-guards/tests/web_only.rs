//! Web-only features: the desktop compiles neither authentication nor
//! GraphQL (docs/authentication.md §7, docs/api.md).
//!
//! Drift it prevents: a manifest change — the desktop's, or that of a crate
//! it depends on — that enables `oxidgene-api`'s `auth` or `graphql`
//! feature in the desktop's build, linking sign-in, sessions, accounts and
//! their dependencies (`jsonwebtoken`, `redis`) into an application that
//! must keep its one local user and its loopback guards unchanged. Cargo
//! unifies features across a build, so the desktop's own manifest is not
//! enough to read: the guard asks Cargo for the resolved graph.
//!
//! Fixing a failure: enable the feature in `oxidgene-server` or
//! `oxidgene-worker` only, and inject a desktop capability through a UI
//! trait instead, as `oxidgene-ui` does for the desktop-only ones.

use std::process::Command;

/// The features of `oxidgene-api` the desktop must not build.
const WEB_ONLY_FEATURES: &[&str] = &["auth", "graphql"];

/// Crates only those features link.
const WEB_ONLY_CRATES: &[&str] = &["jsonwebtoken", "redis", "async-graphql"];

/// `cargo tree` of `package` with its features, for every target.
fn tree(package: &str) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .current_dir(oxidgene_guards::root())
        .args([
            "tree",
            "--locked",
            "--offline",
            "--package",
            package,
            "--edges",
            "normal,features",
            "--target",
            "all",
            "--prefix",
            "none",
        ])
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8")
}

#[test]
fn the_desktop_builds_no_web_only_feature() {
    let desktop = tree("oxidgene-desktop");
    let mut problems = Vec::new();
    for feature in WEB_ONLY_FEATURES {
        if desktop.contains(&format!("oxidgene-api feature \"{feature}\"")) {
            problems.push(format!("oxidgene-api's `{feature}` feature"));
        }
    }
    for krate in WEB_ONLY_CRATES {
        if desktop
            .lines()
            .any(|line| line.starts_with(&format!("{krate} v")))
        {
            problems.push(format!("the `{krate}` crate"));
        }
    }
    assert!(
        problems.is_empty(),
        "the desktop's build includes {}",
        problems.join(", ")
    );
}

#[test]
fn the_server_builds_the_web_features() {
    // The check above means something only if the names it looks for are
    // the ones Cargo prints for the build that does enable them.
    let server = tree("oxidgene-server");
    for feature in WEB_ONLY_FEATURES {
        assert!(
            server.contains(&format!("oxidgene-api feature \"{feature}\"")),
            "the server's build lacks oxidgene-api's `{feature}` feature"
        );
    }
    assert!(
        server
            .lines()
            .any(|line| line.starts_with("jsonwebtoken v"))
    );
}
