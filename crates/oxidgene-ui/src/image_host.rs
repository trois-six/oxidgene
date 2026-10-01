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
//! `docs/cross-cutting.md` §7.1 requires until authentication
//! ships.

use std::sync::Arc;

// An implementor lives outside this crate (the desktop shell) and needs both
// types to write the trait's signature, so the trait's module exposes them.
pub use oxidgene_core::Sex;
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

    /// The path this shell serves the default silhouette from.
    ///
    /// Not backend media: three pictures compiled into the application, drawn
    /// wherever somebody has no portrait. Inline they are a few kilobytes
    /// repeated once per card; served, they are one resource the engine
    /// fetches and decodes once for the whole page.
    fn silhouette_path(&self, sex: Sex) -> Option<String>;
}

/// The stable name a silhouette is served under.
#[must_use]
pub fn silhouette_slug(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "male",
        Sex::Female => "female",
        Sex::Unknown => "unknown",
    }
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

    #[must_use]
    pub fn silhouette_path(&self, sex: Sex) -> Option<String> {
        self.0.silhouette_path(sex)
    }
}

impl std::fmt::Debug for ImageHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImageHost")
    }
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

/// Whether `path` is one [`api_path`] produces: a medium's thumbnail or
/// file, or a vignette's image, named by UUIDs.
///
/// A shell that proxies these paths onto the backend answers nothing else,
/// so whatever reaches the proxy — markup that slipped through, a crafted
/// link — can read no other route with the shell's credentials.
#[must_use]
pub fn is_asset_path(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    let is_id = |segment: &str| segment.parse::<Uuid>().is_ok();
    matches!(
        segments.as_slice(),
        ["", "api", "v1", "trees", tree, "media", media, "thumbnail" | "file"]
            if is_id(tree) && is_id(media)
    ) || matches!(
        segments.as_slice(),
        ["", "api", "v1", "trees", tree, "vignettes", vignette, "image"]
            if is_id(tree) && is_id(vignette)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_asset_paths_are_asset_paths() {
        let id = Uuid::now_v7();
        for asset in [
            MediaAsset::Thumbnail { media_id: id },
            MediaAsset::Crop { vignette_id: id },
            MediaAsset::File { media_id: id },
        ] {
            assert!(is_asset_path(&api_path(id, asset)), "{asset:?}");
        }
        for other in [
            "/api/v1/trees".to_string(),
            format!("/api/v1/trees/{id}"),
            format!("/api/v1/trees/{id}/media/{id}"),
            format!("/api/v1/trees/{id}/media/{id}/archive"),
            format!("/api/v1/trees/{id}/media/{id}/file/extra"),
            format!("/api/v1/trees/{id}/media/../persons/file"),
            format!("/api/v1/trees/{id}/vignettes/{id}/image?x=1"),
            format!("/api/v1/trees/{id}/notes/{id}/thumbnail"),
        ] {
            assert!(!is_asset_path(&other), "{other}");
        }
    }

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

    /// The slug is what a shell serves the picture under, so it has to stay
    /// stable and distinct — a collision would draw the wrong silhouette.
    #[test]
    fn every_sex_has_its_own_silhouette_slug() {
        let slugs = [Sex::Male, Sex::Female, Sex::Unknown].map(silhouette_slug);

        assert_eq!(slugs, ["male", "female", "unknown"]);
        assert_eq!(
            slugs.iter().collect::<std::collections::HashSet<_>>().len(),
            3
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
