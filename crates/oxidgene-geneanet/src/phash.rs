//! Perceptual hashing, used to recognise an original we already hold.
//!
//! The problem it solves is narrow. Geneanet states a deposit's byte length
//! only for single-page deposits, so those match against the data archive
//! exactly (see [`crate::archive`]). A *page* of a multi-page deposit has no
//! length to match on — its download is a ZIP assembled on the fly and
//! streamed without a `Content-Length` — and pulling the whole deposit to get
//! one page is measurably wasteful: on the reference account it is 244 pages
//! fetched to use 9.
//!
//! So a page is recognised by content instead: fetch its rendition (small),
//! hash it, and look for the archive entry that is the same picture. The
//! rendition is a downscale and re-encode of the original, which is exactly
//! what a perceptual hash is for.
//!
//! # Why 256 bits, and why a margin
//!
//! The population this runs against is the worst case for perceptual hashing:
//! roughly a third of a real account's media are pages of administrative
//! dossiers — same scanner, mostly white, a block of text. Measured on the
//! reference archive's 228 hashable multi-page entries:
//!
//! | | 64-bit | 256-bit |
//! |---|---|---|
//! | entries with an exact twin elsewhere in the pool | **12** | **0** |
//! | entries with a twin within distance 4 | 109 | 9 |
//!
//! A 64-bit hash — the usual default — cannot tell these pages apart at all.
//! At 256 bits (a 16×16 DCT block rather than 8×8) they separate: nearest
//! wrong neighbour is at distance 2 minimum, 27 median, while a simulated
//! rendition sits 2 from its own original (median; 6 at worst).
//!
//! That gap is what [`MAX_DISTANCE`] and [`MIN_MARGIN`] encode. Both
//! conditions must hold, and the margin is the one doing the safety work: it
//! is what turns "two candidates look equally good" into a *detected* clash
//! rather than a coin toss. This crate never attaches on a probable match —
//! an ambiguous page is downloaded instead. See
//! `docs/specifications/geneanet-media-import.md` §5.

#[cfg(any(test, feature = "phash-validation"))]
use std::io::Cursor;

#[cfg(any(test, feature = "phash-validation"))]
use anyhow::anyhow;
use anyhow::{Context, Result};
use image::GrayImage;
#[cfg(any(test, feature = "phash-validation"))]
use image::RgbImage;
#[cfg(any(test, feature = "phash-validation"))]
use jpeg_decoder::PixelFormat;

/// Side of the square the image is reduced to before the transform.
const SAMPLE: usize = 32;

/// Side of the low-frequency block kept from it. 16×16 = 256 bits.
const BLOCK: usize = 16;

/// Bytes in a hash.
const BYTES: usize = BLOCK * BLOCK / 8;

/// Largest box requested from the JPEG decoder's reduced IDCT.
#[cfg(any(test, feature = "phash-validation"))]
const JPEG_DECODE_TARGET: u16 = 128;

/// How far a rendition may sit from its own original and still be recognised.
///
/// Measured worst case is 6 over 228 simulated pairs; this is that, not a
/// guess with headroom. Raising it does not buy matches — it only widens the
/// window in which [`MIN_MARGIN`] has to do the discriminating.
pub const MAX_DISTANCE: u32 = 6;

/// How much closer the best candidate must be than the runner-up.
///
/// The measured floor for a *wrong* candidate is 2, so a margin alone would
/// not be safe; combined with [`MAX_DISTANCE`] it left 0 mismatches over the
/// reference pool while still resolving the large majority of it.
pub const MIN_MARGIN: u32 = 8;

/// A 256-bit perceptual hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Phash([u8; BYTES]);

