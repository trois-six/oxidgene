//! Repository guards: tests that read the workspace's sources, manifests and
//! specifications and fail when they drift from the rules of `AGENTS.md`.
//!
//! The tests live under `tests/`, one file per rule; this library holds what
//! they share. Nothing here is linked into a product binary. Each guard's
//! module documentation says what drift it prevents and how to fix a
//! failure; docs/development.md lists them with their tier and CI job.

use std::path::{Path, PathBuf};

/// The repository root.
#[must_use]
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root exists")
}

/// A file of the repository, read whole.
///
/// # Panics
///
/// When the file cannot be read: a guard reading a file that moved must be
/// updated, not silently pass.
#[must_use]
pub fn read(relative: impl AsRef<Path>) -> String {
    let path = root().join(relative.as_ref());
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The path of `path` relative to the repository root, for messages.
#[must_use]
pub fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Every file under `dir` (relative to the root) with extension `ext`,
/// sorted, skipping build output and dependencies.
#[must_use]
pub fn files(dir: impl AsRef<Path>, ext: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(&root().join(dir.as_ref()), ext, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, ext: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(
                name.to_str(),
                Some("target" | "node_modules" | "fuzz" | ".git")
            ) {
                walk(&path, ext, found);
            }
        } else if path.extension().is_some_and(|e| e == ext) {
            found.push(path);
        }
    }
}

/// Every workspace member's directory, read from the root manifest.
#[must_use]
pub fn workspace_members() -> Vec<String> {
    let manifest = read("Cargo.toml");
    let start = manifest
        .find("members = [")
        .expect("the root manifest lists its members");
    let end = start + manifest[start..].find(']').expect("the list ends");
    manifest[start..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// `source` without its `#[cfg(test)]` modules, so a guard about production
/// code does not trip on a test fixture. A test module is cut from its
/// attribute to the end of the file, where this codebase keeps them.
#[must_use]
pub fn without_test_modules(source: &str) -> &str {
    let mut search = 0;
    while let Some(found) = source[search..].find("#[cfg(test)]") {
        let at = search + found;
        let after = source[at + "#[cfg(test)]".len()..].trim_start();
        if after.starts_with("mod ") || after.starts_with("pub(crate) mod ") {
            return &source[..at];
        }
        search = at + 1;
    }
    source
}

/// The 1-based line number of byte `offset` in `source`.
#[must_use]
pub fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

/// The recipe names of the root `justfile`.
#[must_use]
pub fn just_recipes() -> Vec<String> {
    read("justfile")
        .lines()
        .filter(|line| {
            line.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !line.contains(":=")
        })
        .filter_map(|line| {
            let name = line.split([' ', ':']).next()?;
            line.contains(':').then(|| name.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modules_are_cut_at_their_attribute() {
        let source =
            "fn a() {}\n#[cfg(test)]\nfn helper() {}\n#[cfg(test)]\nmod tests { fn b() {} }\n";
        assert_eq!(
            without_test_modules(source),
            "fn a() {}\n#[cfg(test)]\nfn helper() {}\n"
        );
    }

    #[test]
    fn the_justfile_recipes_are_read() {
        let recipes = just_recipes();
        assert!(recipes.contains(&"check".to_string()));
        assert!(recipes.contains(&"e2e".to_string()));
        assert!(!recipes.iter().any(|r| r.starts_with("e2e_")));
    }
}
