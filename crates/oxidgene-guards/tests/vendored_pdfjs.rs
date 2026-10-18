//! Vendored pdf.js: the files under `assets/public/pdfjs/` are one release,
//! the one `VERSION` names, its legacy build, with what the viewer loads.
//!
//! Drift it prevents: a file updated by hand or left over from another
//! release — pdf.js refuses a worker of another version, so the viewer
//! breaks only once a PDF is opened — the modern build copied in, which
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

#[test]
fn every_script_is_of_the_release_version_names() {
    let version = recorded_version();
    for script in ["pdf.min.mjs", "pdf.worker.min.mjs", "pdf_viewer.mjs"] {
        let source = read(format!("{DIRECTORY}/{script}"));
        assert!(
            source.contains(&format!("pdfjsVersion = {version}")),
            "{DIRECTORY}/{script} is not pdf.js {version}"
        );
    }
}

#[test]
fn the_build_is_the_legacy_one() {
    // The legacy build carries core-js's polyfills; the modern one has none.
    for script in ["pdf.min.mjs", "pdf.worker.min.mjs", "pdf_viewer.mjs"] {
        assert!(
            read(format!("{DIRECTORY}/{script}")).contains("core-js"),
            "{DIRECTORY}/{script} is not from pdf.js's legacy build"
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
    let directory = root().join(DIRECTORY);
    for path in loaded
        .iter()
        .map(String::as_str)
        .chain(["images/loading-icon.gif", "LICENSE"])
        .chain(["wasm/LICENSE_OPENJPEG", "wasm/LICENSE_PDFJS_OPENJPEG"])
        .chain(["wasm/LICENSE_JBIG2", "wasm/LICENSE_PDFJS_JBIG2"])
    {
        assert!(
            directory.join(path).is_file(),
            "{DIRECTORY}/{path} is missing"
        );
    }
}
