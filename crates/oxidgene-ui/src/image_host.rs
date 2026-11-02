//! Turning a backend-held picture into something the engine can draw.
//!
//! The API never sends pictures inline: a gallery or a pedigree says where each
//! one lives and at which version (`ImageSource`), and the engine fetches the
//! bytes from the versioned address, which it keeps immutable
//! (`docs/architecture.md` §4.3). Whose address it is differs per platform:
//!
//! - **Desktop** installs a [`MediaAssetHost`]. It serves the picture from the
//!   application's own origin, which proxies the embedded backend, so the
//!   markup carries a short relative path.
//! - **Web** draws it from the API's own address, which the browser fetches
//!   with the page's credentials (`docs/cross-cutting.md` §7.1).
//!
//! Either way the engine caches the picture, decodes it off the main thread,
//! and never fetches one that stays off screen.

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
    fn path(&self, tree_id: Uuid, asset: &MediaAsset) -> Option<String>;

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
/// A closed set of endpoints rather than an arbitrary path: a shell that
/// answers for these is answering for pictures, and cannot be talked into
/// proxying the rest of the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// The thumbnail generated for a media.
    Thumbnail,
    /// The region cut out of a media.
    Crop,
    /// The stored file itself, at full size.
    File,
    /// The card rendition of a media: its picture at the size of a portrait
    /// on a card.
    Card,
    /// The region cut out of a media, at the size of a portrait on a card.
    CardCrop,
}

impl AssetKind {
    /// The path of the asset of record `id`, under tree `tree_id`.
    fn path(self, tree_id: Uuid, id: Uuid) -> String {
        match self {
            Self::Thumbnail => format!("/api/v1/trees/{tree_id}/media/{id}/thumbnail"),
            Self::Crop => format!("/api/v1/trees/{tree_id}/vignettes/{id}/image"),
            Self::File => format!("/api/v1/trees/{tree_id}/media/{id}/file"),
            Self::Card => format!("/api/v1/trees/{tree_id}/media/{id}/card"),
            Self::CardCrop => format!("/api/v1/trees/{tree_id}/vignettes/{id}/card"),
        }
    }
}

/// One held picture: what serves it, the medium or vignette it is of, and
/// its version, which makes its address change with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAsset {
    pub kind: AssetKind,
    /// The medium (thumbnail, card, file) or the vignette (crops).
    pub id: Uuid,
    pub version: String,
}

impl MediaAsset {
    /// A medium's stored file, at `version`.
    #[must_use]
    pub fn file(media_id: Uuid, version: String) -> Self {
        Self {
            kind: AssetKind::File,
            id: media_id,
            version,
        }
    }

