use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let assets = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    let locales: Vec<_> = json_documents(&assets.join("i18n"))
        .iter()
        .map(|file| embedded_locale(file, &output))
        .collect();
    write_list(&output.join("locales.rs"), &locales);
}

/// Every `*.json` file of `directory`, sorted by name.
fn json_documents(directory: &Path) -> Vec<PathBuf> {
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut files: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("asset directory entry").path())
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    files
}

fn write_list(destination: &Path, items: &[String]) {
    fs::write(destination, format!("&[{}]", items.join(","))).expect("embedded asset list");
}

fn embedded_text(file: &Path) -> String {
    format!("include_str!({:?})", file.canonicalize().unwrap())
}

fn embedded_locale(file: &Path, output: &Path) -> String {
    #[cfg(feature = "compressed-locales")]
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("wasm32") {
        let compressed = output.join(format!(
            "{}.br",
            file.file_name().unwrap().to_string_lossy()
        ));
        compress_if_changed(file, &compressed);
        return format!("include_bytes!({:?}).as_slice()", compressed);
    }
    let _ = output;
    embedded_text(file)
}

#[cfg(feature = "compressed-locales")]
fn compress_if_changed(source: &Path, destination: &Path) {
    use std::io::Write;
    let modified = |file: &Path| {
        fs::metadata(file)
            .and_then(|metadata| metadata.modified())
            .ok()
    };
    if let (Some(source_time), Some(compressed_time)) = (modified(source), modified(destination))
        && compressed_time >= source_time
    {
        return;
    }
    let json = fs::read(source).expect("locale JSON source");
    let mut compressed = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut compressed, 1 << 16, 11, 24);
        encoder.write_all(&json).expect("Brotli locale compression");
    }
    fs::write(destination, compressed).expect("compressed locale asset");
}
