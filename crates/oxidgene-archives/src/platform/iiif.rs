//! IIIF Image API services: what OxidGene reads of an `info.json`, and the
//! [`ArchiveImage`] of a view built on an image-service base.
//!
//! The base is the address the adapter knows the service by on the portal's
//! own origin. It is never the `id`/`@id` the `info.json` declares, which on
//! some portals names an internal host. Image API 2 and 3 differ in the
//! keyword for the whole image (`full`, `max`) and in how they state their
//! compliance level; both are read.

use serde::Deserialize;

use crate::{ArchiveImage, ResolveError};

/// The longest side the viewer asks for where the service scales freely.
pub(crate) const PICTURE_BOUND: u32 = 2048;

/// The narrowest listed size a gallery tile is built from.
pub(crate) const THUMBNAIL_MIN_WIDTH: u32 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

/// What OxidGene reads of an image service's `info.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ImageInfo {
    pub(crate) width: u32,
    pub(crate) height: u32,
    #[serde(default)]
    sizes: Vec<Size>,
    /// Image API 2: a list whose first entry is the level's URL. Image API
    /// 3: the level itself, `level0` to `level2`.
    #[serde(default)]
    profile: serde_json::Value,
    #[serde(default, rename = "@context")]
    context: serde_json::Value,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

/// Reads an `info.json`; one without a pixel size is a changed answer.
pub(crate) fn image_info(answer: &str) -> Result<ImageInfo, ResolveError> {
    let info: ImageInfo = serde_json::from_str(answer).map_err(|_| unexpected())?;
    if info.width == 0 || info.height == 0 {
        return Err(unexpected());
    }
    Ok(info)
}

fn unexpected() -> ResolveError {
    ResolveError::UnexpectedResponse("iiif: info.json lacks the image size".to_owned())
}

impl ImageInfo {
    /// Whether the service follows Image API 3.
    fn is_version_3(&self) -> bool {
        self.kind.as_deref() == Some("ImageService3")
            || match &self.context {
                serde_json::Value::String(context) => context.contains("/image/3/"),
                serde_json::Value::Array(contexts) => contexts
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|context| context.contains("/image/3/")),
                _ => false,
            }
    }

    /// Whether the service scales to any size: level 2.
    fn scales_freely(&self) -> bool {
        let level = match &self.profile {
            serde_json::Value::Array(entries) => entries.first(),
            other => Some(other),
        };
        level
            .and_then(serde_json::Value::as_str)
            .is_some_and(|level| level == "level2" || level.ends_with("level2.json"))
    }

    /// The size keyword of the whole image.
    fn whole(&self) -> &'static str {
        if self.is_version_3() { "max" } else { "full" }
    }

    /// The size parameter of the picture: bounded to the screen where the
    /// service scales freely, the whole image otherwise.
    pub(crate) fn picture_size(&self) -> String {
        if self.scales_freely() && self.width.max(self.height) > PICTURE_BOUND {
            format!("!{PICTURE_BOUND},{PICTURE_BOUND}")
        } else {
            self.whole().to_owned()
        }
    }

    /// The size parameter of the thumbnail: the smallest listed size wide
    /// enough for a tile, which every level serves, or the whole image.
    pub(crate) fn thumbnail_size(&self) -> String {
        self.sizes
            .iter()
            .filter(|size| size.width >= THUMBNAIL_MIN_WIDTH)
            .min_by_key(|size| size.width)
            .or_else(|| self.sizes.iter().max_by_key(|size| size.width))
            .map_or_else(
                || self.whole().to_owned(),
                |size| format!("{},{}", size.width, size.height),
            )
    }

    /// The view's image on the service at `base` (no trailing slash), such as
    /// `https://archives.example.org/iiif/register/12`.
    pub(crate) fn image(&self, base: &str) -> ArchiveImage {
        ArchiveImage {
            picture: format!("{base}/full/{}/0/default.jpg", self.picture_size()),
            thumbnail: format!("{base}/full/{}/0/default.jpg", self.thumbnail_size()),
            width: self.width,
            height: self.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_view_on_a_level_2_service() {
        let info = image_info(
            r#"{"@context": "http://iiif.io/api/image/2/context.json",
                "@id": "http://internal-host.example.invalid/iiif/2/x",
                "width": 3352, "height": 2248,
                "sizes": [{"width": 105, "height": 70}, {"width": 210, "height": 141}, {"width": 419, "height": 281}],
                "profile": ["http://iiif.io/api/image/2/level2.json", {}]}"#,
        )
        .unwrap();
        assert_eq!(
            info.image("https://archives.example.org/iiif/12"),
            ArchiveImage {
                picture: "https://archives.example.org/iiif/12/full/!2048,2048/0/default.jpg"
                    .to_owned(),
                thumbnail: "https://archives.example.org/iiif/12/full/210,141/0/default.jpg"
                    .to_owned(),
                width: 3352,
                height: 2248,
            }
        );
    }

    #[test]
    fn falls_back_to_the_whole_image_below_level_2() {
        let level0 = image_info(
            r#"{"width": 1000, "height": 800, "profile": "http://iiif.io/api/image/2/level0.json"}"#,
        )
        .unwrap();
        assert_eq!(level0.picture_size(), "full");
        assert_eq!(level0.thumbnail_size(), "full");

        let version3 = image_info(
            r#"{"@context": "http://iiif.io/api/image/3/context.json", "type": "ImageService3",
                "id": "https://api.example.org/iiif/3/x", "width": 3000, "height": 2000,
                "profile": "level1", "sizes": [{"width": 100, "height": 66}]}"#,
        )
        .unwrap();
        assert_eq!(version3.picture_size(), "max");
        assert_eq!(version3.thumbnail_size(), "100,66");
        let level2 = image_info(
            r#"{"type": "ImageService3", "width": 3000, "height": 2000, "profile": "level2"}"#,
        )
        .unwrap();
        assert_eq!(level2.picture_size(), "!2048,2048");
        assert_eq!(level2.thumbnail_size(), "max");

        assert!(image_info(r#"{"width": 0, "height": 10}"#).is_err());
        assert!(image_info("not json").is_err());
    }
}