impl Phash {
    /// Bits that differ between two hashes.
    #[must_use]
    pub fn distance(self, other: Self) -> u32 {
        self.0
            .iter()
            .zip(other.0.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum()
    }
}

/// Hashes already-decoded 8-bit greyscale samples of a `SAMPLE`×`SAMPLE` image.
///
/// Split out from [`hash_image`] so the transform can be tested without going
/// through a decoder.
fn hash_samples(samples: &[f32; SAMPLE * SAMPLE]) -> Phash {
    // Separable 2-D DCT-II: rows then columns, against a precomputed basis.
    // At 32×32 this is ~32k multiply-adds, far cheaper than the decode that
    // produced the samples.
    let basis = dct_basis();

    let mut rows = [0f32; SAMPLE * SAMPLE];
    for (u, row) in rows.as_chunks_mut::<SAMPLE>().0.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            *cell = (0..SAMPLE)
                .map(|y| samples[u * SAMPLE + y] * basis[y * SAMPLE + x])
                .sum();
        }
    }

    let mut coefficients = [0f32; BLOCK * BLOCK];
    for v in 0..BLOCK {
        for u in 0..BLOCK {
            coefficients[v * BLOCK + u] = (0..SAMPLE)
                .map(|y| rows[y * SAMPLE + u] * basis[y * SAMPLE + v])
                .sum();
        }
    }

    // The DC term carries overall brightness and would drag the median toward
    // it, so the threshold is taken over everything else — the standard trick,
    // and the reason a uniformly darker scan hashes like its lighter twin.
    let mut rest: Vec<f32> = coefficients[1..].to_vec();
    rest.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = rest[rest.len() / 2];

    let mut bits = [0u8; BYTES];
    for (index, coefficient) in coefficients.iter().enumerate() {
        if *coefficient > median {
            bits[index / 8] |= 0x80 >> (index % 8);
        }
    }

    Phash(bits)
}

/// The DCT-II basis for `SAMPLE` points, row-major `[y * SAMPLE + k]`.
fn dct_basis() -> &'static [f32; SAMPLE * SAMPLE] {
    use std::sync::OnceLock;
    static BASIS: OnceLock<[f32; SAMPLE * SAMPLE]> = OnceLock::new();

    BASIS.get_or_init(|| {
        let mut basis = [0f32; SAMPLE * SAMPLE];
        for y in 0..SAMPLE {
            for k in 0..SAMPLE {
                let n = SAMPLE as f32;
                basis[y * SAMPLE + k] =
                    (std::f32::consts::PI * (2.0 * y as f32 + 1.0) * k as f32 / (2.0 * n)).cos();
            }
        }
        basis
    })
}

/// Hashes an encoded image.
///
/// Both sides of every comparison must go through this function. Hashing one
/// side elsewhere — in the page that fetched it, say — would compare
/// coefficients produced by two different resamplers, and the distances would
/// mean nothing.
///
/// # Errors
///
/// Returns `Err` if the bytes are not an image this build can decode.
pub fn hash_image(bytes: &[u8]) -> Result<Phash> {
    hash_image_generic(bytes)
}

fn hash_image_generic(bytes: &[u8]) -> Result<Phash> {
    let image = image::load_from_memory(bytes).context("decoding an image to hash it")?;
    Ok(hash_luma(&image.to_luma8()))
}

#[cfg(feature = "phash-validation")]
#[doc(hidden)]
pub fn hash_image_reduced_decode_for_validation(bytes: &[u8]) -> Result<Phash> {
    if bytes.starts_with(&[0xff, 0xd8]) {
        if let Ok(image) = decode_jpeg_reduced(bytes) {
            return Ok(hash_luma(&image));
        }
    }

    hash_image_generic(bytes)
}

fn hash_luma(image: &GrayImage) -> Phash {
    let reduced = image::imageops::resize(
        image,
        SAMPLE as u32,
        SAMPLE as u32,
        image::imageops::FilterType::Lanczos3,
    );

    let mut samples = [0f32; SAMPLE * SAMPLE];
    for (sample, pixel) in samples.iter_mut().zip(reduced.pixels()) {
        *sample = f32::from(pixel.0[0]);
    }

    hash_samples(&samples)
}

