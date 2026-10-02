//! The memory a Geneanet import holds, on a fictitious tree generated here:
//! ten thousand people, photographs with identification boxes, group photos
//! and multi-page documents whose pages are recognised in the archive by
//! their content.
//!
//! Opt-in, through `just import-memory` (see `docs/development.md`): the
//! fixture is a few hundred megabytes of generated pictures and the run is
//! timed, so it is ignored by the ordinary suite and run in release mode.
//!
//! The import runs the way the desktop runs it: the router with local file
//! access, a file-backed SQLite database, a media root on disk, and the
//! background worker's own loop step. The test reports the job's time and
//! peak resident memory phase by phase, and fails when the job's peak or what
//! the process keeps once the job is over exceeds its budget.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine as _;
use http_body_util::BodyExt;
use image::{Rgb, RgbImage};
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::util::SubscriberInitExt as _;

/// The most resident memory the process may reach while the job runs, in
/// MiB. Measured at 400 MiB on a 16-thread machine, where eight pictures are
/// decoded at once, at its peak while the archive's pages are hashed; a
/// runner with fewer threads decodes fewer at once. Holding every page of a
/// document before storing any, with twelve decodes at once, it peaked at
/// 920 to 1,060 MiB.
const PEAK_BUDGET_MIB: u64 = 500;

/// The most resident memory the process may keep once the job is over, in
/// MiB. Measured at 45 MiB: an idle backend and the database's page cache.
/// Before the allocator was tuned the process kept its whole peak.
const RETAINED_BUDGET_MIB: u64 = 60;

// ── The fictitious tree ─────────────────────────────────────────────────

/// Families of the `.gw`, each a couple and two children.
const FAMILIES: usize = 2_500;
/// Single-page deposits: photographs, matched in the archive by size.
const PHOTOS: usize = 160;
/// Every how many photographs one shows a group of three.
const GROUP_EVERY: usize = 8;
/// Multi-page deposits and their page counts: scanned documents, matched in
/// the archive by the content of a smaller rendition.
const DOCUMENTS: [usize; 2] = [48, 8];
const PHOTO_SIZE: (u32, u32) = (2400, 1600);
/// An A4 page scanned at 300 dpi.
const PAGE_SIZE: (u32, u32) = (2480, 3508);
/// The width of the `medium` and `normal` renditions of a page.
const MEDIUM_WIDTH: u32 = 424;
const NORMAL_WIDTH: u32 = 1200;

/// A deterministic generator (xorshift64*): the same fixture on every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}

/// A picture no other seed draws: a coarse grid of random tones, which the
/// perceptual hash tells apart, under fine grain, which gives the file the
/// weight of a real photograph or scan.
fn picture(seed: u64, (width, height): (u32, u32)) -> RgbImage {
    let mut rng = Rng::new(seed);
    let grid: Vec<[u8; 3]> = (0..64)
        .map(|_| [rng.byte(), rng.byte(), rng.byte()])
        .collect();
    RgbImage::from_fn(width, height, |x, y| {
        let cell = grid[(y * 8 / height * 8 + x * 8 / width) as usize];
        let grain = rng.byte() / 12;
        Rgb(cell.map(|tone| tone.saturating_add(grain)))
    })
}

fn jpeg(image: &RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(image)
        .expect("encodes the picture");
    out
}

/// `image` scaled to `width`, as Geneanet renders a page.
fn rendition(image: &RgbImage, width: u32) -> Vec<u8> {
    let height = (u64::from(image.height()) * u64::from(width) / u64::from(image.width())) as u32;
    let scaled =
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle);
    jpeg(&scaled, 85)
}

/// One medium of the fixture, as its files on disk.
struct Generated {
    /// The archive entry of the original.
    entry: String,
    original: PathBuf,
    /// The page's `medium` and `normal` renditions, for a document page.
    renditions: Option<(PathBuf, PathBuf)>,
}

/// What a generated medium is.
#[derive(Clone, Copy)]
enum Kind {
    Photo,
    Page,
}

/// Draws and writes medium `index` of `kind`, several at a time.
fn generate(dir: &Path, items: &[(Kind, usize)]) -> Vec<Generated> {
    let workers = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = items.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|slice| {
                scope.spawn(move || {
                    slice
                        .iter()
                        .map(|item| draw(dir, *item))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("draws the fixture"))
            .collect()
    })
}

