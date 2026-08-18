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
//!   cargo test --release -p oxidgene-geneanet --test phash_separation -- --ignored --nocapture
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

use std::io::Read;

use oxidgene_geneanet::phash::{self, MAX_DISTANCE, MIN_MARGIN, Match, Phash};

/// Reads every non-directory entry of every archive named in the environment.
fn load() -> Vec<(String, Vec<u8>)> {
    let Ok(list) = std::env::var("OXIDGENE_GENEANET_ARCHIVES") else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for path in list.split(':').filter(|p| !p.is_empty()) {
        let Ok(file) = std::fs::File::open(path) else {
            eprintln!("skipping {path}: cannot open");
            continue;
        };
        let Ok(mut zip) = zip::ZipArchive::new(file) else {
            eprintln!("skipping {path}: not a ZIP");
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

    for (name, bytes) in &entries {
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

    let (mut matched, mut declined, mut skipped) = (0usize, 0usize, 0usize);
    let mut wrong: Vec<String> = Vec::new();

    for (index, query) in renditions.iter().enumerate() {
        let Some(query) = query else {
            skipped += 1;
            continue;
        };

        match phash::find(*query, &originals) {
            Match::Found(found) if found == index => matched += 1,
            Match::Found(found) => {
                // The only outcome that is a defect. A duplicate upload is not
                // one: byte-identical entries are interchangeable by design,
                // and the matcher settles those by comparing bytes.
                let interchangeable = bytes_of
                    .get(names[index].as_str())
                    .zip(bytes_of.get(names[found].as_str()))
                    .is_some_and(|(a, b)| a == b);
                if interchangeable {
                    matched += 1;
                } else {
                    wrong.push(format!("{} matched to {}", names[index], names[found]));
                }
            }
            Match::None => declined += 1,
            Match::Ambiguous(tied) => {
                // What the caller then does is compare bytes; it only accepts
                // if every tied entry is identical. Mirror that here.
                let reference = bytes_of.get(names[tied[0]].as_str());
                let all_same = tied
                    .iter()
                    .all(|other| bytes_of.get(names[*other].as_str()) == reference);
                if all_same && tied.contains(&index) {
                    matched += 1;
                } else {
                    declined += 1;
                }
            }
        }
    }

    let total = matched + declined + wrong.len();
    println!(
        "radius {MAX_DISTANCE}, margin {MIN_MARGIN}: {matched} matched, {declined} declined \
         (downloaded instead), {} wrong, {skipped} unrenderable",
        wrong.len()
    );
    println!(
        "resolved without a download: {:.1}%",
        100.0 * matched as f64 / total.max(1) as f64
    );

    assert!(
        wrong.is_empty(),
        "a rendition was matched to the wrong original — this is the failure \
         that puts one person's photograph on another:\n  {}",
        wrong.join("\n  ")
    );
}
