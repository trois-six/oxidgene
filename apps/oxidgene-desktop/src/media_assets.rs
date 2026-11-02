//! Serving the tree's pictures from the application's own origin.
//!
//! A picture the backend holds is addressed by an [`ImageSource`] and its
//! version, never by a URL. The web build draws it from the API's versioned
//! address (`docs/cross-cutting.md` §7.1); the embedded backend answers only
//! requests carrying the launch token, which a WebView fetching an image
//! does not send.
//!
//! Dioxus lets a shell answer for a path prefix on its *own* origin, so the
//! markup carries `/oxidgene-media/…` — no backend address, and a real URL as
//! far as the WebView is concerned: the picture is cached between renders and
//! pages, decoded off the main thread, and never fetched at all
//! while it stays off screen. The path carries the picture's version, and
//! the backend's caching rule — immutable at that version — travels unchanged
//! (`docs/architecture.md` §4.3).
//!
//! The handler is a proxy onto the embedded server rather than a second reader
//! of the media store: the store lives behind `AppState` in the server thread,
//! and one loopback request is far cheaper than a second access path to keep
//! in step with the first.

use std::sync::Arc;

use dioxus::desktop::use_asset_handler;
use dioxus::desktop::wry::http::Response;
use dioxus::prelude::*;
use oxidgene_ui::api::{ApiClient, AssetResponse};
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
    fn path(&self, tree_id: Uuid, asset: &MediaAsset) -> Option<String> {
        // The API path travels as-is behind the handler prefix, so the handler
        // has nothing to parse and the two cannot drift apart.
        let api_path = oxidgene_ui::image_host::api_path(tree_id, asset);
        Some(format!("/{HANDLER}{api_path}"))
    }

    fn silhouette_path(&self, sex: Sex) -> Option<String> {
        Some(format!(
            "/{HANDLER}/{SILHOUETTE}/{}?v={}",
            silhouette_slug(sex),
            silhouette_version(sex)
        ))
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
        // Strip the handler prefix to recover the API path, version
        // included, the host built.
        let Some(path) = request
            .uri()
            .path_and_query()
            .and_then(|path| path.as_str().strip_prefix(&format!("/{HANDLER}")))
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
        spawn(async move {
            responder.respond(match fetch(&api, &path).await {
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

/// The backend's answer for `path`: its bytes with its caching rule, or,
/// for a version that is no longer the picture's, a redirect to the current
/// one's path under this handler's prefix, never stored — the old path must
/// never come to hold the new picture (`docs/architecture.md` §4.3).
/// Anything the backend refuses is not found here.
async fn fetch(api: &ApiClient, path: &str) -> Option<Response<Vec<u8>>> {
    match api.get_asset(path).await.ok()? {
        AssetResponse::Content {
            bytes,
            content_type,
            cache_control,
        } => {
            let mut response = Response::builder()
                .status(200)
                .header("Content-Type", content_type)
                // The file's type is whatever its upload declared, and this
                // origin is the application's own: an HTML or SVG file must
                // never run here.
                .header("X-Content-Type-Options", "nosniff")
                .header("Content-Security-Policy", "sandbox");
            if let Some(rule) = cache_control {
                response = response.header("Cache-Control", rule);
            }
            response.body(bytes).ok()
        }
        AssetResponse::Moved { location } => is_asset_path(&location)
            .then(|| {
                Response::builder()
                    .status(307)
                    .header("Location", format!("/{HANDLER}{location}"))
                    .header("Cache-Control", "no-store")
                    .body(Vec::new())
                    .ok()
            })
            .flatten(),
    }
}

/// The version of the silhouette of `sex`: a hash of the picture compiled
/// in, so that a release that changes it is a new address.
fn silhouette_version(sex: Sex) -> String {
    let bytes = oxidgene_ui::components::pedigree_chart::silhouette_png(sex);
    // FNV-1a: the picture is ours and the hash names its content, nothing
    // more is asked of it.
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The silhouette `slug` names, `{sex}?v={version}`, at its version; any
/// other version is not found, since only this build's paths name one.
fn silhouette(slug: &str) -> Option<Response<Vec<u8>>> {
    let (slug, version) = slug.split_once("?v=")?;
    let sex = [Sex::Male, Sex::Female, Sex::Unknown]
        .into_iter()
        .find(|sex| silhouette_slug(*sex) == slug)?;
    (version == silhouette_version(sex)).then_some(())?;
    Response::builder()
        .status(200)
        .header("Content-Type", "image/png")
        .header("X-Content-Type-Options", "nosniff")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silhouette_is_served_at_its_version_only() {
        for sex in [Sex::Male, Sex::Female, Sex::Unknown] {
            let path = DesktopAssetHost.silhouette_path(sex).unwrap();
            let slug = path
                .strip_prefix(&format!("/{HANDLER}/{SILHOUETTE}/"))
                .unwrap();
            let served = silhouette(slug).unwrap();
            assert_eq!(
                served.headers()["Cache-Control"],
                "private, max-age=31536000, immutable"
            );
            let stale = format!("{}?v=0000000000000000", silhouette_slug(sex));
            assert!(silhouette(&stale).is_none(), "{sex:?}");
            assert!(silhouette(silhouette_slug(sex)).is_none(), "{sex:?}");
        }
        assert_ne!(
            silhouette_version(Sex::Male),
            silhouette_version(Sex::Female)
        );
    }
}
