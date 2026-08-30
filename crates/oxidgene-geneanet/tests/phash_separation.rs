//! Measures, on a real data archive, whether the perceptual hash can be
//! trusted to recognise a rendition without ever recognising the wrong one.
//!
//! This is the measurement behind [`oxidgene_geneanet::phash`]'s thresholds,
//! kept runnable rather than written down and trusted. It is `#[ignore]`d and
//! self-skipping: a Geneanet data archive is hundreds of megabytes of someone's
//! family photographs, so it is never committed. Point it at your own:
//!
//! ```text
//! OXIDGENE_GENEANET_ARCHIVES=/path/a.zip:/path/b.zip \
//!   cargo test --release -p oxidgene-geneanet --features phash-validation \
//!     --test phash_separation a_rendition_is_never_matched_to_the_wrong_original \
//!     -- --ignored --nocapture
//! ```
//!
//! To compare the full-resolution and reduced-IDCT JPEG implementations on
//! exactly the same generated renditions:
//!
//! ```text
//! OXIDGENE_GENEANET_ARCHIVES=/path/a.zip:/path/b.zip \
//!   cargo test --release -p oxidgene-geneanet --features phash-validation \
//!     --test phash_separation compare_full_and_reduced_decode \
//!     -- --ignored --nocapture
//! ```
//!
//! # The property under test
//!
//! Not "no two entries hash alike" — some do, and that is expected. A real
//! archive holds the same upload twice under a `(1)` suffix, and it holds
//! pages of the same administrative dossier that differ only in the text on
//! them. Measured on the reference archive, the closest pair of *different*
//! pictures sits at distance 2, well inside the acceptance radius.
//!
//! What the matcher promises is narrower and is what is checked here: given a
//! rendition, it either finds the entry it really came from or declines. It
//! never returns a different one. Pairs too close to separate become
//! [`Match::Ambiguous`], get settled by comparing bytes, and — when the bytes
//! differ — fall through to being downloaded. A miss costs a download; a wrong
//! match would put a stranger's photograph on someone's ancestor, so the test
//! fails only on the second.
//!
//! This test deliberately sends every decodable archive entry through pHash.
//! It cannot model the number of fallback downloads in an import because a
//! data archive does not carry Geneanet deposit or page metadata. In
//! production, single-page deposits are resolved by exact size and only
//! unresolved multi-page views reach this matcher.

use std::io::Read;

use oxidgene_geneanet::phash::{self, MAX_DISTANCE, MIN_MARGIN, Match, Phash};

#[cfg(feature = "phash-validation")]
use oxidgene_geneanet::phash::hash_image_reduced_decode_for_validation;

#[derive(Debug, Default)]
struct Outcome {
    matched: usize,
    outside_radius: usize,
    ambiguous: usize,
    wrong: usize,
    skipped: usize,
}

impl Outcome {
    fn declined(&self) -> usize {
        self.outside_radius + self.ambiguous
    }

    fn print(&self, label: &str) {
        let total = self.matched + self.declined() + self.wrong;
        println!(
            "{label}: {} matched, {} outside radius, {} ambiguous, {} wrong, {} unrenderable; resolved {:.1}%",
            self.matched,
            self.outside_radius,
            self.ambiguous,
            self.wrong,
            self.skipped,
            100.0 * self.matched as f64 / total.max(1) as f64,
        );
    }
}

/// Reads every non-directory entry of every archive named in the environment.
fn load() -> Vec<(String, Vec<u8>)> {
    let Ok(list) = std::env::var("OXIDGENE_GENEANET_ARCHIVES") else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for (archive_index, path) in list.split(':').filter(|p| !p.is_empty()).enumerate() {
        let Ok(file) = std::fs::File::open(path) else {
            eprintln!("skipping archive {archive_index}: cannot open");
            continue;
        };
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            eprintln!("skipping archive {archive_index}: not a ZIP");
            continue;
        };
        for index in 0..zip.len() {
            let Ok(mut entry) = zip.by_index(index) else {
                continue;
            };
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();
            let mut bytes = Vec::new();
            if entry.read_to_end(&mut bytes).is_ok() {
                entries.push((name, bytes));
            }
        }
    }
    entries
}

/// Stands in for a Geneanet rendition: a downscale and a re-encode.
///
/// The import fetches `medium`, so that is what this simulates by default —
/// override with `OXIDGENE_RENDITION_WIDTH` to check another rung of the
/// ladder. It *is* a proxy, and the thresholds it validates should be
/// re-checked against real renditions once the wizard has run against a live
/// account.
fn rendition(bytes: &[u8]) -> Option<Vec<u8>> {
    // Overridable, because which rendition the import fetches is a real
    // choice: `medium` moves far fewer bytes than `normal` across every page
    // of every document, and the thresholds have to hold at whichever is used.
    let width: u32 = std::env::var("OXIDGENE_RENDITION_WIDTH")
        .ok()
        .and_then(|w| w.parse().ok())
        .unwrap_or(600);

    let image = image::load_from_memory(bytes).ok()?;
    let scaled = image.resize(width, width, image::imageops::FilterType::Lanczos3);

    let mut buffer = std::io::Cursor::new(Vec::new());
    scaled
        .to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut buffer,
            82,
        ))
        .ok()?;
    Some(buffer.into_inner())
}