fn draw(dir: &Path, (kind, index): (Kind, usize)) -> Generated {
    let (prefix, size, seed) = match kind {
        Kind::Photo => ("photo", PHOTO_SIZE, index as u64),
        Kind::Page => ("page", PAGE_SIZE, 1_000_000 + index as u64),
    };
    let image = picture(seed, size);
    let entry = format!("{prefix}_{index:04}.jpg");
    let original = dir.join(&entry);
    std::fs::write(&original, jpeg(&image, 92)).expect("writes the original");
    let renditions = matches!(kind, Kind::Page).then(|| {
        let medium = dir.join(format!("{prefix}_{index:04}_medium.jpg"));
        let normal = dir.join(format!("{prefix}_{index:04}_normal.jpg"));
        std::fs::write(&medium, rendition(&image, MEDIUM_WIDTH)).expect("writes a rendition");
        std::fs::write(&normal, rendition(&image, NORMAL_WIDTH)).expect("writes a rendition");
        (medium, normal)
    });
    Generated {
        entry,
        original,
        renditions,
    }
}

/// The `.gw`: `FAMILIES` couples with a son and a daughter each.
fn geneweb() -> String {
    let mut gw = String::from("encoding: utf-8\n\n");
    for family in 0..FAMILIES {
        gw.push_str(&format!(
            "fam BRANCH_{family} father_{family} 1850 + SPOUSE_{family} mother_{family} 1852\n\
             beg\n- h son_{family} 1880\n- f daughter_{family} 1882\nend\n\n"
        ));
    }
    gw
}

/// The Geneanet reference of a person of the `.gw`: (last name, first name,
/// key).
fn person(rng: &mut Rng) -> (String, String, String) {
    let family = rng.below(FAMILIES);
    let (last, first) = match rng.below(4) {
        0 => (format!("BRANCH_{family}"), format!("father_{family}")),
        1 => (format!("SPOUSE_{family}"), format!("mother_{family}")),
        2 => (format!("BRANCH_{family}"), format!("son_{family}")),
        _ => (format!("BRANCH_{family}"), format!("daughter_{family}")),
    };
    let key = format!(
        "{}|{}|",
        last.to_lowercase().replace('_', " "),
        first.to_lowercase().replace('_', " ")
    );
    (last, first, key)
}

/// A reference to a person, with a box round them when `face` is given.
fn reference(rng: &mut Rng, face: Option<usize>) -> Value {
    let (lastname, firstname, key) = person(rng);
    let mut reference = json!({
        "firstname": firstname,
        "lastname": lastname,
        "reference_extra_geneweb": { "ref": key },
    });
    if let Some(slot) = face {
        let left = 5.0 + 30.0 * slot as f64;
        reference["face"] =
            json!({ "position": { "x1": left, "y1": 20.0, "x2": left + 25.0, "y2": 60.0 } });
    }
    reference
}

/// The import request's inputs, everything on disk under `dir`.
struct Fixture {
    body: Value,
    /// Pictures the import should store: photographs and document pages.
    media: usize,
    /// Identification boxes on them.
    boxes: usize,
    bytes: u64,
}

