//! Thumbnails and intrinsic dimensions for raster uploads, and the thumbnail
//! of a scanned PDF.
//!
//! Every function here is CPU-bound and synchronous. Handlers call them from
//! `tokio::task::spawn_blocking`, so decoding a 40-megapixel scan does not
//! stall the request that is being served on the same worker thread.

use std::borrow::Cow;
use std::io::Cursor;

use hayro_syntax::content::ops::{TypedInstruction, XObject};
use hayro_syntax::object::dict::keys::{
    COLUMNS, DECODE_PARMS, DP, HEIGHT, IMAGE, IMAGE_MASK, ROWS, SUBTYPE, WIDTH,
};
use hayro_syntax::object::stream::{ImageColorSpace, ImageData, ImageDecodeParams};
use hayro_syntax::object::{Array, Dict, Name, Stream};
use hayro_syntax::page::{Page, Rotation};
use hayro_syntax::{Filter, LoadPdfError, Pdf};
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

/// Ceiling on the pixels one vignette request decodes, in bytes (128 MiB).
///
/// A thumbnail is made once, when the file arrives, so it may afford
/// [`MAX_DECODED_BYTES`]. A vignette is cut on every request that misses the
/// client's cache, so a gigabyte per request would let a handful of requests
/// exhaust the server's memory. 128 MiB is a 6000 × 7000 colour scan at full
/// resolution; a larger JPEG is decoded at a half, a quarter or an eighth of
/// its size — its decoder can skip the detail it would only throw away — and
/// anything else that large is cut from its stored thumbnail.
const MAX_CROP_DECODED_BYTES: u64 = 128 * 1024 * 1024;

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

/// Whether a file of this MIME type may have a raster preview: a thumbnail
/// the gallery draws instead of an icon.
///
/// Every raster format we decode, and PDFs: a scanned PDF gets a thumbnail of
/// the image its first page paints ([`generate_pdf`]). Whether one was made is
/// `thumbnail_key`; this only says whether it is worth trying.
pub fn may_preview(mime_type: &str) -> bool {
    can_crop(mime_type) || oxidgene_core::types::is_pdf_mime(mime_type)
}

/// Whether the original itself is a raster image a region can be cut from.
///
/// Crops are raster-only. A PDF's thumbnail is a picture of one image of its
/// first page, not of the page, so a rectangle drawn on the document has
/// nothing it can be cut from.
pub fn can_crop(mime_type: &str) -> bool {
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
    encode_thumbnail(shrink(image), source_format)
}

/// `image` fitted inside [`THUMBNAIL_MAX_EDGE`], never enlarged.
fn shrink(image: DynamicImage) -> DynamicImage {
    // `thumbnail` fits to the box in both directions, so a small source would
    // come back enlarged and blurry. Only shrink.
    if image.width() > THUMBNAIL_MAX_EDGE || image.height() > THUMBNAIL_MAX_EDGE {
        image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
    } else {
        image
    }
}

/// Whether a pixel of `image` is not fully opaque. A layout this does not
/// know is taken as transparent when it has an alpha channel, the safe side.
fn is_transparent(image: &DynamicImage) -> bool {
    match image {
        DynamicImage::ImageLumaA8(pixels) => pixels.pixels().any(|p| p.0[1] < u8::MAX),
        DynamicImage::ImageRgba8(pixels) => pixels.pixels().any(|p| p.0[3] < u8::MAX),
        DynamicImage::ImageLumaA16(pixels) => pixels.pixels().any(|p| p.0[1] < u16::MAX),
        DynamicImage::ImageRgba16(pixels) => pixels.pixels().any(|p| p.0[3] < u16::MAX),
        DynamicImage::ImageRgba32F(pixels) => pixels.pixels().any(|p| p.0[3] < 1.0),
        other => other.color().has_alpha(),
    }
}

