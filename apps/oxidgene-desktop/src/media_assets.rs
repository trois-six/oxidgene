//! Serving the tree's pictures from the application's own origin.
//!
//! A picture the backend holds is addressed by an [`ImageSource`], never by a
//! URL: until authentication ships, no backend address may appear in the markup
//! (`docs/cross-cutting.md` §7.1). The web build satisfies that
//! by fetching each picture through the typed client and handing it over as a
//! `data:` URL.
//!
//! The desktop can do better. Dioxus lets a shell answer for a path prefix on
//! its *own* origin, so the markup carries `/oxidgene-media/…` — no backend
//! address, and a real URL as far as the WebView is concerned. That is what
//! buys back everything a `data:` URL costs: the picture is cached between
//! renders and between pages, decoded off the main thread, and never fetched at
//! all while it stays off screen.
//!
//! The handler is a proxy onto the embedded server rather than a second reader
//! of the media store: the store lives behind `AppState` in the server thread,
//! and one loopback request is far cheaper than a second access path to keep
//! in step with the first.

use std::sync::Arc;

use dioxus::desktop::use_asset_handler;
use dioxus::desktop::wry::http::Response;
use dioxus::prelude::*;
use oxidgene_ui::api::ApiClient;
use oxidgene_ui::image_host::{
    ImageHost, MediaAsset, MediaAssetHost, Sex, Uuid, is_asset_path, silhouette_slug,
};

/// The path prefix this shell answers on. Dioxus routes `/<name>/…` here.
const HANDLER: &str = "oxidgene-media";

/// Resolves a held picture to a path on this shell's own origin.
struct DesktopAssetHost;

/// Where the silhouettes are served from, under the handler's own prefix.
const SILHOUETTE: &str = "silhouette";

impl MediaAssetHost for DesktopAssetHost {
    fn path(&self, tree_id: Uuid, asset: MediaAsset) -> Option<String> {
        // The API path travels as-is behind the handler prefix, so the handler
        // has nothing to parse and the two cannot drift apart.
        let api_path = oxidgene_ui::image_host::api_path(tree_id, asset);
        Some(format!("/{HANDLER}{api_path}"))
    }

    fn silhouette_path(&self, sex: Sex) -> Option<String> {
        Some(format!("/{HANDLER}/{SILHOUETTE}/{}", silhouette_slug(sex)))
    }
}

/// The capability handle to hand the API client.
#[must_use]
pub fn host() -> ImageHost {
    ImageHost::new(Arc::new(DesktopAssetHost))
}

/// Installs the handler and renders the application inside it.
///
/// A component because `use_asset_handler` is a hook: the handler lives exactly
/// as long as the window that serves from it.
#[component]
pub fn DesktopApp() -> Element {
    let api = use_context::<ApiClient>();
    use_asset_handler(HANDLER, move |request, responder| {
        let api = api.clone();
        // Strip the handler prefix to recover the API path the host built.
        let Some(path) = request
            .uri()
            .path()
            .strip_prefix(&format!("/{HANDLER}"))
            .map(str::to_string)
        else {
            responder.respond(not_found());
            return;
        };
        // The silhouettes are compiled into the application, not held by the
        // backend: they are answered from here rather than proxied.
        if let Some(slug) = path.strip_prefix(&format!("/{SILHOUETTE}/")) {
            responder.respond(match silhouette(slug) {
                Some(response) => response,
                None => not_found(),
            });
            return;
        }
        // Only the paths the host builds are proxied: the request goes out
        // with the launch token, so no other route may be reached this way.
        if !is_asset_path(&path) {
            responder.respond(not_found());
            return;
        }
        // The engine's validator travels on, so it revalidates its copy
        // against the backend's own checks rather than this handler's.
        let if_none_match = request
            .headers()
            .get("If-None-Match")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        spawn(async move {
            responder.respond(match fetch(&api, &path, if_none_match.as_deref()).await {
                Some(response) => response,
                None => not_found(),
            });
        });
    });

    // The PDF viewer's library answers on the same window, at `/pdfjs/…`.
    crate::pdfjs_assets::use_pdfjs_handler();

    rsx! {
        oxidgene_ui::App {}
    }
}

/// The backend's answer for `path`, conditional on `if_none_match`: its
/// `304`, or its bytes, each with its validator and caching rule — a crop
/// is revalidated on every use, so a moved rectangle never shows its old
/// picture (`docs/architecture.md` §4.2). Anything the backend refuses is
/// not found here.
async fn fetch(
    api: &ApiClient,
    path: &str,
    if_none_match: Option<&str>,
) -> Option<Response<Vec<u8>>> {
    let asset = api.get_asset(path, if_none_match).await.ok()?;
    let mut response = Response::builder()
        .status(if asset.not_modified { 304 } else { 200 })
        // The file's type is whatever its upload declared, and this origin is
        // the application's own: an HTML or SVG file must never run here.
        .header("X-Content-Type-Options", "nosniff")
        .header("Content-Security-Policy", "sandbox")
        // The backend's rule, or, should it state none, the window's
        // lifetime: same process, same store.
        .header(
            "Cache-Control",
            asset
                .cache_control
                .as_deref()
                .unwrap_or("private, max-age=3600"),
        );
    if let Some(etag) = &asset.etag {
        response = response.header("ETag", etag.as_str());
    }
    if !asset.not_modified {
        response = response.header("Content-Type", asset.content_type);
    }
    response.body(asset.bytes).ok()
}

fn silhouette(slug: &str) -> Option<Response<Vec<u8>>> {
    let sex = [Sex::Male, Sex::Female, Sex::Unknown]
        .into_iter()
        .find(|sex| silhouette_slug(*sex) == slug)?;
    Response::builder()
        .status(200)
        .header("Content-Type", "image/png")
        .header("X-Content-Type-Options", "nosniff")
        // Compiled into the binary: it cannot change while the window is open.
        .header("Cache-Control", "private, max-age=31536000, immutable")
        .body(oxidgene_ui::components::pedigree_chart::silhouette_png(sex).to_vec())
        .ok()
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(404)
        .body(Vec::new())
        .expect("a bodyless 404 is well-formed")
}
