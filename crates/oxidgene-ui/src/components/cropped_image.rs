//! Draw a picture, or one region of a picture, from a single source.
//!
//! Most images arrive ready to draw: a thumbnail we generated, a crop we cut,
//! a stored file. A region of a file we do *not* hold cannot arrive that way —
//! cutting it means re-decoding our own copy, and a remote file is never
//! fetched by us — so the whole picture arrives with the rectangle to take out
//! of it, and the cutting happens here, in the one place that has the pixels.
//!
//! # Why an `<svg>` and not a wrapper with CSS offsets
//!
//! An `svg` with `viewBox` set to the rectangle and
//! `preserveAspectRatio="xMidYMid slice"` *is* `object-fit: cover` over a crop:
//! the view box selects the region, `slice` scales it to cover the element and
//! clips the rest, and the whole thing scales with the element's CSS size with
//! no measurement, no script, and no second element to keep in sync. The same
//! markup nests inside the pedigree's own SVG, so one rule covers both.

use dioxus::prelude::*;
use oxidgene_core::types::ImageCrop;

use crate::api::CroppedSource;

/// A picture in an HTML context, cropped if it has to be.
///
/// `class` lands on whichever element is drawn, so the caller's sizing and
/// rounding apply either way. A caller that already crops with `object-fit`
/// gets the same framing here: the crop covers the box and is centred.
#[component]
pub fn CroppedImage(
    image: CroppedSource,
    alt: String,
    #[props(default)] class: Option<String>,
) -> Element {
    let CroppedSource { source, crop } = image;
    let Some(crop) = crop else {
        return rsx! {
            img { class, src: "{source}", alt, loading: "lazy" }
        };
    };
    rsx! {
        svg {
            class,
            "viewBox": "{view_box(&crop)}",
            "preserveAspectRatio": "xMidYMid slice",
            role: "img",
            "aria-label": "{alt}",
            image {
                "href": "{source}",
                x: "0",
                y: "0",
                width: "{crop.source_width}",
                height: "{crop.source_height}",
            }
        }
    }
}

/// The same picture inside an SVG document, placed at a given rectangle.
///
/// A nested `svg` rather than an `image`: it takes the same placement
/// attributes, and gives the crop its own view box so the region — not the
/// whole photograph — is what fills the frame.
#[component]
pub fn CroppedSvgImage(
    image: CroppedSource,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[props(default)] class: Option<String>,
) -> Element {
    let CroppedSource { source, crop } = image;
    let Some(crop) = crop else {
        return rsx! {
            image {
                class,
                "href": "{source}",
                x: "{x}",
                y: "{y}",
                width: "{width}",
                height: "{height}",
                "preserveAspectRatio": "xMidYMid slice",
            }
        };
    };
    rsx! {
        svg {
            class,
            x: "{x}",
            y: "{y}",
            width: "{width}",
            height: "{height}",
            "viewBox": "{view_box(&crop)}",
            "preserveAspectRatio": "xMidYMid slice",
            image {
                "href": "{source}",
                x: "0",
                y: "0",
                width: "{crop.source_width}",
                height: "{crop.source_height}",
            }
        }
    }
}

/// The region, in the picture's own pixel coordinates — which is exactly what
/// a `viewBox` is.
fn view_box(crop: &ImageCrop) -> String {
    format!("{} {} {} {}", crop.x, crop.y, crop.width, crop.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_box_is_the_region_in_source_pixels() {
        // What makes the crop a crop: the view box names the rectangle, and
        // the image inside it is laid out at its own natural size, so the two
        // agree on what a pixel is.
        let crop = ImageCrop {
            x: 120,
            y: 40,
            width: 200,
            height: 260,
            source_width: 1600,
            source_height: 1200,
        };

        assert_eq!(view_box(&crop), "120 40 200 260");
    }
}
