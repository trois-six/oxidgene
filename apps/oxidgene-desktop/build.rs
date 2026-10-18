use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    embed_pdfjs();
    #[cfg(target_os = "windows")]
    windows_resources();
}

/// Compresses every file of `assets/public/pdfjs/` with Brotli into
/// `OUT_DIR`, and lists them in `pdfjs.rs` as `(path, compressed bytes)` for
/// the window's pdf.js handler (`src/pdfjs_assets.rs`): the files the web
/// bundle serves as they are, a quarter of their size in the binary.
fn embed_pdfjs() {
    let source = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"))
        .ancestors()
        .nth(2)
        .expect("apps/oxidgene-desktop is two levels deep")
        .join("assets/public/pdfjs");
    println!("cargo:rerun-if-changed={}", source.display());
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("set by Cargo")).join("pdfjs");
    fs::create_dir_all(&output).expect("OUT_DIR is writable");
    let mut files = Vec::new();
    collect(&source, &mut files);
    files.sort();
    let entries: Vec<String> = files
        .iter()
        .map(|file| {
            let name = file
                .strip_prefix(&source)
                .expect("under the pdf.js directory")
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let compressed = output.join(format!("{}.br", name.replace('/', "__")));
            compress_if_changed(file, &compressed);
            format!("({name:?}, include_bytes!({compressed:?}).as_slice())")
        })
        .collect();
    fs::write(
        output.join("pdfjs.rs"),
        format!("&[{}]", entries.join(",\n")),
    )
    .expect("OUT_DIR is writable");
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error} (run `just pdfjs`)", directory.display()));
    for entry in entries {
        let path = entry.expect("pdf.js directory entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// Compresses `source` into `destination` unless `destination` already
/// holds it, compared by content as the UI's locales are: a target
/// directory shared with another checkout may hold another release's.
fn compress_if_changed(source: &Path, destination: &Path) {
    use std::io::Write;
    let bytes = fs::read(source).unwrap_or_else(|error| panic!("{}: {error}", source.display()));
    let current = fs::read(destination).ok().and_then(|compressed| {
        let mut bytes = Vec::new();
        brotli::BrotliDecompress(&mut compressed.as_slice(), &mut bytes).ok()?;
        Some(bytes)
    });
    if current.as_deref() == Some(bytes.as_slice()) {
        return;
    }
    let mut compressed = Vec::new();
    {
        let mut encoder = brotli::CompressorWriter::new(&mut compressed, 1 << 16, 11, 24);
        encoder.write_all(&bytes).expect("Brotli compression");
    }
    fs::write(destination, compressed).expect("OUT_DIR is writable");
}

/// Compiles `assets/desktop/windows.rc` with its placeholders filled in: the
/// icon's full path, and the package version as text and as the four numbers
/// of a `VERSIONINFO`.
#[cfg(target_os = "windows")]
fn windows_resources() {
    // The workspace root, two levels up, without `..` in the path.
    let desktop = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"))
        .ancestors()
        .nth(2)
        .expect("apps/oxidgene-desktop is two levels deep")
        .join("assets")
        .join("desktop");
    let template = desktop.join("windows.rc");
    let icon = desktop.join("icon.ico");
    println!("cargo:rerun-if-changed={}", template.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    let number = |part| env::var(part).expect("set by Cargo");
    let numbers = format!(
        "{},{},{},0",
        number("CARGO_PKG_VERSION_MAJOR"),
        number("CARGO_PKG_VERSION_MINOR"),
        number("CARGO_PKG_VERSION_PATCH"),
    );
    let resources = std::fs::read_to_string(&template)
        .expect("assets/desktop/windows.rc is readable")
        // Resource scripts escape backslashes in quoted paths.
        .replace("@ICON@", &icon.display().to_string().replace('\\', "\\\\"))
        .replace("@VERSION_NUMBERS@", &numbers)
        .replace("@VERSION@", &number("CARGO_PKG_VERSION"));
    let script = PathBuf::from(env::var("OUT_DIR").expect("set by Cargo")).join("windows.rc");
    std::fs::write(&script, resources).expect("OUT_DIR is writable");

    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_required()
        .expect("failed to compile Windows resources");
}