#[cfg(any(test, feature = "phash-validation"))]
fn decode_jpeg_reduced(bytes: &[u8]) -> Result<GrayImage> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().context("reading JPEG headers")?;
    let dimensions = decoder
        .scale(JPEG_DECODE_TARGET, JPEG_DECODE_TARGET)
        .context("selecting a reduced JPEG scale")?;
    let pixels = decoder.decode().context("decoding reduced JPEG")?;
    let info = decoder.info().context("JPEG decoder returned no info")?;

    match info.pixel_format {
        PixelFormat::L8 => {
            GrayImage::from_raw(u32::from(dimensions.0), u32::from(dimensions.1), pixels)
                .context("reduced JPEG luminance buffer has the wrong length")
        }
        PixelFormat::RGB24 => Ok(image::DynamicImage::ImageRgb8(
            RgbImage::from_raw(u32::from(dimensions.0), u32::from(dimensions.1), pixels)
                .context("reduced JPEG RGB buffer has the wrong length")?,
        )
        .to_luma8()),
        format => Err(anyhow!("unsupported JPEG pixel format {format:?}")),
    }
}

/// What a lookup concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    /// One candidate is close enough and far enough ahead of the next.
    Found(usize),
    /// Nothing was close enough.
    None,
    /// Several candidates are equally close, so the hash alone cannot choose.
    ///
    /// Carries every one of them, because the caller can often settle it and
    /// the hash cannot: the commonest cause by far is the *same file uploaded
    /// twice*, where the entries are byte-identical and either will do. Only
    /// when they genuinely differ is this a clash, and then the caller
    /// downloads rather than guessing — see [`crate::archive::PhashIndex`].
    Ambiguous(Vec<usize>),
}

/// Finds the one candidate that is the same picture as `query`.
///
/// Both thresholds must be met. Ambiguity is reported rather than resolved —
/// see the module docs for why that is the whole point.
#[must_use]
pub fn find(query: Phash, candidates: &[Phash]) -> Match {
    let mut best = u32::MAX;
    let mut distances = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let distance = query.distance(*candidate);
        best = best.min(distance);
        distances.push(distance);
    }

    if best > MAX_DISTANCE {
        return Match::None;
    }

    // Everything the margin cannot separate from the winner. One entry here
    // means a clear winner; several means the caller has to look at the bytes.
    let contenders: Vec<usize> = distances
        .iter()
        .enumerate()
        .filter(|(_, distance)| distance.saturating_sub(best) < MIN_MARGIN)
        .map(|(index, _)| index)
        .collect();

    match contenders.as_slice() {
        [only] => Match::Found(*only),
        _ => Match::Ambiguous(contenders),
    }
}

