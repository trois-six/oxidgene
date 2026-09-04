//! Replays the content matcher over a **real saved session and real archives**.
//!
//! [`phash_separation`](./phash_separation.rs) measures the matcher against
//! renditions it generates itself, and says so: it "cannot model the number of
//! fallback downloads in an import because a data archive does not carry
//! Geneanet deposit or page metadata". A session archive does carry it — the
//! collection, the deposit sizes, and the renditions the login window actually
//! fetched — so this replays the production pipeline end to end:
//!
//! 1. single-page deposits claim archive entries by exact byte length;
//! 2. the pages of multi-page deposits collect the target dimensions;
//! 3. the perceptual index is built over what is left;
//! 4. every page is looked up the way the import looks it up.
//!
//! What it is for is changing the hashing without guessing. A candidate change
//! — a cheaper downscale, a reduced decode, a different `JPEG_DECODE_TARGET` —
//! is judged by whether the pairing survives it rather than by whether it feels
//! faster.
//!
//! Record a reference with `OXIDGENE_GENEANET_PAIRING_OUT`, then run the
//! variant with `OXIDGENE_GENEANET_PAIRING_REF` pointing at it. The comparison
//! separates the two ways a variant can differ, and they are not equivalent: a
//! page the reference resolved and the variant declined costs one download,
//! while a page both resolved *to different entries* means one of them attached
//! the wrong picture. The first is a number to weigh, the second fails the run.
//!
//! It is `#[ignore]`d and self-skipping. A session archive is hundreds of
//! megabytes of someone's family photographs and their account's structure, so
//! it is never committed. Point it at your own:
//!
//! ```text
//! OXIDGENE_GENEANET_SESSION=/path/geneanet-session.zip \
//! OXIDGENE_GENEANET_ARCHIVES=/path/a.zip:/path/b.zip \
//! OXIDGENE_GENEANET_PAIRING_OUT=/tmp/pairing-reference.tsv \
//!   cargo test --release -p oxidgene-geneanet \
//!     --test phash_real_session the_matcher_resolves_a_real_session \
//!     -- --ignored --nocapture
//! ```
//!
//! One account is one account. A tree with no multi-page deposit exercises
//! nothing here, and one whose pages are administrative scans is the hard case
//! this cannot claim to represent. Treat a green run as "this change did not
//! regress *this* account", never as a general guarantee.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use base64::Engine as _;
use oxidgene_geneanet::archive::{ArchiveSet, PhashIndex, image_dimensions};
use oxidgene_geneanet::model::{ManifestDeposit, ManifestView};
use oxidgene_geneanet::phash;

/// Rendition preference for the perceptual sample.
///
/// Mirrors the import's own order (`rendition_url` in
/// `oxidgene-api/src/service/geneanet.rs`): the smallest rendition that is
/// still a faithful reduction, because it is what gets hashed.
const SAMPLE_ORDER: [&str; 4] = ["medium", "normal", "screen", "thumbnail"];

/// Where a rendition path is served from when it is not absolute.
const RENDITION_HOST: &str = "https://gw.geneanet.org";

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn env_paths(key: &str) -> Vec<PathBuf> {
    std::env::var(key)
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

fn rendition_url(view: &ManifestView) -> Option<String> {
    for rendition in SAMPLE_ORDER {
        if let Some(path) = view.files.get(rendition) {
            return Some(if path.starts_with("http") {
                path.clone()
            } else {
                format!("{RENDITION_HOST}{path}")
            });
        }
    }
    None
}

/// A page of a multi-page deposit, with the bytes the window fetched for it.
struct Page {
    deposit_id: i64,
    page: Option<i64>,
    rendition: Vec<u8>,
}

/// Reads a pairing written by an earlier run, if one was named.
///
/// The file holds deposit and page numbers against archive positions — no
/// title, no filename, nothing about a person — and is written only where the
/// caller asks for it.
fn read_pairing(path: &PathBuf) -> BTreeMap<(i64, Option<i64>), usize> {
    let mut pairs = BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return pairs;
    };
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [deposit, page, position] = fields.as_slice() else {
            continue;
        };
        let (Ok(deposit), Ok(position)) = (deposit.parse::<i64>(), position.parse::<usize>())
        else {
            continue;
        };
        pairs.insert((deposit, page.parse::<i64>().ok()), position);
    }
    pairs
}