/// `thumb` encoded for storage: PNG when a source of `source_format` had
/// transparency to keep, JPEG otherwise.
fn encode_thumbnail(
    thumb: DynamicImage,
    source_format: Option<ImageFormat>,
) -> Result<Thumbnail, OxidGeneError> {
    let (width, height) = (thumb.width(), thumb.height());

    // Only formats that can actually carry transparency get PNG. Flattening a
    // logo with a transparent background onto JPEG's implicit black is the
    // kind of thing nobody notices until it is in a printed family book.
    //
    // An alpha channel is not transparency, though: photographs and scans
    // saved as PNG by many tools carry one that is opaque everywhere, and a
    // 400 px photograph as PNG weighs ten times its JPEG — measured on a real
    // tree, a pedigree's portraits went from 0.3 MB apiece to 25 kB. So the
    // pixels decide, not the channel layout.
    let keep_alpha = matches!(
        source_format,
        Some(ImageFormat::Png) | Some(ImageFormat::Gif) | Some(ImageFormat::WebP)
    ) && is_transparent(&thumb);

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

/// Why a PDF has no thumbnail. Each names a case, never anything the file
/// holds, so it can go into a log as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoPdfThumbnail {
    /// It opens only with a password.
    Encrypted,
    /// Not a PDF the parser can read, or one without a page.
    Unreadable,
    /// Its first page paints no image: a text or vector page.
    NoImage,
    /// The image declares more pixels than the decoding ceiling holds.
    TooLarge,
    /// The image is compressed some way not decoded here: Flate, LZW, or a
    /// chain of filters.
    UnsupportedEncoding,
    /// The image could not be decoded, or its pixels have no plain grey or
    /// RGB reading.
    Undecodable,
}

impl NoPdfThumbnail {
    /// A short, stable name for the log.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Encrypted => "encrypted",
            Self::Unreadable => "unreadable",
            Self::NoImage => "no_image",
            Self::TooLarge => "too_large",
            Self::UnsupportedEncoding => "unsupported_encoding",
            Self::Undecodable => "undecodable",
        }
    }
}

impl From<&LoadPdfError> for NoPdfThumbnail {
    fn from(error: &LoadPdfError) -> Self {
        match error {
            LoadPdfError::Decryption(_) => Self::Encrypted,
            LoadPdfError::Invalid => Self::Unreadable,
        }
    }
}

/// The thumbnail of a scanned PDF: the largest image its first page paints,
/// turned as the page's `/Rotate` turns it.
///
/// OxidGene ships no PDF renderer, so a page is not drawn: the thumbnail is
/// the scan the page is made of. That is what a register, an act or a
/// census return uploaded as PDF almost always is, and a text or vector page
/// has none, so it keeps its icon. The image's placement on the page, its
/// clipping and masks are ignored.
///
/// A JPEG goes to [`generate`] as it is stored. CCITT fax, JBIG2 and JPEG 2000
/// images are decoded by `hayro-syntax`. Nothing is decoded before the
/// image's declared size is checked against the decoding ceiling.
pub fn generate_pdf(pdf: &Pdf) -> Result<Thumbnail, NoPdfThumbnail> {
    let page = pdf.pages().first().ok_or(NoPdfThumbnail::Unreadable)?;
    let image = PdfImage::largest_painted(page).ok_or(NoPdfThumbnail::NoImage)?;
    if image.decoded_bytes() > MAX_DECODED_BYTES {
        return Err(NoPdfThumbnail::TooLarge);
    }
    let picture = match image.stream.filters().as_slice() {
        [Filter::DctDecode] => {
            decode(&image.stream.raw_data(), MAX_DECODED_BYTES)
                .map_err(|_| NoPdfThumbnail::Undecodable)?
                .0
        }
        [Filter::CcittFaxDecode | Filter::Jbig2Decode | Filter::JpxDecode] => image.decoded()?,
        _ => return Err(NoPdfThumbnail::UnsupportedEncoding),
    };
    encode_thumbnail(turned(shrink(picture), page.rotation()), None)
        .map_err(|_| NoPdfThumbnail::Undecodable)
}

