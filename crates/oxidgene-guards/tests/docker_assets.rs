//! Docker images and embedded assets: every file under `assets/` that a
//! crate of an image's binary embeds or reads at build time is copied into
//! that image's builder stage.
//!
//! Drift it prevents: a crate starts embedding a new asset
//! (`include_str!("../../../../assets/mail/en.json")`, a build script
//! reading `assets/citations`), every local build and test passes — the
//! checkout has the file — and the image build fails on `couldn't read
//! assets/…`, which only the release workflow and `just e2e-auth` build.
//!
//! Fixing a failure: add `COPY assets/<dir>/ assets/<dir>/` (or the single
//! file) to the Dockerfile named, next to the other assets its crates embed.

use std::collections::BTreeSet;
use std::path::Path;

use oxidgene_guards::{files, read, relative, root};

/// Each image: its Dockerfile and the workspace package it builds.
const IMAGES: &[(&str, &str)] = &[
    ("docker/Dockerfile.server", "oxidgene-server"),
    ("docker/Dockerfile.worker", "oxidgene-worker"),
    ("docker/Dockerfile.web", "oxidgene-web"),
];

/// The directory of workspace package `name`.
fn package_dir(name: &str) -> String {
    ["crates", "apps"]
        .iter()
        .map(|parent| format!("{parent}/{name}"))
        .find(|dir| root().join(dir).join("Cargo.toml").is_file())
        .unwrap_or_else(|| panic!("no workspace package {name}"))
}

/// `name` and every workspace package it depends on, optional or not: an
/// image may enable any feature, so all are counted.
fn closure(name: &str) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![name.to_string()];
    while let Some(package) = pending.pop() {
        if !seen.insert(package.clone()) {
            continue;
        }
        let manifest = read(format!("{}/Cargo.toml", package_dir(&package)));
        for line in manifest.lines() {
            let line = line.trim_start();
            if let Some(dependency) = line.strip_prefix("oxidgene-")
                && let Some(rest) = dependency.split_once([' ', '='])
            {
                let dependency = format!("oxidgene-{}", rest.0.trim());
                if !seen.contains(&dependency) {
                    pending.push(dependency);
                }
            }
        }
    }
    seen
}

/// Whether `path` is test code, which no image builds.
fn is_test(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_test.rs")
        || path.components().any(|c| c.as_os_str() == "tests")
}

/// The asset paths a source names in its string literals: what follows
/// `assets/` up to the closing quote, `assets/mail/en.json` or
/// `assets/reference`. Comments are skipped, and so is the rest of a file
/// from its `#[cfg(test)]` module on.
fn assets_named(source: &str) -> BTreeSet<String> {
    let mut named = BTreeSet::new();
    for line in source.lines() {
        let code = line.trim_start();
        if code.starts_with("#[cfg(test)]") {
            break;
        }
        if code.starts_with("//") {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("assets/") {
            let tail = &rest[at..];
            let end = tail.find(['"', ' ', ')']).unwrap_or(tail.len());
            let path = tail[..end].trim_end_matches('/');
            // `"assets/"` alone, or a path built further on, names nothing
            // this guard can check.
            if path.len() > "assets/".len() {
                named.insert(path.to_string());
            }
            rest = &tail["assets/".len()..];
        }
    }
    named
}

/// The `assets/…` sources a Dockerfile copies.
fn copied(dockerfile: &str) -> Vec<String> {
    read(dockerfile)
        .lines()
        .filter_map(|line| line.trim().strip_prefix("COPY "))
        .filter_map(|args| args.split_whitespace().next())
        .filter(|source| source.starts_with("assets/"))
        .map(|source| source.trim_end_matches('/').to_string())
        .collect()
}

#[test]
fn every_embedded_asset_is_copied_into_its_image() {
    let mut missing = Vec::new();
    for (dockerfile, package) in IMAGES {
        let copies = copied(dockerfile);
        for crate_name in closure(package) {
            let dir = package_dir(&crate_name);
            let mut sources = files(format!("{dir}/src"), "rs");
            let build = root().join(&dir).join("build.rs");
            if build.is_file() {
                sources.push(build);
            }
            for source in sources.iter().filter(|path| !is_test(path)) {
                let text = std::fs::read_to_string(source).expect("a readable source");
                for asset in assets_named(&text) {
                    let covered = copies
                        .iter()
                        .any(|copy| asset == *copy || asset.starts_with(&format!("{copy}/")));
                    if !covered {
                        missing.push(format!(
                            "{dockerfile} does not copy {asset}, which {} names",
                            relative(source)
                        ));
                    }
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn the_guard_reads_the_assets_a_source_names() {
    let source = r#"
//! Reads `assets/ignored/` in a comment.
const A: &str = include_str!("../../../../assets/mail/en.json");
let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/reference");
#[cfg(test)]
mod tests { const B: &str = include_str!("../../assets/test-only/x.json"); }
"#;
    let named: Vec<String> = assets_named(source).into_iter().collect();
    assert_eq!(named, ["assets/mail/en.json", "assets/reference"]);
}
