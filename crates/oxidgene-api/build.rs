//! Gzip-compresses the reference-data JSON files (occupation sheets, given
//! name meanings) at build time so the plain-text JSON stays diffable in
//! git while only the compressed bytes get embedded into the binary via
//! `include_bytes!` (see `src/reference/loader.rs`).

use std::fs::File;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::GzEncoder;

const DATA_FILES: &[&str] = &[
    "occupations.fr.json",
    "occupations.en.json",
    "given_names.fr.json",
    "given_names.en.json",
];

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let data_dir = Path::new(&manifest_dir).join("src/reference/data");

    for file_name in DATA_FILES {
        let src_path = data_dir.join(file_name);
        println!("cargo:rerun-if-changed={}", src_path.display());

        let json = std::fs::read(&src_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_path.display()));

        let out_path = Path::new(&out_dir).join(format!("{file_name}.gz"));
        let out_file = File::create(&out_path)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", out_path.display()));
        let mut encoder = GzEncoder::new(out_file, Compression::best());
        encoder
            .write_all(&json)
            .unwrap_or_else(|e| panic!("failed to compress {}: {e}", src_path.display()));
        encoder
            .finish()
            .unwrap_or_else(|e| panic!("failed to finish gzip stream for {file_name}: {e}"));
    }
}