#[cfg(test)]
mod tests {
    use std::hint::black_box;
    use std::io::Read;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, anyhow};
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    use super::*;

    /// A deterministic pseudo-image, so tests do not need fixture files.
    fn gradient(seed: u32) -> [f32; SAMPLE * SAMPLE] {
        let mut samples = [0f32; SAMPLE * SAMPLE];
        for (index, sample) in samples.iter_mut().enumerate() {
            let x = (index % SAMPLE) as f32;
            let y = (index / SAMPLE) as f32;
            *sample = ((x * 7.0 + y * 13.0 + seed as f32 * 31.0) % 255.0).abs();
        }
        samples
    }

    #[test]
    fn a_hash_is_256_bits() {
        assert_eq!(BYTES, 32);
        let hash = hash_samples(&gradient(1));
        assert_eq!(hash.0.len(), 32);
    }

    #[test]
    fn a_hash_matches_itself_and_differs_from_another() {
        let one = hash_samples(&gradient(1));
        let two = hash_samples(&gradient(2));

        assert_eq!(one.distance(one), 0);
        assert!(one.distance(two) > 0, "different pictures must differ");
    }

    #[test]
    fn brightness_alone_does_not_change_the_hash() {
        // The DC term is excluded from the median for exactly this reason: a
        // rendition is often a shade lighter than its original, and that must
        // not read as a different picture.
        let base = gradient(3);
        let mut brighter = base;
        for sample in &mut brighter {
            *sample = (*sample + 20.0).min(255.0);
        }

        let distance = hash_samples(&base).distance(hash_samples(&brighter));
        assert!(distance <= MAX_DISTANCE, "brightness shifted by {distance}");
    }

    #[test]
    fn a_clear_winner_is_found() {
        let query = hash_samples(&gradient(1));
        let candidates = vec![
            hash_samples(&gradient(9)),
            hash_samples(&gradient(1)),
            hash_samples(&gradient(7)),
        ];

        assert_eq!(find(query, &candidates), Match::Found(1));
    }

    #[test]
    fn nothing_close_enough_is_not_a_match() {
        let query = hash_samples(&gradient(1));
        let candidates = vec![hash_samples(&gradient(9)), hash_samples(&gradient(7))];

        assert_eq!(find(query, &candidates), Match::None);
    }

    #[test]
    fn candidates_that_are_too_alike_are_all_reported_not_chosen_between() {
        // The case the whole margin rule exists for. Both are handed back
        // because the caller can often settle it — on a real archive this is
        // overwhelmingly the same file uploaded twice — but the hash itself
        // never picks.
        let query = hash_samples(&gradient(1));
        let candidates = vec![hash_samples(&gradient(1)), hash_samples(&gradient(1))];

        assert_eq!(find(query, &candidates), Match::Ambiguous(vec![0, 1]));
    }

    #[test]
    fn an_empty_pool_matches_nothing() {
        assert_eq!(find(hash_samples(&gradient(1)), &[]), Match::None);
    }

    #[test]
    fn the_thresholds_are_the_measured_ones() {
        // Pinned so a later tweak is a deliberate act with a measurement
        // behind it, not a drift. Both come from 228 simulated
        // rendition/original pairs on the reference archive: worst within-pair
        // distance 6, and a margin of 8 left zero mismatches.
        assert_eq!(MAX_DISTANCE, 6);
        assert_eq!(MIN_MARGIN, 8);
    }

    #[test]
    fn a_decoded_image_round_trips_through_the_hasher() {
        let mut buffer = std::io::Cursor::new(Vec::new());
        let image = image::RgbImage::from_fn(64, 48, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 5) as u8, ((x + y) * 2) as u8])
        });
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut buffer, image::ImageFormat::Png)
            .expect("encodes");
        let png = buffer.into_inner();

        let hash = hash_image(&png).expect("hashes");
        assert_eq!(hash.distance(hash_image(&png).expect("hashes again")), 0);
    }

    fn encoded_gradient(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                (x % 251) as u8,
                (y % 241) as u8,
                ((x * 3 + y * 5) % 239) as u8,
            ])
        });
        let mut buffer = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut buffer, format)
            .expect("encodes test image");
        buffer.into_inner()
    }

    #[test]
    fn a_large_jpeg_can_be_hashed_from_a_reduced_decode() {
        let jpeg = encoded_gradient(1600, 1200, image::ImageFormat::Jpeg);
        let reduced = decode_jpeg_reduced(&jpeg).expect("decodes with reduced IDCT");

        assert!(reduced.width() < 1600);
        assert!(reduced.height() < 1200);
        assert_eq!(hash_luma(&reduced), hash_luma(&reduced));
    }

    #[test]
    fn non_jpeg_codecs_keep_using_the_generic_hasher() {
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::WebP,
            image::ImageFormat::Gif,
            image::ImageFormat::Bmp,
            image::ImageFormat::Tiff,
        ] {
            let bytes = encoded_gradient(64, 64, format);
            hash_image(&bytes).unwrap_or_else(|error| panic!("{format:?} must hash: {error}"));
        }
    }

    fn decode_jpeg_luma(bytes: &[u8]) -> Result<(GrayImage, (usize, usize))> {
        let options = DecoderOptions::default()
            .set_max_width(usize::MAX)
            .set_max_height(usize::MAX)
            .jpeg_set_out_colorspace(ColorSpace::Luma);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        decoder
            .decode_headers()
            .map_err(|error| anyhow!("reading JPEG headers: {error}"))?;
        let dimensions = decoder
            .dimensions()
            .context("JPEG decoder returned no dimensions")?;
        let pixels = decoder
            .decode()
            .map_err(|error| anyhow!("decoding JPEG luminance: {error}"))?;
        let image = GrayImage::from_raw(dimensions.0 as u32, dimensions.1 as u32, pixels)
            .context("JPEG luminance buffer has the wrong length")?;
        Ok((image, dimensions))
    }

    fn hash_jpeg_luma(bytes: &[u8]) -> Result<(Phash, (usize, usize), usize)> {
        let (image, dimensions) = decode_jpeg_luma(bytes)?;
        let decoded_bytes = image.as_raw().len();
        Ok((hash_luma(&image), dimensions, decoded_bytes))
    }

    fn hash_jpeg_reduced(bytes: &[u8]) -> Result<(Phash, (u32, u32), usize)> {
        let image = decode_jpeg_reduced(bytes)?;
        let dimensions = image.dimensions();
        let decoded_bytes = image.as_raw().len();
        Ok((hash_luma(&image), dimensions, decoded_bytes))
    }

    fn jpeg_sample() -> Result<Option<Vec<u8>>> {
        let Ok(path) = std::env::var("OXIDGENE_PHASH_JPEG") else {
            return Ok(None);
        };
        let bytes = std::fs::read(&path).with_context(|| format!("reading {path}"))?;
        if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return Ok(Some(bytes));
        }

        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("opening sample ZIP")?;
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .context("reading sample ZIP entry")?;
            if entry.is_dir() {
                continue;
            }
            let mut candidate = Vec::new();
            entry
                .read_to_end(&mut candidate)
                .context("extracting sample ZIP entry")?;
            if candidate.starts_with(&[0xff, 0xd8, 0xff]) {
                return Ok(Some(candidate));
            }
        }
        Err(anyhow!("sample contains no JPEG image"))
    }

    fn timed<T>(iterations: usize, mut operation: impl FnMut() -> T) -> (Duration, T) {
        let mut result = black_box(operation());
        let started = Instant::now();
        for _ in 0..iterations {
            result = black_box(operation());
        }
        (started.elapsed() / iterations as u32, result)
    }

    #[test]
    #[ignore = "needs OXIDGENE_PHASH_JPEG pointing to a JPEG or ZIP"]
    fn compare_jpeg_phash_decode_strategies() -> Result<()> {
        let Some(bytes) = jpeg_sample()? else {
            eprintln!("OXIDGENE_PHASH_JPEG unset; skipping");
            return Ok(());
        };
        let iterations = 5;

        let (baseline_time, baseline) =
            timed(iterations, || hash_image_generic(&bytes).expect("baseline"));
        let (luma_time, luma) = timed(iterations, || hash_jpeg_luma(&bytes).expect("luminance"));
        let (luma_decode_time, luma_image) = timed(iterations, || {
            decode_jpeg_luma(&bytes).expect("luminance decode")
        });
        let (luma_hash_time, _) = timed(iterations, || hash_luma(&luma_image.0));
        let (reduced_time, reduced) = timed(iterations, || {
            hash_jpeg_reduced(&bytes).expect("reduced IDCT")
        });

        println!(
            "JPEG bytes={}, iterations={iterations}\n\
             baseline={baseline_time:?}\n\
             luminance={luma_time:?} (decode={luma_decode_time:?}, resize+hash={luma_hash_time:?}), \
             dimensions={:?}, decoded_bytes={}, distance={}\n\
             reduced={reduced_time:?}, dimensions={:?}, decoded_bytes={}, distance={}",
            bytes.len(),
            luma.1,
            luma.2,
            baseline.distance(luma.0),
            reduced.1,
            reduced.2,
            baseline.distance(reduced.0),
        );
        Ok(())
    }

    #[test]
    fn bytes_that_are_not_an_image_fail_rather_than_hashing_noise() {
        assert!(hash_image(b"this is not an image").is_err());
    }
}
