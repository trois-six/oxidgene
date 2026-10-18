//! pdf.js, served to the window from the binary.
//!
//! The PDF viewer loads its library from `/pdfjs/…` on the page's own origin
//! (`docs/ui-common.md`, PDF viewer). The web bundle serves those files as
//! static files; the desktop answers for the same paths here, from the copy
//! the build script compressed into the binary (`build.rs`), so a PDF opens
//! without the network and with exactly the release the application was
//! built with.

use dioxus::desktop::use_asset_handler;
use dioxus::desktop::wry::http::Response;

/// The path prefix this handler answers on. Dioxus routes `/<name>/…` here.
const HANDLER: &str = "pdfjs";

/// Every file of `assets/public/pdfjs/`, by its path there, compressed.
static FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/pdfjs/pdfjs.rs"));

/// Installs the handler for as long as the calling component lives: the
/// window's.
pub fn use_pdfjs_handler() {
    use_asset_handler(HANDLER, |request, responder| {
        let found = request
            .uri()
            .path()
            .strip_prefix(&format!("/{HANDLER}/"))
            .and_then(response);
        responder.respond(found.unwrap_or_else(not_found));
    });
}

/// The file at `path`, decompressed, or `None` for a path that names none.
fn response(path: &str) -> Option<Response<Vec<u8>>> {
    let bytes = file(path)?;
    Response::builder()
        .status(200)
        .header("Content-Type", content_type(path))
        // A module script and its worker are only run under a script type,
        // which the engine must not second-guess.
        .header("X-Content-Type-Options", "nosniff")
        // The library is loaded once per window, and a later release of the
        // application must never meet an earlier one's files in the
        // WebView's cache: decompressing it again costs milliseconds.
        .header("Cache-Control", "no-store")
        .body(bytes)
        .ok()
}

/// The decompressed contents of the embedded file at `path`.
fn file(path: &str) -> Option<Vec<u8>> {
    let (_, compressed) = FILES.iter().find(|(name, _)| *name == path)?;
    let mut bytes = Vec::new();
    brotli_decompressor::BrotliDecompress(&mut &compressed[..], &mut bytes).ok()?;
    Some(bytes)
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("gif") => "image/gif",
        _ => "text/plain; charset=utf-8",
    }
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(404)
        .body(Vec::new())
        .expect("a bodyless 404 is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `pdf_viewer.js` loads, and the decoders pdf.js asks for by name.
    const LOADED: &[&str] = &[
        "pdf.min.mjs",
        "pdf.worker.min.mjs",
        "pdf_viewer.mjs",
        "pdf_viewer.css",
        "images/loading-icon.gif",
        "wasm/openjpeg.wasm",
        "wasm/jbig2.wasm",
    ];

    #[test]
    fn every_file_the_viewer_loads_is_embedded_as_it_is_on_disk() {
        let directory = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/public/pdfjs");
        for path in LOADED {
            let on_disk = std::fs::read(format!("{directory}/{path}")).unwrap();
            assert_eq!(file(path).as_deref(), Some(on_disk.as_slice()), "{path}");
        }
        // The licences travel with the code they cover.
        assert!(file("LICENSE").is_some());
    }

    #[test]
    fn scripts_are_served_as_scripts_and_nothing_else_is_answered() {
        let served = response("pdf.min.mjs").unwrap();
        assert_eq!(served.headers()["Content-Type"], "text/javascript");
        assert_eq!(served.headers()["X-Content-Type-Options"], "nosniff");
        assert_eq!(content_type("pdf_viewer.css"), "text/css");
        assert_eq!(content_type("wasm/jbig2.wasm"), "application/wasm");
        for path in [
            "",
            "missing.mjs",
            "../Cargo.toml",
            "/pdf.min.mjs",
            "pdf.min.mjs?x=1",
        ] {
            assert!(response(path).is_none(), "{path}");
        }
    }
}
