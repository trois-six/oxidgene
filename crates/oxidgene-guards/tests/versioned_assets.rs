//! Versioned assets (docs/architecture.md §4.3): every asset is named by an
//! address that changes with its content and is kept a year, and only the
//! page, which names the others, and the deployment's configuration keep a
//! stable name.
//!
//! Drift it prevents: a file added to `assets/public/` at a stable name,
//! which the immutable `/pdfjs/` rule — or any rule like it — would then keep
//! a year after a release changed it; nginx's page cached by heuristics and
//! naming a bundle that is gone; and a route of the API or a handler of the
//! desktop answering a picture with a lifetime under a stable name, or
//! revalidating it, where its address should carry its version.
//!
//! Fixing a failure: put a new public file under a directory named after
//! its version, as pdf.js is; give a new picture route a `v` and answer it
//! with `rest::caching::versioned` (immutable at its version, a redirect to
//! it otherwise); keep nginx's rules as §4.3 lists them.

use oxidgene_guards::{files, read, relative, root, without_test_modules};

const NGINX: &str = "docker/nginx-web.conf";

/// The `Cache-Control` nginx sends in the location that starts with `head`.
fn nginx_rule(head: &str) -> String {
    let conf = read(NGINX);
    let start = conf
        .find(head)
        .unwrap_or_else(|| panic!("{NGINX} has no `{head}`"));
    let block = &conf[start..];
    let block = &block[..block.find("\n        }").expect("a closed location")];
    let rule = block
        .split("add_header Cache-Control \"")
        .nth(1)
        .unwrap_or_else(|| panic!("{NGINX}: `{head}` sends no Cache-Control"));
    rule[..rule.find('"').expect("a closed value")].to_string()
}

#[test]
fn nginx_keeps_the_versioned_files_and_revalidates_the_page() {
    const IMMUTABLE: &str = "public, max-age=31536000, immutable";
    assert_eq!(nginx_rule("location /assets/ {"), IMMUTABLE);
    assert_eq!(nginx_rule("location /pdfjs/ {"), IMMUTABLE);
    assert_eq!(nginx_rule("location / {"), "no-cache");
    assert_eq!(nginx_rule("location = /runtime-config.js {"), "no-store");
}

#[test]
fn every_public_file_is_under_its_release() {
    let public = root().join("assets/public");
    let mut stray = Vec::new();
    let mut found = 0;
    for entry in walk(&public) {
        found += 1;
        let path = relative(&entry);
        let inside = path.strip_prefix("assets/public/").unwrap_or(&path);
        let mut parts = inside.split('/');
        let versioned = match (parts.next(), parts.next(), parts.next()) {
            // The record of the vendored release, which the build reads.
            (Some("pdfjs"), Some("VERSION"), None) => true,
            (Some("pdfjs"), Some(release), Some(_)) => is_release(release),
            _ => false,
        };
        if !versioned {
            stray.push(path);
        }
    }
    assert!(found > 0, "assets/public/ was not found");
    assert!(
        stray.is_empty(),
        "public files outside a release directory: {stray:?}"
    );
}

/// Whether `name` is a release number, `6.4.299`.
fn is_release(name: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// Handlers that answer a document at a published, stable address rather
/// than an asset the application loads: the API description, which API
/// clients read by its name and App Settings links, revalidated on each use.
const STABLE_DOCUMENTS: &[&str] = &["crates/oxidgene-api/src/rest/openapi.rs"];

/// The code that answers assets: the API's REST handlers and the desktop's
/// asset handlers.
fn serving_sources() -> Vec<std::path::PathBuf> {
    let mut sources: Vec<_> = files("crates/oxidgene-api/src/rest", "rs")
        .into_iter()
        .filter(|path| !STABLE_DOCUMENTS.contains(&relative(path).as_str()))
        .collect();
    for handler in ["media_assets.rs", "pdfjs_assets.rs"] {
        sources.push(root().join("apps/oxidgene-desktop/src").join(handler));
    }
    sources
}

#[test]
fn no_asset_is_served_with_a_lifetime_or_revalidated() {
    let mut found = Vec::new();
    for path in serving_sources() {
        let source = read(&path);
        let code = without_test_modules(&source);
        for (number, line) in code.lines().enumerate() {
            let lifetime = line.contains("max-age=") && !line.contains("max-age=31536000");
            let revalidated = line.contains("no-cache")
                || line.contains("IF_NONE_MATCH")
                || line.contains("If-None-Match")
                || line.contains("ETAG");
            if lifetime || revalidated {
                found.push(format!(
                    "{}:{}: {}",
                    relative(&path),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "an asset served under a stable name with a lifetime, or revalidated: {found:#?}"
    );
}