fn fixture(dir: &Path) -> Fixture {
    let pages: usize = DOCUMENTS.iter().sum();
    let items: Vec<(Kind, usize)> = (0..PHOTOS)
        .map(|index| (Kind::Photo, index))
        .chain((0..pages).map(|index| (Kind::Page, index)))
        .collect();
    let generated = generate(dir, &items);

    // One archive, as Geneanet's export packs a user's originals.
    let archive_path = dir.join("media_images.zip");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut sizes = Vec::with_capacity(generated.len());
    for medium in &generated {
        let mut bytes = Vec::new();
        std::fs::File::open(&medium.original)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .expect("reads an original");
        archive.start_file(&medium.entry, stored).unwrap();
        archive.write_all(&bytes).unwrap();
        sizes.push(bytes.len() as u64);
        std::fs::remove_file(&medium.original).unwrap();
    }
    archive.finish().unwrap();

    let mut rng = Rng::new(7);
    let mut deposits = Vec::new();
    let mut references = Vec::new();
    let mut deposit_sizes = serde_json::Map::new();
    let mut boxes = 0;
    for (index, size) in sizes.iter().enumerate().take(PHOTOS) {
        let (deposit_id, view_id) = (10_000 + index, 20_000 + index);
        let url = format!("https://example.invalid/photo/{view_id}/normal.jpg");
        let deposit = json!({
            "id": deposit_id,
            "title": format!("Photograph {index}"),
            "type": "photos",
            "views": [{ "id": view_id, "page": 1, "files": { "normal": url } }],
        });
        deposit_sizes.insert(deposit_id.to_string(), json!(size));
        let people = if index % GROUP_EVERY == 0 { 3 } else { 1 };
        for slot in 0..people {
            let mut entry = reference(&mut rng, (index % 2 == 0).then_some(slot));
            boxes += usize::from(index % 2 == 0);
            entry["deposit"] = json!({ "id": deposit_id, "views": [{ "id": view_id }] });
            references.push(entry);
        }
        deposits.push(deposit);
    }

    let mut fetched = serde_json::Map::new();
    let mut view_references = serde_json::Map::new();
    let mut page_media = generated[PHOTOS..].iter();
    for (document, page_count) in DOCUMENTS.iter().enumerate() {
        let deposit_id = 30_000 + document;
        let mut views = Vec::new();
        for page in 0..*page_count {
            let view_id = 40_000 + document * 1_000 + page;
            let (medium, normal) = page_media
                .next()
                .and_then(|generated| generated.renditions.clone())
                .expect("every page has its renditions");
            let medium_url = format!("https://example.invalid/page/{view_id}/medium.jpg");
            let normal_url = format!("https://example.invalid/page/{view_id}/normal.jpg");
            fetched.insert(medium_url.clone(), json!(medium));
            fetched.insert(normal_url.clone(), json!(normal));
            views.push(json!({
                "id": view_id,
                "page": page + 1,
                "files": { "medium": medium_url, "normal": normal_url },
            }));
            if page % 4 == 0 {
                view_references.insert(
                    format!("{deposit_id}:{view_id}"),
                    json!([reference(&mut rng, None)]),
                );
            }
        }
        deposits.push(json!({
            "id": deposit_id,
            "title": format!("Document {document}"),
            "type": "documents",
            "views": views,
        }));
    }

    let collection = json!({
        "deposits": deposits,
        "references": references,
        "view_references": view_references,
    });
    let gw = geneweb();
    Fixture {
        body: json!({
            "gw_base64": base64::engine::general_purpose::STANDARD.encode(gw.as_bytes()),
            "file_name": "fixture.gw",
            "collection": collection.to_string(),
            "deposit_sizes": deposit_sizes,
            "archive_paths": [archive_path],
            "fetched": fetched,
            "media_fidelity": "originals",
        }),
        media: PHOTOS + pages,
        boxes,
        bytes: sizes.iter().sum(),
    }
}

// ── Measuring ───────────────────────────────────────────────────────────

/// A line of the report: aggregates only.
fn report(line: &str) {
    eprintln!("[import-memory] {line}");
}

/// A field of `/proc/self/status`, in MiB.
fn status_mib(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(field))
                .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        })
        .map_or(0, |kib: u64| kib / 1024)
}

/// The highest peak [`take_peak_mib`] has read.
static HIGHEST_PEAK: AtomicU64 = AtomicU64::new(0);

/// The peak resident set since the last call, then a new start: writing 5 to
/// `clear_refs` sets the peak back to the current resident set (Linux).
fn take_peak_mib() -> u64 {
    let peak = status_mib("VmHWM:");
    let _ = std::fs::write("/proc/self/clear_refs", "5");
    HIGHEST_PEAK.fetch_max(peak, Ordering::Relaxed);
    peak
}

/// The import's phases, by the spans they run under.
const PHASES: [&str; 8] = [
    "import.job",
    "import.parse",
    "import.persist",
    "import.identities",
    "import.media",
    "import.media.index",
    "import.media.prepare",
    "import.projections",
];

/// Reports each phase's time and peak memory as its span closes.
///
/// The peak starts over as each phase opens and closes, so a phase that
/// holds others reports what it held after the last of them: for
/// `import.media`, its documents; for `import.job`, the end of the job.
#[derive(Default)]
struct PhaseMemory {
    open: Mutex<HashMap<u64, (&'static str, Instant)>>,
}

impl<S: Subscriber> Layer<S> for PhaseMemory {
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, _: Context<'_, S>) {
        let name = attributes.metadata().name();
        if PHASES.contains(&name) {
            take_peak_mib();
            if let Ok(mut open) = self.open.lock() {
                open.insert(id.into_u64(), (name, Instant::now()));
            }
        }
    }

