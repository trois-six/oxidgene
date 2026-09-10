//! Turning a backend-held picture into something the engine can draw.
//!
//! The API never sends pictures inline: a gallery or a pedigree says where each
//! one lives (`ImageSource`) and the bytes travel over their own request. What
//! a "request" means differs per platform, and this is the one place that knows:
//!
//! - **Desktop** installs a [`MediaAssetHost`]. It serves the picture from the
//!   application's own origin, so the markup carries a short relative path, the
//!   WebView caches it, decodes it off the main thread, and never fetches one
//!   that stays off screen.
//! - **Web** has no such origin. The bytes are fetched through the typed client
//!   and handed over as a `data:` URL, which is what every picture used to be.
//!
//! Either way no backend address reaches the markup, which is what
//! `docs/specifications/cross-cutting.md` §7.1 requires until authentication
//! ships.

use std::sync::Arc;

use dioxus::prelude::*;

// An implementor lives outside this crate (the desktop shell) and needs both
// types to write the trait's signature, so the trait's module exposes them.
pub use oxidgene_core::types::ImageSource;
pub use uuid::Uuid;

/// Serves backend-held pictures from the application's own origin.
///
/// Implemented by `oxidgene-desktop`. One method, because there is exactly one
/// thing the UI cannot work out for itself: what address its own shell answers
/// on.
pub trait MediaAssetHost: Send + Sync {
    /// The path this shell serves `asset` from, relative to its own origin.
    ///
    /// `None` for an asset this host does not serve, which sends the caller
    /// down the fetch-and-encode path instead.
    fn path(&self, tree_id: Uuid, asset: MediaAsset) -> Option<String>;
}

/// One picture the backend holds, named by what serves it.
///
/// A closed set of three endpoints rather than an arbitrary path: a shell that
/// answers for these is answering for pictures, and cannot be talked into
/// proxying the rest of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAsset {
    /// The thumbnail generated for a media.
    Thumbnail { media_id: Uuid },
    /// The region cut out of a media.
    Crop { vignette_id: Uuid },
    /// The stored file itself, at full size.
    File { media_id: Uuid },
}

impl MediaAsset {
    /// The asset an [`ImageSource`] names, when it names one of ours.
    #[must_use]
    pub fn from_source(source: &ImageSource) -> Option<Self> {
        match source {
            ImageSource::Remote { .. } => None,
            ImageSource::Thumbnail { media_id } => Some(Self::Thumbnail {
                media_id: *media_id,
            }),
            ImageSource::Crop { vignette_id } => Some(Self::Crop {
                vignette_id: *vignette_id,
            }),
        }
    }
}

/// Context handle the API client looks for.
#[derive(Clone)]
pub struct ImageHost(Arc<dyn MediaAssetHost>);

impl ImageHost {
    #[must_use]
    pub fn new(host: Arc<dyn MediaAssetHost>) -> Self {
        Self(host)
    }

    #[must_use]
    pub fn path(&self, tree_id: Uuid, asset: MediaAsset) -> Option<String> {
        self.0.path(tree_id, asset)
    }
}

impl std::fmt::Debug for ImageHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImageHost")
    }
}

/// The host, if this build has one. `None` on the web target.
pub fn use_image_host() -> Option<ImageHost> {
    try_use_context::<ImageHost>()
}

/// The API path that serves one held picture.
///
/// Shared by both resolution paths: the desktop handler proxies this path, and
/// the web fallback fetches it through the typed client.
#[must_use]
pub fn api_path(tree_id: Uuid, asset: MediaAsset) -> String {
    match asset {
        MediaAsset::Thumbnail { media_id } => {
            format!("/api/v1/trees/{tree_id}/media/{media_id}/thumbnail")
        }
        MediaAsset::Crop { vignette_id } => {
            format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}/image")
        }
        MediaAsset::File { media_id } => {
            format!("/api/v1/trees/{tree_id}/media/{media_id}/file")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_asset_names_the_endpoint_that_serves_it() {
        let id = Uuid::nil();
        let zero = "00000000-0000-0000-0000-000000000000";

        assert_eq!(
            api_path(id, MediaAsset::Thumbnail { media_id: id }),
            format!("/api/v1/trees/{zero}/media/{zero}/thumbnail")
        );
        assert_eq!(
            api_path(id, MediaAsset::Crop { vignette_id: id }),
            format!("/api/v1/trees/{zero}/vignettes/{zero}/image")
        );
        assert_eq!(
            api_path(id, MediaAsset::File { media_id: id }),
            format!("/api/v1/trees/{zero}/media/{zero}/file")
        );
    }

    /// A file somebody else hosts is fetched from where it lives; we never
    /// become a proxy for it, so it maps to no asset of ours.
    #[test]
    fn a_remote_source_names_no_asset_of_ours() {
        assert_eq!(
            MediaAsset::from_source(&ImageSource::Remote {
                url: "https://archives.example.org/1.jpg".to_string()
            }),
            None
        );
        assert_eq!(
            MediaAsset::from_source(&ImageSource::Thumbnail {
                media_id: Uuid::nil()
            }),
            Some(MediaAsset::Thumbnail {
                media_id: Uuid::nil()
            })
        );
    }
}