/// `image` turned clockwise as a page's `/Rotate` says it is displayed.
fn turned(image: DynamicImage, rotation: Rotation) -> DynamicImage {
    match rotation {
        Rotation::None => image,
        Rotation::Horizontal => image.rotate90(),
        Rotation::Flipped => image.rotate180(),
        Rotation::FlippedHorizontal => image.rotate270(),
    }
}

/// An image XObject of a page, and the size its dictionary declares.
struct PdfImage<'a> {
    stream: Stream<'a>,
    width: u32,
    height: u32,
}

impl<'a> PdfImage<'a> {
    /// The image `stream` is, unless it is something else, a stencil mask,
    /// or declares no size.
    fn of(stream: Stream<'a>) -> Option<Self> {
        let dict = stream.dict();
        let is_image = dict
            .get::<Name<'_>>(SUBTYPE)
            .is_some_and(|subtype| &*subtype == IMAGE);
        let is_mask = dict.get::<bool>(IMAGE_MASK).unwrap_or(false);
        let width = dict.get::<u32>(WIDTH).filter(|side| *side > 0)?;
        let height = dict.get::<u32>(HEIGHT).filter(|side| *side > 0)?;
        (is_image && !is_mask).then_some(Self {
            stream,
            width,
            height,
        })
    }

    /// The largest image, by declared area, that `page`'s content paints.
    ///
    /// Only what the page draws counts: resources are often inherited from
    /// the page tree, where one dictionary lists the scans of every page.
    fn largest_painted(page: &Page<'a>) -> Option<Self> {
        let mut operations = page.typed_operations();
        let mut largest: Option<Self> = None;
        while let Some(operation) = operations.next() {
            let TypedInstruction::XObject(XObject(name)) = operation else {
                continue;
            };
            let Some(image) = page.resources().get_x_object(name).and_then(Self::of) else {
                continue;
            };
            if largest
                .as_ref()
                .is_none_or(|kept| image.area() > kept.area())
            {
                largest = Some(image);
            }
        }
        largest
    }

    fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// The most bytes its decoder could allocate, from what it declares.
    ///
    /// Four bytes a pixel, the widest any decoder here produces (CMYK or RGB
    /// with alpha, eight bits each). A fax image is decoded onto the grid its
    /// own parameters declare, which can be wider than `/Width`, so the larger
    /// of the two counts.
    fn decoded_bytes(&self) -> u64 {
        let params = self.decode_parms();
        let fax_grid = params.as_ref().map_or(0, |params| {
            let columns = params.get::<u32>(COLUMNS).unwrap_or(1728);
            let rows = params.get::<u32>(ROWS).unwrap_or(0).max(self.height);
            u64::from(columns) * u64::from(rows)
        });
        self.area().max(fax_grid).saturating_mul(4)
    }