fn write_pairing(path: &PathBuf, pairs: &BTreeMap<(i64, Option<i64>), usize>) {
    let mut text = String::new();
    for ((deposit, page), position) in pairs {
        let page = page.map_or_else(|| "-".to_string(), |p| p.to_string());
        text.push_str(&format!("{deposit}\t{page}\t{position}\n"));
    }
    if let Err(error) = std::fs::write(path, text) {
        eprintln!("could not write the pairing to {}: {error}", path.display());
    }
}

/// Compares a pairing against a reference, telling apart the two ways they can
/// differ.
///
/// The distinction is the whole reason this exists. A page the reference
/// resolved and this run declined costs a download — a real cost, but a safe
/// one. A page both resolved *to different entries* means one of them attached
/// the wrong picture, which is the failure the matcher exists to prevent, and
/// no amount of speed pays for it.
fn compare(
    reference: &BTreeMap<(i64, Option<i64>), usize>,
    current: &BTreeMap<(i64, Option<i64>), usize>,
) {
    let mut agreed = 0usize;
    let mut disagreed = 0usize;
    let mut lost = 0usize;
    for (key, position) in reference {
        match current.get(key) {
            Some(found) if found == position => agreed += 1,
            Some(_) => disagreed += 1,
            None => lost += 1,
        }
    }
    let gained = current
        .keys()
        .filter(|key| !reference.contains_key(*key))
        .count();

    println!("against the reference pairing:");
    println!("  {agreed:>4} identical");
    println!("  {lost:>4} the reference resolved and this run declined (a download each)");
    println!("  {gained:>4} this run resolved and the reference declined");
    println!("  {disagreed:>4} resolved to a DIFFERENT entry\n");

    assert_eq!(
        disagreed, 0,
        "a variant that resolves a page to a different entry than the reference \
         is not a cheaper matcher, it is a different answer"
    );
}

