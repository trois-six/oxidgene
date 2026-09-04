//! Measures **where** a perceptual hash spends its time, on real archives.
//!
//! [`phash_separation`](./phash_separation.rs) answers whether the matcher is
//! correct. This answers why it is slow, which is a different question and was
//! being guessed at: an import of the reference account spent 3m33s hashing 244
//! entries, and "a JPEG decode is expensive" does not survive the arithmetic —
//! those entries average about a megabyte each.
//!
//! Like its neighbour it is `#[ignore]`d and self-skipping, because a Geneanet
//! data archive is hundreds of megabytes of someone's family photographs and is
//! never committed. Point it at your own:
//!
//! ```text
//! OXIDGENE_GENEANET_ARCHIVES=/path/a.zip:/path/b.zip \
//!   cargo test --release -p oxidgene-geneanet \
//!     --test phash_cost where_a_perceptual_hash_spends_its_time \
//!     -- --ignored --nocapture
//! ```
//!
//! Release mode is not optional. The image decoders and the resampler are both
//! an order of magnitude slower without optimizations, and in different
//! proportions, so a debug run would misattribute the cost it is here to
//! attribute.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How many entries to profile. Enough to average over, short enough to rerun.
const SAMPLE_ENTRIES: usize = 24;

/// The side of the hash grid, mirroring `phash::SAMPLE`.
const GRID: u32 = 32;

#[derive(Default)]
struct Phase {
    total: Duration,
    count: usize,
}

impl Phase {
    fn add(&mut self, elapsed: Duration) {
        self.total += elapsed;
        self.count += 1;
    }

    fn mean_ms(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        self.total.as_secs_f64() * 1000.0 / self.count as f64
    }
}

fn archives() -> Vec<PathBuf> {
    std::env::var("OXIDGENE_GENEANET_ARCHIVES")
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(':')
                .filter(|part| !part.is_empty())
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Reads up to `SAMPLE_ENTRIES` decodable entries, largest last so the sample
/// is not accidentally all thumbnails.
fn sample_entries(paths: &[PathBuf]) -> Vec<Vec<u8>> {
    let mut bytes = Vec::new();

    for path in paths {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            continue;
        };
        for index in 0..zip.len() {
            if bytes.len() >= SAMPLE_ENTRIES {
                return bytes;
            }
            let Ok(mut entry) = zip.by_index(index) else {
                continue;
            };
            if entry.is_dir() || entry.size() < 128 * 1024 {
                continue;
            }
            let mut buffer = Vec::new();
            if entry.read_to_end(&mut buffer).is_err() {
                continue;
            }
            if image::guess_format(&buffer).is_ok() {
                bytes.push(buffer);
            }
        }
    }

    bytes
}

#[test]
#[ignore = "needs OXIDGENE_GENEANET_ARCHIVES pointing at real data archives"]
fn where_a_perceptual_hash_spends_its_time() {
    let paths = archives();
    if paths.is_empty() {
        eprintln!("skipped: set OXIDGENE_GENEANET_ARCHIVES to profile");
        return;
    }

    let entries = sample_entries(&paths);
    assert!(
        !entries.is_empty(),
        "no decodable entry over 128 KiB found in the given archives"
    );

    let mut header = Phase::default();
    let mut decode = Phase::default();
    let mut luma = Phase::default();
    let mut resize_full = Phase::default();
    let mut resize_two_step = Phase::default();
    let mut megapixels = 0f64;

    for bytes in &entries {
        // 1. Header only — what the aspect-ratio filter pays.
        let started = Instant::now();
        let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_decoder().ok())
            .map(|decoder| image::ImageDecoder::dimensions(&decoder));
        header.add(started.elapsed());
        let Some((width, height)) = dimensions else {
            continue;
        };
        megapixels += f64::from(width) * f64::from(height) / 1_000_000.0;

        // 2. Full-resolution decode.
        let started = Instant::now();
        let Ok(image) = image::load_from_memory(bytes) else {
            continue;
        };
        decode.add(started.elapsed());

        // 3. Greyscale conversion at full resolution.
        let started = Instant::now();
        let grey = image.to_luma8();
        luma.add(started.elapsed());

        // 4. The downscale the hash actually performs: full resolution
        //    straight to the grid, with Lanczos3. The filter's support grows
        //    with the scale factor, so this is the phase most likely to be
        //    quadratic in the source size rather than linear.
        let started = Instant::now();
        let reduced =
            image::imageops::resize(&grey, GRID, GRID, image::imageops::FilterType::Lanczos3);
        resize_full.add(started.elapsed());
        std::hint::black_box(&reduced);

        // 5. The same downscale in two steps: a cheap box prefilter to a few
        //    multiples of the grid, then Lanczos3 over that. Standard practice
        //    for large reductions, and the candidate replacement.
        let started = Instant::now();
        let prefiltered = image::imageops::resize(
            &grey,
            GRID * 8,
            GRID * 8,
            image::imageops::FilterType::Triangle,
        );
        let two_step = image::imageops::resize(
            &prefiltered,
            GRID,
            GRID,
            image::imageops::FilterType::Lanczos3,
        );
        resize_two_step.add(started.elapsed());
        std::hint::black_box(&two_step);
    }

    let profiled = decode.count;
    assert!(profiled > 0, "no entry could be decoded");

    println!(
        "\nprofiled {profiled} entries, mean {:.1} Mpx",
        megapixels / profiled as f64
    );
    println!("  {:>28}  {:>9}", "phase", "mean ms");
    println!(
        "  {:>28}  {:>9.1}",
        "header only (dimensions)",
        header.mean_ms()
    );
    println!("  {:>28}  {:>9.1}", "full decode", decode.mean_ms());
    println!("  {:>28}  {:>9.1}", "to_luma8", luma.mean_ms());
    println!(
        "  {:>28}  {:>9.1}",
        "resize full -> 32 (Lanczos3)",
        resize_full.mean_ms()
    );
    println!(
        "  {:>28}  {:>9.1}",
        "resize two-step -> 32",
        resize_two_step.mean_ms()
    );
    println!(
        "  {:>28}  {:>9.1}",
        "hash total (2+3+4)",
        decode.mean_ms() + luma.mean_ms() + resize_full.mean_ms()
    );
    println!(
        "  {:>28}  {:>9.1}\n",
        "hash total, two-step resize",
        decode.mean_ms() + luma.mean_ms() + resize_two_step.mean_ms()
    );
}