    fn on_close(&self, id: Id, _: Context<'_, S>) {
        let closed = self
            .open
            .lock()
            .ok()
            .and_then(|mut open| open.remove(&id.into_u64()));
        if let Some((name, opened)) = closed {
            let peak = take_peak_mib();
            report(&format!(
                "{name}: {:.1} s, peak RSS {peak} MiB",
                opened.elapsed().as_secs_f64()
            ));
        }
    }
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<&Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Deletes the work directory, whatever the outcome.
struct WorkDir(PathBuf);

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "generates a large fixture and measures memory: run by `just import-memory`"]
async fn a_geneanet_import_stays_within_its_memory_budget() {
    // As the desktop's `main` does first.
    oxidgene_api::memory::tune();
    tracing_subscriber::registry()
        .with(PhaseMemory::default())
        .init();
    let work = WorkDir(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("import-memory"));
    let _ = std::fs::remove_dir_all(&work.0);
    let inputs = work.0.join("inputs");
    std::fs::create_dir_all(&inputs).unwrap();

    let started = Instant::now();
    let fixture = fixture(&inputs);
    report(&format!(
        "fixture: {} persons, {} pictures, {} boxes, archive {} MiB, drawn in {:.1} s",
        FAMILIES * 4,
        fixture.media,
        fixture.boxes,
        fixture.bytes >> 20,
        started.elapsed().as_secs_f64()
    ));

    let url = format!("sqlite://{}?mode=rwc", work.0.join("oxidgene.db").display());
    let db = connect(&url).await.expect("opens the database");
    run_migrations(&db).await.expect("migrates the database");
    let state = AppState::new(db, work.0.join("media")).with_local_file_access();
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "import-memory",
    );
    let app = build_router(state);
    let (_, tree) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(&json!({ "name": "Fixture" })),
    )
    .await;
    let tree_id = tree["id"].as_str().expect("creates the tree").to_owned();

    // What the fixture's drawing left behind is not the import's.
    oxidgene_api::memory::release_free_memory();
    report(&format!("before the job: RSS {} MiB", status_mib("VmRSS:")));
    let (status, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/geneanet/import"),
        Some(&fixture.body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "error code {}",
        started["error"]
    );
    let job_id = started["job_id"].as_str().unwrap();

    // The worker's own loop step, which also hands the job's memory back.
    take_peak_mib();
    HIGHEST_PEAK.store(0, Ordering::Relaxed);
    let started = Instant::now();
    assert!(
        worker.run_once().await.expect("runs the job"),
        "no job was queued"
    );
    take_peak_mib();
    let peak = HIGHEST_PEAK.load(Ordering::Relaxed);
    let retained = status_mib("VmRSS:");
    report(&format!(
        "job {:.1} s, peak RSS {peak} MiB; after it, RSS {retained} MiB",
        started.elapsed().as_secs_f64()
    ));

    let status_uri = format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}");
    let (_, status) = send(&app, Method::GET, &status_uri, None).await;

    assert_eq!(
        status["phase"], "completed",
        "error code {}",
        status["error"]
    );
    let receipt = &status["geneanet_result"];
    assert_eq!(receipt["persons_count"], FAMILIES * 4);
    assert_eq!(
        receipt["images_count"], PHOTOS,
        "every photograph is stored"
    );
    assert_eq!(receipt["documents_count"], DOCUMENTS.len());
    assert_eq!(
        receipt["document_pages_count"],
        DOCUMENTS.iter().sum::<usize>(),
        "every page is stored"
    );
    assert_eq!(receipt["vignettes_count"], fixture.boxes);
    assert_eq!(receipt["skipped"], json!([]), "nothing is skipped");
    assert!(
        peak <= PEAK_BUDGET_MIB,
        "the job peaked at {peak} MiB, over its {PEAK_BUDGET_MIB} MiB budget"
    );
    assert!(
        retained <= RETAINED_BUDGET_MIB,
        "the process kept {retained} MiB after the job, over its {RETAINED_BUDGET_MIB} MiB budget"
    );
}