/// Cheap order-independent digest of a pairing, so two runs can be compared
/// without printing anyone's deposit structure in full.
fn digest(pairs: &BTreeMap<(i64, Option<i64>), usize>) -> u64 {
    let mut accumulator = 0xcbf2_9ce4_8422_2325u64;
    for ((deposit, page), position) in pairs {
        for value in [
            u64::from_ne_bytes(deposit.to_ne_bytes()),
            page.map_or(u64::MAX, |p| u64::from_ne_bytes(p.to_ne_bytes())),
            *position as u64,
        ] {
            accumulator ^= value;
            accumulator = accumulator.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    accumulator
}

#[test]
#[ignore = "needs OXIDGENE_GENEANET_SESSION and OXIDGENE_GENEANET_ARCHIVES"]
fn the_matcher_resolves_a_real_session() {
    let Some(session_path) = env_path("OXIDGENE_GENEANET_SESSION") else {
        eprintln!("skipped: set OXIDGENE_GENEANET_SESSION to replay a session");
        return;
    };
    let archive_paths = env_paths("OXIDGENE_GENEANET_ARCHIVES");
    if archive_paths.is_empty() {
        eprintln!("skipped: set OXIDGENE_GENEANET_ARCHIVES to replay a session");
        return;
    }

    let session = oxidgene_geneanet::session::decode(
        &std::fs::read(&session_path).expect("reads the session archive"),
    )
    .expect("decodes the session archive");
    let manifest = oxidgene_geneanet::manifest_from_collection(&session.collection)
        .expect("parses the collection the session carries");

    let mut set = ArchiveSet::new();
    for path in &archive_paths {
        set.add(path).expect("indexes an archive");
    }
    println!(
        "\nsession: {} deposits, {} sizes, {} fetched renditions",
        manifest.deposits.len(),
        session.deposit_sizes.len(),
        session.media.len()
    );
    println!("archives: {} entries indexed", set.entry_count());

    // ── 1. Single-page deposits claim entries by exact length ────────
    let mut claimed: BTreeSet<usize> = BTreeSet::new();
    let mut claimed_by_size = 0usize;
    let mut size_ambiguous = 0usize;
    let single: Vec<&ManifestDeposit> = manifest
        .deposits
        .iter()
        .filter(|d| d.views.len() <= 1)
        .collect();
    for deposit in &single {
        let Some(size) = session.deposit_sizes.get(&deposit.id) else {
            continue;
        };
        match set.locate_by_size(*size) {
            Ok(Some(position)) => {
                claimed.insert(position);
                claimed_by_size += 1;
            }
            Ok(None) => size_ambiguous += 1,
            Err(_) => size_ambiguous += 1,
        }
    }
    println!(
        "single-page deposits: {} total, {claimed_by_size} claimed by exact size, \
         {size_ambiguous} unresolved",
        single.len()
    );

    // ── 2. The pages that need content matching ──────────────────────
    let mut pages = Vec::new();
    let mut missing_rendition = 0usize;
    for deposit in manifest.deposits.iter().filter(|d| d.views.len() > 1) {
        for view in &deposit.views {
            let Some(bytes) = rendition_url(view)
                .and_then(|url| session.media.get(&url).cloned())
                .and_then(|encoded| {
                    base64::engine::general_purpose::STANDARD
                        .decode(encoded.as_bytes())
                        .ok()
                })
            else {
                missing_rendition += 1;
                continue;
            };
            pages.push(Page {
                deposit_id: deposit.id,
                page: view.page,
                rendition: bytes,
            });
        }
    }
    assert!(
        !pages.is_empty(),
        "this session has no multi-page page with a fetched rendition, so it \
         exercises nothing the perceptual matcher does"
    );
    println!(
        "multi-page pages: {} with a rendition, {missing_rendition} without",
        pages.len()
    );

    // ── 3. Targets and the index, exactly as the import builds them ──
    let targets: Vec<(u32, u32)> = pages
        .iter()
        .filter_map(|page| image_dimensions(&page.rendition))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let undecodable_renditions = pages.len()
        - pages
            .iter()
            .filter(|page| image_dimensions(&page.rendition).is_some())
            .count();
    println!(
        "targets: {} distinct shapes, {undecodable_renditions} renditions undecodable",
        targets.len()
    );

    let candidates: Vec<usize> = (0..set.entry_count())
        .filter(|position| !claimed.contains(position))
        .collect();

    let started = Instant::now();
    let index = PhashIndex::build_from_matching_dimensions(&set, &candidates, &targets);
    let build = started.elapsed();
    println!(
        "index: {} candidates, {} filtered on ratio, {} hashed, {} undecodable, built in {:.1}s",
        candidates.len(),
        index.filtered_count(),
        index.hashed_count(),
        index.undecodable_count(),
        build.as_secs_f64()
    );

    // ── 4. Resolve every page the way the import resolves it ─────────
    let started = Instant::now();
    let mut pairs: BTreeMap<(i64, Option<i64>), usize> = BTreeMap::new();
    let mut resolved = 0usize;
    let mut declined = 0usize;
    let mut unhashable = 0usize;
    let mut collisions: Vec<((i64, Option<i64>), usize)> = Vec::new();
    let mut claimed_by_page: HashMap<usize, (i64, Option<i64>)> = HashMap::new();

    for page in &pages {
        let key = (page.deposit_id, page.page);
        let Ok(query) = phash::hash_image(&page.rendition) else {
            unhashable += 1;
            continue;
        };
        match index.locate(&set, query) {
            Ok(Some(position)) => {
                resolved += 1;
                if let Some(previous) = claimed_by_page.insert(position, key) {
                    collisions.push((previous, position));
                }
                pairs.insert(key, position);
            }
            Ok(None) => declined += 1,
            Err(_) => declined += 1,
        }
    }
    let resolve = started.elapsed();

    println!(
        "pages: {resolved} resolved, {declined} declined, {unhashable} unhashable \
         — in {:.1}s",
        resolve.as_secs_f64()
    );
    println!("pairing digest: {:016x}\n", digest(&pairs));

    if let Some(path) = env_path("OXIDGENE_GENEANET_PAIRING_REF") {
        let reference = read_pairing(&path);
        if reference.is_empty() {
            eprintln!("no reference pairing at {}", path.display());
        } else {
            compare(&reference, &pairs);
        }
    }
    if let Some(path) = env_path("OXIDGENE_GENEANET_PAIRING_OUT") {
        write_pairing(&path, &pairs);
        println!("pairing written to {}", path.display());
    }

    // A declined page is a download, which is a cost. Two pages resolving to
    // the same archive entry means at least one of them is wrong, which is the
    // failure the matcher exists to avoid.
    assert!(
        collisions.is_empty(),
        "{} pages resolved to an entry another page had already claimed",
        collisions.len()
    );
}
