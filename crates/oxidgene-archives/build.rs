//! Embeds the archive catalogue: every `assets/archives/<country>/*.json`
//! document, with the directory it was found in, so that adding an archive is
//! a data change; and the citation vocabularies, every
//! `assets/citations/<language>.json`, so that adding a language is one too.
//! The documents weigh a few kilobytes and stay plain text.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let catalogue = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/archives")
        .canonicalize()
        .expect("the assets/archives directory");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let mut documents = Vec::new();
    for country in sorted_entries(&catalogue).filter(|path| path.is_dir()) {
        let directory = country.file_name().unwrap().to_string_lossy().into_owned();
        for file in sorted_entries(&country).filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        }) {
            documents.push(format!("({directory:?}, include_str!({file:?}))"));
        }
    }
    fs::write(
        output.join("archives.rs"),
        format!("&[{}]", documents.join(",")),
    )
    .expect("embedded archive list");

    let vocabularies = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../../assets/citations")
        .canonicalize()
        .expect("the assets/citations directory");
    let documents: Vec<String> = sorted_entries(&vocabularies)
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|file| format!("include_str!({file:?})"))
        .collect();
    fs::write(
        output.join("vocabularies.rs"),
        format!("&[{}]", documents.join(",")),
    )
    .expect("embedded vocabulary list");
}

/// The entries of `directory`, sorted by name; Cargo rebuilds when one is
/// added or removed.
fn sorted_entries(directory: &Path) -> impl Iterator<Item = PathBuf> {
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("archive catalogue entry").path())
        .collect();
    entries.sort();
    entries.into_iter()
}
