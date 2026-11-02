//! Vendored pdf.js: the files under `assets/public/pdfjs/` are one release,
//! the one `VERSION` names, its legacy build, with what the viewer loads, in
//! the directory named after the release, which versions their addresses
//! (docs/architecture.md §4.3).
//!
//! Drift it prevents: a file updated by hand or left over from another
//! release — pdf.js refuses a worker of another version, so the viewer
//! breaks only once a PDF is opened — a release's files under another
//! release's name, which browsers would keep a year as that release's, the
//! modern build copied in, which
//! fails in the desktop's WebViews, a file the viewer's script asks for
//! missing, or a licence dropped. And a version bump that only edited
//! `VERSION`, as the update bot does, until the files follow.
//!
//! Fixing a failure: run `just pdfjs VERSION INTEGRITY` (docs/development.md
//! §2.6), which replaces the whole directory from the published archive; when
//! the viewer's script loads a new file, add it to `scripts/pdfjs.sh`.

use oxidgene_guards::{read, root};

const DIRECTORY: &str = "assets/public/pdfjs";

/// The release `VERSION` records: `pdfjs-dist <version> sha512-…`.
fn recorded_version() -> String {
    let record = read(format!("{DIRECTORY}/VERSION"));
    let fields: Vec<&str> = record.split_whitespace().collect();
    match fields.as_slice() {
        ["pdfjs-dist", version, integrity] if integrity.starts_with("sha512-") => {
            (*version).to_string()
        }
        _ => panic!("{DIRECTORY}/VERSION is not `pdfjs-dist <version> sha512-…`: {record}"),
    }
}

/// The directory of the release `VERSION` records.
fn release_directory() -> String {
    format!("{DIRECTORY}/{}", recorded_version())
}

#[test]
fn the_release_directory_is_the_only_one() {
    let mut entries: Vec<String> = std::fs::read_dir(root().join(DIRECTORY))
        .expect("the vendored directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        [recorded_version(), "VERSION".to_string()],
        "{DIRECTORY} holds the release VERSION names and VERSION alone"
    );
}

#[test]
fn every_script_is_of_the_release_version_names() {
    let version = recorded_version();
    let directory = release_directory();
    for script in ["pdf.min.mjs", "pdf.worker.min.mjs", "pdf_viewer.mjs"] {
        let source = read(format!("{directory}/{script}"));
        assert!(
            source.contains(&format!("pdfjsVersion = {version}")),
            "{directory}/{script} is not pdf.js {version}"
        );
    }
}

#[test]
fn the_build_is_the_legacy_one() {
    // The legacy build carries core-js's polyfills; the modern one has none.
    let directory = release_directory();
    for script in ["pdf.min.mjs", "pdf.worker.min.mjs", "pdf_viewer.mjs"] {
        assert!(
            read(format!("{directory}/{script}")).contains("core-js"),
            "{directory}/{script} is not from pdf.js's legacy build"
        );
    }
}

#[test]
fn every_file_the_viewer_loads_is_vendored_with_its_licence() {
    let script = read("crates/oxidgene-ui/src/components/pdf_viewer.js");
    let mut loaded: Vec<String> = script
        .split("base + \"")
        .skip(1)
        .map(|rest| rest[..rest.find('"').expect("a closed string")].to_string())
        .collect();
    assert!(loaded.len() >= 5, "the script's loads were not found");
    // pdf.js asks the wasm directory for its decoders by name.
    if let Some(index) = loaded.iter().position(|path| path == "wasm/") {
        loaded.splice(
            index..=index,
            [
                "wasm/openjpeg.wasm".to_string(),
                "wasm/jbig2.wasm".to_string(),
            ],
        );
    }
    let release = release_directory();
    let directory = root().join(&release);
    for path in loaded
        .iter()
        .map(String::as_str)
        .chain(["images/loading-icon.gif", "LICENSE"])
        .chain(["wasm/LICENSE_OPENJPEG", "wasm/LICENSE_PDFJS_OPENJPEG"])
        .chain(["wasm/LICENSE_JBIG2", "wasm/LICENSE_PDFJS_JBIG2"])
    {
        assert!(
            directory.join(path).is_file(),
            "{release}/{path} is missing"
        );
    }
}
