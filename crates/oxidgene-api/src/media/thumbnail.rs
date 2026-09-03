//! Thumbnails and intrinsic dimensions for raster uploads.
//!
//! Every function here is CPU-bound and synchronous. Handlers call them from
//! `tokio::task::spawn_blocking`, so decoding a 40-megapixel scan does not
//! stall the request that is being served on the same worker thread.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use oxidgene_core::error::OxidGeneError;

/// Longest edge of a generated thumbnail, in pixels.
///
/// Sized for the gallery grid at 2× density: a 200 px card stays sharp on a
/// retina display without shipping the full scan to draw a contact sheet.
pub const THUMBNAIL_MAX_EDGE: u32 = 400;

/// Ceiling on the decoded pixel buffer, in bytes (1 GiB).
///
/// A few kilobytes of crafted PNG can claim 60000×60000 pixels, and a decoder
/// that believes it allocates 13 GiB before failing. The limit turns that into
/// a thumbnail that is not generated — [`super::ingest`] logs the failure and
/// stores the file anyway, so the guard costs a gallery icon rather than an
/// upload.
///
/// It has to clear what the upload ceiling now admits, or every large scan
/// would land in exactly that iconless state: a 1200 dpi colour spread decodes
/// to something under a gigabyte, a crafted bomb to many times one.
const MAX_DECODED_BYTES: u64 = 1024 * 1024 * 1024;

/// A generated thumbnail, ready to store.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    pub bytes: Vec<u8>,
    /// MIME type of `bytes` — `image/png` when the source had transparency to
    /// preserve, `image/jpeg` otherwise.
    pub mime_type: &'static str,
    /// Extension matching `mime_type`, for the store key.
    pub extension: &'static str,
    pub width: u32,
    pub height: u32,
}

/// Intrinsic pixel dimensions of an uploaded image, if it is one we can read.
///
/// Reads headers only — a 200 MB TIFF costs a few hundred bytes of IO here.
/// Returns `None` for anything that is not a raster image we can decode,
/// which includes PDFs and every unrecognised format.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let (width, height) = decoder.dimensions();
    // Orientation is metadata, not pixels: a portrait photo written by a
    // camera that rotated it in EXIF reports landscape dimensions. Report what
    // a viewer will see, matching what `generate` produces.
    if swaps_axes(&mut decoder) {
        Some((height, width))
    } else {
        Some((width, height))
    }
}