    /// The image's decode parameters, for its one filter.
    fn decode_parms(&self) -> Option<Dict<'a>> {
        let dict = self.stream.dict();
        dict.get::<Dict<'a>>(DECODE_PARMS)
            .or_else(|| dict.get::<Dict<'a>>(DP))
            .or_else(|| {
                dict.get::<Array<'a>>(DECODE_PARMS)
                    .and_then(|parms| parms.iter::<Dict<'a>>().next())
            })
    }

    /// The image decoded by `hayro-syntax`, as a grey or RGB picture.
    ///
    /// A JPEG 2000 image is asked for at the resolution level nearest a
    /// thumbnail above it, which skips decoding detail that would only be
    /// thrown away.
    fn decoded(&self) -> Result<DynamicImage, NoPdfThumbnail> {
        let params = ImageDecodeParams {
            bpc: Some(8),
            target_dimension: Some((THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)),
            width: self.width,
            height: self.height,
            ..ImageDecodeParams::default()
        };
        let result = self
            .stream
            .decoded_image(&params)
            .map_err(|_| NoPdfThumbnail::Undecodable)?;
        let data = result.image_data.ok_or(NoPdfThumbnail::Undecodable)?;
        picture(result.data, &data).ok_or(NoPdfThumbnail::Undecodable)
    }
}

/// Eight-bit grey or RGB `pixels` as described by `data`, or `None` for any
/// other layout.
fn picture(pixels: Cow<'_, [u8]>, data: &ImageData) -> Option<DynamicImage> {
    if data.bits_per_component != 8 {
        return None;
    }
    let (width, height) = (data.width, data.height);
    let channels: usize = match data.color_space? {
        ImageColorSpace::Gray => 1,
        ImageColorSpace::Rgb => 3,
        _ => return None,
    };
    let needed = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(channels)?;
    let mut pixels = pixels.into_owned();
    if pixels.len() < needed {
        return None;
    }
    pixels.truncate(needed);
    if channels == 1 {
        image::GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
    } else {
        image::RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
    }
}

/// A source too large to decode whole for a crop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLargeToCrop;

/// Decode and crop one image region, returning standalone JPEG bytes.
///
/// The rectangle is in the image's own pixels, as it is displayed (EXIF
/// orientation applied). At most `MAX_CROP_DECODED_BYTES` are decoded: a
/// larger JPEG is decoded at a reduced scale and the rectangle scaled with
/// it, so the vignette is smaller but sharp. A larger image of any other
/// kind answers `Ok(Err(TooLargeToCrop))`, for the caller to cut the
/// rectangle out of the stored thumbnail instead ([`crop_scaled`]).
///
/// Decoding and encoding are each spanned (`media.decode`, `media.encode`):
/// they are the CPU cost of a crop, and which one dominates depends on the
/// source format and size.
pub fn crop(
    bytes: &[u8],
    rect: (i32, i32, i32, i32),
) -> Result<Result<Vec<u8>, TooLargeToCrop>, OxidGeneError> {
    crop_within(bytes, rect, MAX_CROP_DECODED_BYTES)
}

/// [`crop`], decoding at most `max_decoded` bytes.
fn crop_within(
    bytes: &[u8],
    rect: (i32, i32, i32, i32),
    max_decoded: u64,
) -> Result<Result<Vec<u8>, TooLargeToCrop>, OxidGeneError> {
    let decoded = tracing::info_span!("media.decode", media.input_bytes = bytes.len())
        .in_scope(|| decode_for_crop(bytes, max_decoded))?;
    Ok(match decoded {
        Some((image, scale)) => encode_crop(&image, scaled_rect(rect, scale)).map(Ok)?,
        None => Err(TooLargeToCrop),
    })
}

/// Crop `rect`, given in the pixels of a `full_width`-wide original, out of
/// `bytes`, a smaller rendition of it such as its thumbnail.
pub fn crop_scaled(
    bytes: &[u8],
    rect: (i32, i32, i32, i32),
    full_width: u32,
) -> Result<Vec<u8>, OxidGeneError> {
    let (image, _) = tracing::info_span!("media.decode", media.input_bytes = bytes.len())
        .in_scope(|| decode(bytes, MAX_CROP_DECODED_BYTES))?;
    let scale = f64::from(image.width()) / f64::from(full_width.max(1));
    encode_crop(&image, scaled_rect(rect, scale))
}

/// `rect` scaled by `scale`, at least one pixel wide and high.
fn scaled_rect((x, y, width, height): (i32, i32, i32, i32), scale: f64) -> (u32, u32, u32, u32) {
    let at = |value: i32| (f64::from(value.max(0)) * scale).floor() as u32;
    let size = |value: i32| ((f64::from(value.max(1)) * scale).round() as u32).max(1);
    (at(x), at(y), size(width), size(height))
}

/// The image to crop and the scale it was decoded at, or `None` when it is
/// too large and no reduced decode applies.
fn decode_for_crop(
    bytes: &[u8],
    max_decoded: u64,
) -> Result<Option<(DynamicImage, f64)>, OxidGeneError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| OxidGeneError::Validation(format!("unreadable image: {e}")))?;
    let format = reader.format();
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| OxidGeneError::Validation(format!("unsupported image: {e}")))?;
    let total = decoder.total_bytes();
    if total <= max_decoded {
        return decode(bytes, max_decoded).map(|(image, _)| Some((image, 1.0)));
    }
    if format != Some(ImageFormat::Jpeg) {
        return Ok(None);
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    Ok(
        decode_jpeg_reduced(bytes, total, max_decoded)?.map(|(mut image, scale)| {
            image.apply_orientation(orientation);
            (image, scale)
        }),
    )
}