fn evaluate(
    names: &[String],
    originals: &[Phash],
    renditions: &[Option<Phash>],
    bytes_of: &std::collections::HashMap<&str, &Vec<u8>>,
) -> Outcome {
    let mut outcome = Outcome::default();

    for (index, query) in renditions.iter().enumerate() {
        let Some(query) = query else {
            outcome.skipped += 1;
            continue;
        };

        match phash::find(*query, originals) {
            Match::Found(found) if found == index => outcome.matched += 1,
            Match::Found(found) => {
                let interchangeable = bytes_of
                    .get(names[index].as_str())
                    .zip(bytes_of.get(names[found].as_str()))
                    .is_some_and(|(a, b)| a == b);
                if interchangeable {
                    outcome.matched += 1;
                } else {
                    outcome.wrong += 1;
                }
            }
            Match::None => outcome.outside_radius += 1,
            Match::Ambiguous(tied) => {
                let reference = bytes_of.get(names[tied[0]].as_str());
                let all_same = tied
                    .iter()
                    .all(|other| bytes_of.get(names[*other].as_str()) == reference);
                if all_same && tied.contains(&index) {
                    outcome.matched += 1;
                } else {
                    outcome.ambiguous += 1;
                }
            }
        }
    }

    outcome
}

#[test]
#[ignore = "needs a real Geneanet data archive; see the module docs"]
fn a_rendition_is_never_matched_to_the_wrong_original() {
    let entries = load();
    if entries.is_empty() {
        eprintln!("OXIDGENE_GENEANET_ARCHIVES unset or empty — skipping");
        return;
    }

    let mut names = Vec::new();
    let mut originals: Vec<Phash> = Vec::new();
    let mut renditions: Vec<Option<Phash>> = Vec::new();
    let mut undecodable = 0usize;

    for (entry_index, (name, bytes)) in entries.iter().enumerate() {
        if entry_index > 0 && entry_index % 50 == 0 {
            eprintln!("hashed {entry_index}/{} archive entries", entries.len());
        }
        // PDFs and anything else `image` cannot read are absent from the
        // index; a page that would have matched one is downloaded instead.
        let Ok(original) = phash::hash_image(bytes) else {
            undecodable += 1;
            continue;
        };
        names.push(name.clone());
        originals.push(original);
        renditions.push(rendition(bytes).and_then(|r| phash::hash_image(&r).ok()));
    }

    println!(
        "{} entries, {} hashed, {undecodable} not decodable as images",
        entries.len(),
        originals.len()
    );
    assert!(originals.len() > 1, "need at least two entries to compare");

    let bytes_of: std::collections::HashMap<&str, &Vec<u8>> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes))
        .collect();

    let outcome = evaluate(&names, &originals, &renditions, &bytes_of);
    println!("radius {MAX_DISTANCE}, margin {MIN_MARGIN}");
    outcome.print("production");

    assert!(
        outcome.wrong == 0,
        "{} rendition(s) matched the wrong original",
        outcome.wrong
    );
}

#[cfg(feature = "phash-validation")]
#[test]
#[ignore = "needs a real Geneanet data archive; see the module docs"]
fn compare_full_and_reduced_decode() {
    let entries = load();
    if entries.is_empty() {
        eprintln!("OXIDGENE_GENEANET_ARCHIVES unset or empty — skipping");
        return;
    }

    let mut names = Vec::new();
    let mut full_originals = Vec::new();
    let mut reduced_originals = Vec::new();
    let mut full_renditions = Vec::new();
    let mut reduced_renditions = Vec::new();
    let mut undecodable = 0usize;
    let mut generated = 0usize;

    for (entry_index, (name, bytes)) in entries.iter().enumerate() {
        if entry_index > 0 && entry_index % 50 == 0 {
            eprintln!("compared {entry_index}/{} archive entries", entries.len());
        }

        let (Ok(full_original), Ok(reduced_original)) = (
            phash::hash_image(bytes),
            hash_image_reduced_decode_for_validation(bytes),
        ) else {
            undecodable += 1;
            continue;
        };

        let fake_rendition = rendition(bytes);
        generated += usize::from(fake_rendition.is_some());
        let full_rendition = fake_rendition
            .as_deref()
            .and_then(|rendition| phash::hash_image(rendition).ok());
        let reduced_rendition = fake_rendition
            .as_deref()
            .and_then(|rendition| hash_image_reduced_decode_for_validation(rendition).ok());

        names.push(name.clone());
        full_originals.push(full_original);
        reduced_originals.push(reduced_original);
        full_renditions.push(full_rendition);
        reduced_renditions.push(reduced_rendition);
    }

    assert!(
        full_originals.len() > 1,
        "need at least two entries to compare"
    );
    let bytes_of: std::collections::HashMap<&str, &Vec<u8>> = entries
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes))
        .collect();
    let full = evaluate(&names, &full_originals, &full_renditions, &bytes_of);
    let reduced = evaluate(&names, &reduced_originals, &reduced_renditions, &bytes_of);

    println!(
        "{} entries, {} compared, {undecodable} undecodable; {generated} fake renditions generated once each",
        entries.len(),
        names.len(),
    );
    println!("radius {MAX_DISTANCE}, margin {MIN_MARGIN}");
    full.print("full decode");
    reduced.print("reduced IDCT");
    println!(
        "delta reduced-full: {:+} matched, {:+} declined, {:+} wrong",
        reduced.matched as isize - full.matched as isize,
        reduced.declined() as isize - full.declined() as isize,
        reduced.wrong as isize - full.wrong as isize,
    );

    assert_eq!(full.wrong, 0, "full decode matched the wrong original");
    assert_eq!(reduced.wrong, 0, "reduced IDCT matched the wrong original");
}