    /// The asset an [`ImageSource`] names, when it names one of ours.
    #[must_use]
    pub fn from_source(source: &ImageSource) -> Option<Self> {
        let (kind, id, version) = match source {
            ImageSource::Remote { .. } => return None,
            ImageSource::Thumbnail { media_id, version } => {
                (AssetKind::Thumbnail, media_id, version)
            }
            ImageSource::Crop {
                vignette_id,
                version,
            } => (AssetKind::Crop, vignette_id, version),
            ImageSource::Card { media_id, version } => (AssetKind::Card, media_id, version),
            ImageSource::CardCrop {
                vignette_id,
                version,
            } => (AssetKind::CardCrop, vignette_id, version),
        };
        Some(Self {
            kind,
            id: *id,
            version: version.clone(),
        })
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
    pub fn path(&self, tree_id: Uuid, asset: &MediaAsset) -> Option<String> {
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

/// The API path, version included, that serves one held picture.
///
/// Shared by both resolution paths: the desktop handler proxies this path, and
/// the web draws it from the API's origin.
#[must_use]
pub fn api_path(tree_id: Uuid, asset: &MediaAsset) -> String {
    format!("{}?v={}", asset.kind.path(tree_id, asset.id), asset.version)
}

/// Whether `path` is one [`api_path`] produces: a medium's thumbnail, card
/// rendition or file, or a vignette's image or card, named by UUIDs, with a
/// version of hexadecimal digits.
///
/// A shell that proxies these paths onto the backend answers nothing else,
/// so whatever reaches the proxy — markup that slipped through, a crafted
/// link — can read no other route with the shell's credentials.
#[must_use]
pub fn is_asset_path(path: &str) -> bool {
    let Some((path, version)) = path.split_once("?v=") else {
        return false;
    };
    let versioned = (1..=oxidgene_core::types::VERSION_DIGITS).contains(&version.len())
        && version.bytes().all(|b| b.is_ascii_hexdigit());
    let segments: Vec<&str> = path.split('/').collect();
    let is_id = |segment: &str| segment.parse::<Uuid>().is_ok();
    versioned
        && (matches!(
            segments.as_slice(),
            ["", "api", "v1", "trees", tree, "media", media, "thumbnail" | "file" | "card"]
                if is_id(tree) && is_id(media)
        ) || matches!(
            segments.as_slice(),
            ["", "api", "v1", "trees", tree, "vignettes", vignette, "image" | "card"]
                if is_id(tree) && is_id(vignette)
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [AssetKind; 5] = [
        AssetKind::Thumbnail,
        AssetKind::Crop,
        AssetKind::File,
        AssetKind::Card,
        AssetKind::CardCrop,
    ];

    fn asset(kind: AssetKind, id: Uuid) -> MediaAsset {
        MediaAsset {
            kind,
            id,
            version: "0123456789abcdef".into(),
        }
    }

    #[test]
    fn only_asset_paths_are_asset_paths() {
        let id = Uuid::now_v7();
        for kind in KINDS {
            assert!(is_asset_path(&api_path(id, &asset(kind, id))), "{kind:?}");
        }
        for other in [
            "/api/v1/trees".to_string(),
            format!("/api/v1/trees/{id}?v=1"),
            format!("/api/v1/trees/{id}/media/{id}?v=1"),
            format!("/api/v1/trees/{id}/media/{id}/archive?v=1"),
            format!("/api/v1/trees/{id}/media/{id}/file/extra?v=1"),
            format!("/api/v1/trees/{id}/media/../persons/file?v=1"),
            format!("/api/v1/trees/{id}/vignettes/{id}/image?x=1"),
            format!("/api/v1/trees/{id}/vignettes/{id}/image?v=1&x=1"),
            format!("/api/v1/trees/{id}/vignettes/{id}/image?v=0123456789abcdef0"),
            format!("/api/v1/trees/{id}/vignettes/{id}/image"),
            format!("/api/v1/trees/{id}/notes/{id}/thumbnail?v=1"),
            format!("/api/v1/trees/{id}/notes/{id}/card?v=1"),
            format!("/api/v1/trees/{id}/vignettes/{id}/file?v=1"),
        ] {
            assert!(!is_asset_path(&other), "{other}");
        }
    }

    #[test]
    fn every_asset_names_the_endpoint_that_serves_it_at_its_version() {
        let id = Uuid::nil();
        let zero = "00000000-0000-0000-0000-000000000000";
        let paths = KINDS.map(|kind| api_path(id, &asset(kind, id)));
        assert_eq!(
            paths,
            [
                format!("/api/v1/trees/{zero}/media/{zero}/thumbnail?v=0123456789abcdef"),
                format!("/api/v1/trees/{zero}/vignettes/{zero}/image?v=0123456789abcdef"),
                format!("/api/v1/trees/{zero}/media/{zero}/file?v=0123456789abcdef"),
                format!("/api/v1/trees/{zero}/media/{zero}/card?v=0123456789abcdef"),
                format!("/api/v1/trees/{zero}/vignettes/{zero}/card?v=0123456789abcdef"),
            ]
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
                media_id: Uuid::nil(),
                version: "0123456789abcdef".into(),
            }),
            Some(asset(AssetKind::Thumbnail, Uuid::nil()))
        );
    }
}