/// A JPEG of `total` decoded bytes, decoded at the largest of ½, ¼ and ⅛ of
/// its size that fits `max_decoded`, and that scale; `None` when none fits or
/// its pixel format has no plain RGB or grey rendering.
fn decode_jpeg_reduced(
    bytes: &[u8],
    total: u64,
    max_decoded: u64,
) -> Result<Option<(DynamicImage, f64)>, OxidGeneError> {
    let Some(divisor) = [2u64, 4, 8]
        .into_iter()
        .find(|divisor| total / (divisor * divisor) <= max_decoded)
    else {
        return Ok(None);
    };
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    // Progressive files hold every coefficient whatever the output scale:
    // bound that too, and fall back to the thumbnail past it.
    decoder.set_max_decoding_buffer_size(usize::try_from(max_decoded).unwrap_or(usize::MAX));
    decoder
        .read_info()
        .map_err(|e| OxidGeneError::Validation(format!("unreadable image: {e}")))?;
    let Some(info) = decoder.info() else {
        return Ok(None);
    };
    let wanted = |side: u16| (u64::from(side) / divisor).max(1) as u16;
    let Ok((width, height)) = decoder.scale(wanted(info.width), wanted(info.height)) else {
        return Ok(None);
    };
    let Ok(pixels) = decoder.decode() else {
        return Ok(None);
    };
    let (width32, height32) = (u32::from(width), u32::from(height));
    let image = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => {
            image::RgbImage::from_raw(width32, height32, pixels).map(DynamicImage::ImageRgb8)
        }
        jpeg_decoder::PixelFormat::L8 => {
            image::GrayImage::from_raw(width32, height32, pixels).map(DynamicImage::ImageLuma8)
        }
        _ => None,
    };
    Ok(image.map(|image| (image, f64::from(width) / f64::from(info.width))))
}

/// `image` cut to `(x, y, width, height)`, as JPEG.
fn encode_crop(
    image: &DynamicImage,
    (x, y, width, height): (u32, u32, u32, u32),
) -> Result<Vec<u8>, OxidGeneError> {
    let cropped = DynamicImage::crop_imm(image, x, y, width, height);
    // A crop is only ever drawn as a thumbnail — a portrait on a card, a tile
    // in a gallery — so it is sent at thumbnail size. A face marked on a
    // high-resolution scan can be two thousand pixels wide, and shipping that
    // for a sixty-pixel card cost the bytes and the client's decoding.
    let cropped = if cropped.width() > THUMBNAIL_MAX_EDGE || cropped.height() > THUMBNAIL_MAX_EDGE {
        cropped.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE)
    } else {
        cropped
    };

    let encode = tracing::info_span!("media.encode", media.output_bytes = tracing::field::Empty);
    let _entered = encode.enter();
    let mut out = Cursor::new(Vec::new());
    cropped
        .into_rgb8()
        .write_to(&mut out, ImageFormat::Jpeg)
        .map_err(|e| OxidGeneError::Internal(format!("could not encode: {e}")))?;
    let out = out.into_inner();
    encode.record("media.output_bytes", out.len());
    Ok(out)
}

/// [`crop`] on the blocking pool, under a `media.crop` span of the caller's.
pub async fn crop_off_thread(
    bytes: Vec<u8>,
    rect: (i32, i32, i32, i32),
) -> Result<Result<Vec<u8>, TooLargeToCrop>, OxidGeneError> {
    let span = tracing::info_span!("media.crop", media.input_bytes = bytes.len());
    crate::service::blocking::run(span, move || crop(&bytes, rect)).await?
}