/// Whether the decoder's EXIF orientation transposes width and height.
fn swaps_axes(decoder: &mut impl ImageDecoder) -> bool {
    use image::metadata::Orientation;
    matches!(
        decoder.orientation().unwrap_or(Orientation::NoTransforms),
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Whether a thumbnail can be generated for this MIME type.
///
/// PDFs answer `false`: rasterising a page needs a rendering engine (pdfium,
/// mupdf) that is a C dependency, and OxidGene ships a desktop binary for
/// three platforms. Documents get a page count and an icon instead, and the
/// UI is written against `thumbnail_key` being absent.
pub fn can_thumbnail(mime_type: &str) -> bool {
    matches!(
        mime_type,
        "image/jpeg"
            | "image/png"
            | "image/gif"
            | "image/bmp"
            | "image/tiff"
            | "image/webp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

/// Decode `bytes` and scale them down to fit inside [`THUMBNAIL_MAX_EDGE`].
///
/// An image already smaller than the box is re-encoded rather than scaled up:
/// the thumbnail endpoint should always be able to answer with one format the
/// gallery understands, even when the source is a 64×64 GIF.
pub fn generate(bytes: &[u8]) -> Result<Thumbnail, OxidGeneError> {
    let (image, source_format) = decode(bytes, MAX_DECODED_BYTES)?;

    // `thumbnail` fits to the box in both directions, so a small source would
    // come back enlarged and blurry. Only shrink.
    let thumb = if image.width() > THUMBNAIL_MAX_EDGE || image.height() > THUMBNAIL_MAX_EDGE {
        image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
    } else {
        image
    };
    let (width, height) = (thumb.width(), thumb.height());

    // Only formats that can actually carry transparency get PNG. Flattening a
    // logo with a transparent background onto JPEG's implicit black is the
    // kind of thing nobody notices until it is in a printed family book.
    let keep_alpha = matches!(
        source_format,
        Some(ImageFormat::Png) | Some(ImageFormat::Gif) | Some(ImageFormat::WebP)
    ) && thumb.color().has_alpha();

    let mut out = Cursor::new(Vec::new());
    let (mime_type, extension) = if keep_alpha {
        thumb
            .into_rgba8()
            .write_to(&mut out, ImageFormat::Png)
            .map_err(|e| OxidGeneError::Internal(format!("thumbnail encode failed: {e}")))?;
        ("image/png", "png")
    } else {
        thumb
            .into_rgb8()
            .write_to(&mut out, ImageFormat::Jpeg)
            .map_err(|e| OxidGeneError::Internal(format!("thumbnail encode failed: {e}")))?;
        ("image/jpeg", "jpg")
    };

    Ok(Thumbnail {
        bytes: out.into_inner(),
        mime_type,
        extension,
        width,
        height,
    })
}

/// Decode and crop one image region, returning standalone JPEG bytes.
pub fn crop(
    bytes: &[u8],
    (x, y, width, height): (i32, i32, i32, i32),
) -> Result<Vec<u8>, OxidGeneError> {
    let (image, _) = decode(bytes, MAX_DECODED_BYTES)?;
    let cropped = DynamicImage::crop_imm(
        &image,
        x.max(0) as u32,
        y.max(0) as u32,
        width.max(1) as u32,
        height.max(1) as u32,
    );

    let mut out = Cursor::new(Vec::new());
    cropped
        .into_rgb8()
        .write_to(&mut out, ImageFormat::Jpeg)
        .map_err(|e| OxidGeneError::Internal(format!("could not encode: {e}")))?;
    Ok(out.into_inner())
}

pub(crate) fn decode(
    bytes: &[u8],
    max_decoded_bytes: u64,
) -> Result<(DynamicImage, Option<ImageFormat>), OxidGeneError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| OxidGeneError::Validation(format!("unreadable image: {e}")))?;

    let source_format = reader.format();
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| OxidGeneError::Validation(format!("unsupported image: {e}")))?;
    if decoder.total_bytes() > max_decoded_bytes {
        return Err(OxidGeneError::Validation(
            "image too large to decode".to_string(),
        ));
    }

    let mut limits = image::Limits::default();
    limits.max_alloc = Some(max_decoded_bytes);
    decoder
        .set_limits(limits)
        .map_err(|e| OxidGeneError::Validation(format!("image too large to decode: {e}")))?;

    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)
        .map_err(|e| OxidGeneError::Validation(format!("could not decode image: {e}")))?;
    image.apply_orientation(orientation);
    Ok((image, source_format))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn png_with_alpha(width: u32, height: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 128, 64]);
        }
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut img = RgbImage::new(width, height);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgb([(x % 256) as u8, (y % 256) as u8, 200]);
        }
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Jpeg).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_wide_image_is_scaled_to_fit_the_box_and_keeps_its_ratio() {
        let thumb = generate(&jpeg(1600, 800)).unwrap();
        assert_eq!(thumb.width, THUMBNAIL_MAX_EDGE);
        assert_eq!(thumb.height, THUMBNAIL_MAX_EDGE / 2);
        assert_eq!(thumb.mime_type, "image/jpeg");
    }

    #[test]
    fn a_tall_image_is_bounded_by_its_height() {
        let thumb = generate(&jpeg(300, 1200)).unwrap();
        assert_eq!(thumb.height, THUMBNAIL_MAX_EDGE);
        assert_eq!(thumb.width, 100);
    }

    #[test]
    fn an_image_smaller_than_the_box_is_not_scaled_up() {
        let thumb = generate(&jpeg(64, 48)).unwrap();
        assert_eq!((thumb.width, thumb.height), (64, 48));
    }

    #[test]
    fn a_transparent_source_stays_png() {
        let thumb = generate(&png_with_alpha(500, 500)).unwrap();
        assert_eq!(thumb.mime_type, "image/png");
        assert_eq!(thumb.extension, "png");
    }

    #[test]
    fn an_opaque_source_becomes_jpeg() {
        let thumb = generate(&jpeg(500, 500)).unwrap();
        assert_eq!(thumb.mime_type, "image/jpeg");
        assert_eq!(thumb.extension, "jpg");
    }

    #[test]
    fn a_generated_thumbnail_is_itself_decodable() {
        let thumb = generate(&jpeg(900, 600)).unwrap();
        assert_eq!(
            dimensions(&thumb.bytes),
            Some((thumb.width, thumb.height)),
            "the gallery has to be able to read what we stored"
        );
    }

    #[test]
    fn dimensions_come_from_the_header_not_a_full_decode() {
        assert_eq!(dimensions(&jpeg(1234, 567)), Some((1234, 567)));
    }

    #[test]
    fn decoded_pixels_must_fit_the_memory_budget() {
        let error = decode(&jpeg(64, 48), 1).expect_err("exceeds one byte");
        assert!(matches!(error, OxidGeneError::Validation(_)));
    }

    #[test]
    fn a_pdf_has_no_dimensions_and_no_thumbnail() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\n";
        assert_eq!(dimensions(pdf), None);
        assert!(!can_thumbnail("application/pdf"));
        assert!(generate(pdf).is_err());
    }

    #[test]
    fn garbage_is_rejected_rather_than_stored_as_a_broken_thumbnail() {
        let err = generate(b"this is not an image at all").unwrap_err();
        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn every_thumbnailable_mime_type_can_actually_be_decoded() {
        // Guards against `can_thumbnail` promising a format the build's
        // `image` features do not include — the failure mode would be an
        // upload accepted, then a thumbnail that never appears.
        for (mime, format) in [
            ("image/jpeg", ImageFormat::Jpeg),
            ("image/png", ImageFormat::Png),
            ("image/gif", ImageFormat::Gif),
            ("image/bmp", ImageFormat::Bmp),
            ("image/tiff", ImageFormat::Tiff),
            ("image/webp", ImageFormat::WebP),
            ("image/x-icon", ImageFormat::Ico),
        ] {
            assert!(can_thumbnail(mime));
            assert!(
                format.reading_enabled(),
                "{mime} is advertised but {format:?} decoding is not compiled in"
            );
        }
    }
}
