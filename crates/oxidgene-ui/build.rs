use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/i18n");
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut files: Vec<_> = fs::read_dir(directory)
        .expect("locale asset directory")
        .map(|entry| entry.expect("locale asset entry").path())
        .filter(|file| {
            file.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let sources: Vec<_> = files
        .iter()
        .map(|file| embedded_source(file, &output))
        .collect();
    fs::write(
        output.join("locales.rs"),
        format!("&[{}]", sources.join(",")),
    )
    .expect("embedded locale list");
}

fn embedded_source(file: &Path, output: &Path) -> String {
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
    format!("include_str!({:?})", file.canonicalize().unwrap())
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