/// [`crop_scaled`] on the blocking pool, under a `media.crop` span.
pub async fn crop_scaled_off_thread(
    bytes: Vec<u8>,
    rect: (i32, i32, i32, i32),
    full_width: u32,
) -> Result<Vec<u8>, OxidGeneError> {
    let span = tracing::info_span!("media.crop", media.input_bytes = bytes.len());
    crate::service::blocking::run(span, move || crop_scaled(&bytes, rect, full_width)).await?
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
    use crate::media::test_pdf;
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
    fn a_png_whose_alpha_channel_is_opaque_everywhere_becomes_jpeg() {
        let mut img = RgbaImage::new(500, 500);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x % 256) as u8, (y % 256) as u8, 90, u8::MAX]);
        }
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, ImageFormat::Png).unwrap();
        let thumb = generate(&png.into_inner()).unwrap();
        assert_eq!(thumb.mime_type, "image/jpeg");
        assert_eq!(thumb.extension, "jpg");
    }

    #[test]
    fn a_png_with_a_single_transparent_pixel_stays_png() {
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([10, 20, 30, u8::MAX]));
        img.put_pixel(39, 39, Rgba([10, 20, 30, 0]));
        let mut png = Cursor::new(Vec::new());
        img.write_to(&mut png, ImageFormat::Png).unwrap();
        assert_eq!(generate(&png.into_inner()).unwrap().mime_type, "image/png");
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

    fn pdf_thumbnail(pdf: Vec<u8>) -> Result<Thumbnail, NoPdfThumbnail> {
        let pdf = Pdf::new(pdf);
        pdf.as_ref()
            .map_err(NoPdfThumbnail::from)
            .and_then(generate_pdf)
    }

    fn one_page(images: Vec<test_pdf::Image<'_>>, rotate: i32) -> Vec<u8> {
        test_pdf::build(
            &[test_pdf::Page {
                images,
                rotate,
                ..test_pdf::Page::default()
            }],
            "",
        )
    }

    #[test]
    fn a_pdf_has_no_pixel_size_and_is_not_decoded_as_a_picture() {
        let pdf = test_pdf::scan(&jpeg(64, 48), 64, 48);
        assert_eq!(dimensions(&pdf), None);
        assert!(generate(&pdf).is_err());
    }

    #[test]
    fn a_pdf_may_have_a_preview_but_is_never_cropped() {
        assert!(may_preview("application/pdf"));
        assert!(!can_crop("application/pdf"));
        assert!(may_preview("image/jpeg") && can_crop("image/jpeg"));
        assert!(!may_preview("video/mp4"));
    }

    #[test]
    fn a_scanned_pdf_is_thumbnailed_from_the_scan_its_first_page_paints() {
        let thumb = pdf_thumbnail(test_pdf::scan(&jpeg(800, 600), 800, 600)).unwrap();
        assert_eq!((thumb.width, thumb.height), (400, 300));
        assert_eq!(thumb.mime_type, "image/jpeg");
        assert_eq!(dimensions(&thumb.bytes), Some((400, 300)));
    }

    #[test]
    fn the_largest_image_the_page_paints_is_the_one_drawn() {
        let (logo, scan, unpainted) = (jpeg(32, 32), jpeg(600, 800), jpeg(1000, 1000));
        let pdf = one_page(
            vec![
                test_pdf::Image::jpeg(&logo, 32, 32),
                test_pdf::Image::jpeg(&scan, 600, 800),
                // Listed in the resources, never drawn: another page's scan.
                test_pdf::Image {
                    painted: false,
                    ..test_pdf::Image::jpeg(&unpainted, 1000, 1000)
                },
            ],
            0,
        );
        let thumb = pdf_thumbnail(pdf).unwrap();
        assert_eq!((thumb.width, thumb.height), (300, 400));
    }

    #[test]
    fn a_page_turned_by_its_rotate_entry_turns_its_thumbnail() {
        let scan = jpeg(64, 32);
        for (rotate, size) in [
            (0, (64, 32)),
            (90, (32, 64)),
            (180, (64, 32)),
            (-90, (32, 64)),
        ] {
            let pdf = one_page(vec![test_pdf::Image::jpeg(&scan, 64, 32)], rotate);
            let thumb = pdf_thumbnail(pdf).unwrap();
            assert_eq!((thumb.width, thumb.height), size, "/Rotate {rotate}");
        }
    }

    #[test]
    fn a_fax_encoded_scan_is_decoded_into_a_thumbnail() {
        // CCITT group 4, sixteen white rows of 16 pixels: against an all-white
        // reference line each row is one vertical-mode code, a single 1 bit.
        let pdf = one_page(
            vec![test_pdf::Image {
                filter: "CCITTFaxDecode",
                width: 16,
                height: 16,
                extra: "/ColorSpace /DeviceGray /BitsPerComponent 1 \
                        /DecodeParms << /K -1 /Columns 16 /Rows 16 >>",
                data: &[0xFF, 0xFF],
                painted: true,
            }],
            0,
        );
        let thumb = pdf_thumbnail(pdf).unwrap();
        assert_eq!((thumb.width, thumb.height), (16, 16));
    }

    #[test]
    fn a_text_or_vector_pdf_has_no_thumbnail() {
        assert_eq!(
            pdf_thumbnail(test_pdf::text(2)).unwrap_err(),
            NoPdfThumbnail::NoImage
        );
    }

    #[test]
    fn a_locked_or_broken_pdf_has_no_thumbnail() {
        assert_eq!(
            pdf_thumbnail(test_pdf::locked()).unwrap_err(),
            NoPdfThumbnail::Encrypted
        );
        assert_eq!(
            pdf_thumbnail(b"%PDF-1.4\ntruncated".to_vec()).unwrap_err(),
            NoPdfThumbnail::Unreadable
        );
    }

    #[test]
    fn an_image_declaring_too_many_pixels_is_refused_before_any_decoding() {
        // Not a JPEG at all: decoding it would fail as undecodable, so only a
        // refusal made from the declared size answers `TooLarge`.
        let pdf = one_page(
            vec![test_pdf::Image::jpeg(b"not a jpeg", 100_000, 100_000)],
            0,
        );
        assert_eq!(pdf_thumbnail(pdf).unwrap_err(), NoPdfThumbnail::TooLarge);
    }

    #[test]
    fn a_fax_grid_wider_than_the_image_declares_counts_against_the_limit() {
        let pdf = one_page(
            vec![test_pdf::Image {
                filter: "CCITTFaxDecode",
                width: 16,
                height: 16,
                extra: "/DecodeParms << /K -1 /Columns 100000 /Rows 100000 >>",
                data: &[0xFF],
                painted: true,
            }],
            0,
        );
        assert_eq!(pdf_thumbnail(pdf).unwrap_err(), NoPdfThumbnail::TooLarge);
    }

    #[test]
    fn an_image_compressed_some_other_way_is_not_decoded() {
        let pdf = one_page(
            vec![test_pdf::Image {
                filter: "FlateDecode",
                ..test_pdf::Image::jpeg(b"x", 16, 16)
            }],
            0,
        );
        assert_eq!(
            pdf_thumbnail(pdf).unwrap_err(),
            NoPdfThumbnail::UnsupportedEncoding
        );
    }

    #[test]
    fn garbage_is_rejected_rather_than_stored_as_a_broken_thumbnail() {
        let err = generate(b"this is not an image at all").unwrap_err();
        assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");
    }

    #[test]
    fn every_croppable_mime_type_can_actually_be_decoded() {
        // Guards against `can_crop` promising a format the build's `image`
        // features do not include — the failure mode would be an upload
        // accepted, then a thumbnail that never appears and a crop refused.
        for (mime, format) in [
            ("image/jpeg", ImageFormat::Jpeg),
            ("image/png", ImageFormat::Png),
            ("image/gif", ImageFormat::Gif),
            ("image/bmp", ImageFormat::Bmp),
            ("image/tiff", ImageFormat::Tiff),
            ("image/webp", ImageFormat::WebP),
            ("image/x-icon", ImageFormat::Ico),
        ] {
            assert!(can_crop(mime) && may_preview(mime));
            assert!(
                format.reading_enabled(),
                "{mime} is advertised but {format:?} decoding is not compiled in"
            );
        }
    }

    /// A span's name and its parent's name.
    type Opened = (String, Option<String>);

    /// Each span opened.
    #[derive(Clone, Default)]
    struct Spans(std::sync::Arc<std::sync::Mutex<Vec<Opened>>>);

    impl<S> tracing_subscriber::Layer<S> for Spans
    where
        S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            context: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let parent = attributes
                .parent()
                .and_then(|parent| context.span(parent))
                .or_else(|| context.lookup_current())
                .map(|span| span.name().to_string());
            self.0
                .lock()
                .expect("capture lock")
                .push((attributes.metadata().name().to_string(), parent));
        }
    }

    fn size_of(jpeg_bytes: &[u8]) -> (u32, u32) {
        dimensions(jpeg_bytes).expect("a readable JPEG")
    }

    #[test]
    fn a_crop_of_a_small_image_is_cut_at_full_resolution() {
        let cropped = crop(&jpeg(64, 48), (8, 8, 16, 12)).unwrap().unwrap();
        assert_eq!(size_of(&cropped), (16, 12));
    }

    #[test]
    fn a_jpeg_over_the_ceiling_is_cropped_from_a_reduced_decode() {
        // 64 x 48 x 3 = 9216 bytes decoded; 4000 admits the half scale only.
        let cropped = crop_within(&jpeg(64, 48), (8, 8, 16, 12), 4_000)
            .unwrap()
            .unwrap();
        assert_eq!(size_of(&cropped), (8, 6));
    }

    #[test]
    fn any_other_image_over_the_ceiling_is_left_to_its_thumbnail() {
        let png = png_with_alpha(64, 48);
        assert_eq!(
            crop_within(&png, (8, 8, 16, 12), 4_000).unwrap(),
            Err(TooLargeToCrop)
        );
        // The thumbnail of a 64-pixel-wide original, at half its size.
        let thumbnail = generate(&jpeg(32, 24)).unwrap().bytes;
        let cropped = crop_scaled(&thumbnail, (8, 8, 16, 12), 64).unwrap();
        assert_eq!(size_of(&cropped), (8, 6));
    }

    #[tokio::test]
    async fn an_off_thread_crop_spans_its_decode_and_encode_under_the_callers_span() {
        use tracing_subscriber::layer::SubscriberExt as _;

        let spans = Spans::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(spans.clone()));

        let cropped = crop_off_thread(jpeg(64, 48), (8, 8, 16, 16))
            .await
            .expect("crop")
            .expect("small enough to crop whole");

        assert!(!cropped.is_empty());
        let spans = spans.0.lock().expect("capture lock").clone();
        for (name, parent) in [
            ("media.decode", "media.crop"),
            ("media.encode", "media.crop"),
        ] {
            assert!(
                spans
                    .iter()
                    .any(|(span, of)| span == name && of.as_deref() == Some(parent)),
                "{name} under {parent}: {spans:?}"
            );
        }
    }

    #[test]
    fn a_crop_is_sent_at_thumbnail_size_and_a_small_one_as_it_is() {
        let large = crop(&jpeg(1200, 900), (100, 50, 1000, 800))
            .expect("crop")
            .expect("small enough to crop whole");
        assert_eq!(
            dimensions(&large),
            Some((THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE * 800 / 1000))
        );
        let small = crop(&jpeg(64, 48), (8, 8, 16, 12))
            .expect("crop")
            .expect("small enough to crop whole");
        assert_eq!(dimensions(&small), Some((16, 12)));
    }
}
