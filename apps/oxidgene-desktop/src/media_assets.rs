//! Serving the tree's pictures from the application's own origin.
//!
//! A picture the backend holds is addressed by an [`ImageSource`], never by a
//! URL: until authentication ships, no backend address may appear in the markup
//! (`docs/specifications/cross-cutting.md` §7.1). The web build satisfies that
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
use oxidgene_ui::image_host::{ImageHost, MediaAsset, MediaAssetHost, Uuid};

/// The path prefix this shell answers on. Dioxus routes `/<name>/…` here.
const HANDLER: &str = "oxidgene-media";

/// Resolves a held picture to a path on this shell's own origin.
struct DesktopAssetHost;

impl MediaAssetHost for DesktopAssetHost {
    fn path(&self, tree_id: Uuid, asset: MediaAsset) -> Option<String> {
        // The API path travels as-is behind the handler prefix, so the handler
        // has nothing to parse and the two cannot drift apart.
        let api_path = oxidgene_ui::image_host::api_path(tree_id, asset);
        Some(format!("/{HANDLER}{api_path}"))
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
        spawn(async move {
            responder.respond(match fetch(&api, &path).await {
                Some(response) => response,
                None => not_found(),
            });
        });
    });

    rsx! {
        oxidgene_ui::App {}
    }
}

async fn fetch(api: &ApiClient, path: &str) -> Option<Response<Vec<u8>>> {
    let (bytes, content_type) = api.get_binary(path).await.ok()?;
    Response::builder()
        .status(200)
        .header("Content-Type", content_type)
        // Same window, same process, same lifetime as the store it came from:
        // the WebView may keep it for as long as it is open.
        .header("Cache-Control", "private, max-age=3600")
        .body(bytes)
        .ok()
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(404)
        .body(Vec::new())
        .expect("a bodyless 404 is well-formed")
}
