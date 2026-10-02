//! The maintainer's own exports, imported end to end through the desktop's
//! backend: the router with local file access and the background worker,
//! over a file-backed SQLite database and a media root on disk.
//!
//! Opt-in, through `just real-import` (see `docs/development.md`): every test
//! here is ignored, and skips itself when the variables naming its files are
//! unset.
//!
//! - `OXIDGENE_REAL_GW`: a GeneWeb `.gw` export of a Geneanet tree;
//! - `OXIDGENE_REAL_SESSION`: a saved Geneanet session of the same account;
//! - `OXIDGENE_REAL_ARCHIVES`: its media archives, comma-separated (optional);
//! - `OXIDGENE_REAL_FIDELITY`: `originals` or `renditions` (defaults to
//!   `originals` when archives are given);
//! - `OXIDGENE_REAL_GED`: GEDCOM files, comma-separated;
//! - `OXIDGENE_REAL_GDZ`: a GEDZIP archive;
//! - `OXIDGENE_REAL_WORKDIR`: where the database, media and job storage go
//!   (defaults to `target/real-import`). It is deleted afterwards.
//!
//! These files are real genealogy. The tests print aggregates only — counts,
//! timings, peak memory, error codes — never a row, a name or an error
//! message, which can quote the file.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine as _;
use http_body_util::BodyExt;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::entities::{
    citation, event, family, media, media_link, note, person, place, record_version, source,
    vignette,
};
use oxidgene_db::repo::{PersonDenormRepo, PersonSearchRepo, connect, run_migrations};
use oxidgene_db::sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect,
};
use serde_json::{Value, json};
use tokio::io::AsyncReadExt as _;
use tower::ServiceExt as _;
use uuid::Uuid;

const SKIP: &str = "needs the maintainer's private samples; see the module docs";

// ── Environment ─────────────────────────────────────────────────────────

fn path_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn list_var(name: &str) -> Vec<PathBuf> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// A database, a media root and the app and worker over them, in a
/// directory of their own that is deleted with this value.
struct Backend {
    dir: PathBuf,
    media_root: PathBuf,
    db: DatabaseConnection,
    app: Router,
    worker: tokio::task::JoinHandle<()>,
}

impl Backend {
    async fn start(name: &str) -> Self {
        // As the desktop's `main` does first.
        oxidgene_api::memory::tune();
        let base = path_var("OXIDGENE_REAL_WORKDIR").unwrap_or_else(|| {
            PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../target/real-import"
            ))
        });
        let dir = base.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creates the work directory");
        report(
            "setup",
            &format!(
                "work dir on disk, temporary dir {}",
                if std::env::temp_dir().starts_with("/tmp") {
                    "UNDER /tmp (RAM): set TMPDIR"
                } else {
                    "on disk"
                }
            ),
        );
        let media_root = dir.join("media");
        let url = format!("sqlite://{}?mode=rwc", dir.join("oxidgene.db").display());
        let db = connect(&url).await.expect("opens the database");
        run_migrations(&db).await.expect("migrates the database");
        let state = AppState::new(db.clone(), &media_root).with_local_file_access();
        // As the desktop does: one in-process worker sharing the app's parts.
        let worker = BackgroundJobWorker::new(
            state.db.clone(),
            std::sync::Arc::clone(&state.profiles),
            std::sync::Arc::clone(&state.media),
            "real-import",
        );
        let worker = tokio::spawn(worker.run());
        Self {
            dir,
            media_root,
            db,
            app: build_router(state),
            worker,
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.worker.abort();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ── Reporting ───────────────────────────────────────────────────────────

fn report(phase: &str, line: &str) {
    eprintln!("[real-import] {phase}: {line}");
}

/// A memory field of `/proc/self/status`, in MiB.
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

/// The process's peak resident set since the last [`reset_peak`], in MiB.
fn peak_rss_mib() -> u64 {
    status_mib("VmHWM:")
}

fn reset_peak() {
    // Writing 5 resets VmHWM to the current resident set (Linux).
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// Time `work` as phase `name`, with the peak memory it reached and what
/// the process still held after it.
async fn phase<T>(name: &str, work: impl std::future::Future<Output = T>) -> T {
    reset_peak();
    let started = Instant::now();
    let value = work.await;
    report(
        name,
        &format!(
            "{:.1} s, peak RSS {} MiB, then RSS {} MiB",
            started.elapsed().as_secs_f64(),
            peak_rss_mib(),
            status_mib("VmRSS:")
        ),
    );
    value
}

/// What went wrong, collected so one long run reports every failure.
#[derive(Default)]
struct Failures(Vec<String>);

impl Failures {
    fn check(&mut self, ok: bool, what: impl Into<String>) {
        if !ok {
            let what = what.into();
            report("FAILED", &what);
            self.0.push(what);
        }
    }

    fn assert_none(&self) {
        assert!(
            self.0.is_empty(),
            "{} failure(s): {:?}",
            self.0.len(),
            self.0
        );
    }
}

// ── HTTP ────────────────────────────────────────────────────────────────

/// Send a request; the status and JSON answer (`Null` when not JSON).
async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    content_type: &str,
    body: Body,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A route without its identifiers, for reporting.
fn route_of(uri: &str) -> String {
    uri.split('?')
        .next()
        .unwrap_or_default()
        .split('/')
        .map(|segment| {
            if Uuid::parse_str(segment).is_ok() {
                "{id}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The answer of a request that must succeed. A failure reports the status
/// and the stable error code, never the message.
async fn expect_ok(
    app: &Router,
    method: Method,
    uri: &str,
    content_type: &str,
    body: Body,
) -> Value {
    let (status, value) = send(app, method, uri, content_type, body).await;
    assert!(
        status.is_success(),
        "{} answered {status} with error code {}",
        route_of(uri),
        value["error"]
    );
    value
}

async fn post_json(app: &Router, uri: &str, body: &Value) -> Value {
    expect_ok(
        app,
        Method::POST,
        uri,
        "application/json",
        Body::from(serde_json::to_vec(body).unwrap()),
    )
    .await
}

async fn get(app: &Router, uri: &str) -> Value {
    expect_ok(app, Method::GET, uri, "application/json", Body::empty()).await
}

/// `path` streamed in 1 MiB pieces, as the desktop sends a picked file.
fn file_body(path: &Path) -> Body {
    let file = tokio::fs::File::from_std(std::fs::File::open(path).expect("opens the sample"));
    let stream = futures_util::stream::unfold(file, |mut file| async move {
        let mut buffer = vec![0; 1 << 20];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(read) => {
                buffer.truncate(read);
                Some((Ok::<_, std::io::Error>(Bytes::from(buffer)), file))
            }
            Err(error) => Some((Err(error), file)),
        }
    });
    Body::from_stream(stream)
}

async fn new_tree(app: &Router, name: &str) -> Uuid {
    let tree = post_json(app, "/api/v1/trees", &json!({ "name": name })).await;
    tree["id"].as_str().unwrap().parse().unwrap()
}

/// Poll import or export job `job_id` until it ends, as the wizard does;
/// its final status. Reports the slowest poll: on SQLite a poll that misses
/// the in-memory progress waits for the import's connection.
async fn follow_job(app: &Router, tree_id: Uuid, kind: &str, job_id: &str) -> Value {
    let uri = format!("/api/v1/trees/{tree_id}/{kind}/{job_id}");
    let mut slowest = Duration::ZERO;
    let mut last_phase = String::new();
    loop {
        let asked = Instant::now();
        let status = get(app, &uri).await;
        slowest = slowest.max(asked.elapsed());
        let phase = status["phase"].as_str().unwrap_or_default().to_owned();
        if phase != last_phase {
            report(kind, &format!("phase {phase}"));
            last_phase = phase.clone();
        }
        if phase == "completed" || phase == "failed" {
            report(
                kind,
                &format!("slowest status poll {} ms", slowest.as_millis()),
            );
            return status;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Stream the response of GET `uri` to `path`, as the desktop saves an
/// export: never whole in memory.
async fn download(app: &Router, uri: &str, path: &Path) -> u64 {
    use futures_util::StreamExt as _;
    use tokio::io::AsyncWriteExt as _;

    let request = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert!(
        response.status().is_success(),
        "{} answered {}",
        route_of(uri),
        response.status()
    );
    let mut file = tokio::fs::File::create(path).await.unwrap();
    let mut written = 0;
    let mut stream = response.into_body().into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("reads the download");
        written += chunk.len() as u64;
        file.write_all(&chunk).await.unwrap();
    }
    file.flush().await.unwrap();
    written
}

/// The numbers and flags of a JSON object, with the length of each list;
/// never a string, which may carry a name.
fn numbers(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return String::new();
    };
    object
        .iter()
        .filter_map(|(key, value)| match value {
            Value::Number(number) => Some(format!("{key}={number}")),
            Value::Bool(flag) => Some(format!("{key}={flag}")),
            Value::Array(items) => Some(format!("{key}.len={}", items.len())),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn size_of(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |meta| meta.len())
}

fn tree_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| match entry.file_type() {
                Ok(kind) if kind.is_dir() => tree_size(&entry.path()),
                _ => entry.metadata().map_or(0, |meta| meta.len()),
            })
            .sum()
    })
}

// ── What a tree holds ───────────────────────────────────────────────────

/// Row counts of one tree, live rows only.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Counts(BTreeMap<&'static str, u64>);

impl Counts {
    fn get(&self, key: &str) -> u64 {
        self.0.get(key).copied().unwrap_or(0)
    }
}

/// Media rows that point at nothing, or that nothing on disk backs.
#[derive(Debug, Default)]
struct Orphans {
    links_to_missing_media: usize,
    links_to_missing_person: usize,
    links_without_target: usize,
    vignettes_to_missing_media: usize,
    media_without_file: usize,
    media_without_thumbnail_file: usize,
}

impl Orphans {
    fn total(&self) -> usize {
        self.links_to_missing_media
            + self.links_to_missing_person
            + self.links_without_target
            + self.vignettes_to_missing_media
            + self.media_without_file
            + self.media_without_thumbnail_file
    }
}

async fn live_persons(db: &DatabaseConnection, tree_id: Uuid) -> HashSet<Uuid> {
    person::Entity::find()
        .select_only()
        .column(person::Column::Id)
        .filter(person::Column::TreeId.eq(tree_id))
        .filter(person::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

async fn count_records(
    db: &DatabaseConnection,
    tree_id: Uuid,
    counts: &mut BTreeMap<&'static str, u64>,
) {
    let families = family::Entity::find()
        .filter(family::Column::TreeId.eq(tree_id))
        .filter(family::Column::DeletedAt.is_null())
        .count(db);
    counts.insert("families", families.await.unwrap());
    let events = event::Entity::find()
        .filter(event::Column::TreeId.eq(tree_id))
        .filter(event::Column::DeletedAt.is_null())
        .count(db);
    counts.insert("events", events.await.unwrap());
    let places = place::Entity::find()
        .filter(place::Column::TreeId.eq(tree_id))
        .count(db);
    counts.insert("places", places.await.unwrap());
    let notes = note::Entity::find()
        .filter(note::Column::TreeId.eq(tree_id))
        .filter(note::Column::DeletedAt.is_null())
        .count(db);
    counts.insert("notes", notes.await.unwrap());
    let sources: HashSet<Uuid> = source::Entity::find()
        .select_only()
        .column(source::Column::Id)
        .filter(source::Column::TreeId.eq(tree_id))
        .filter(source::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
        .unwrap()
        .into_iter()
        .collect();
    counts.insert("sources", sources.len() as u64);
    let citations = citation::Entity::find()
        .select_only()
        .column(citation::Column::SourceId)
        .into_tuple::<Uuid>()
        .all(db)
        .await
        .unwrap();
    let citations = citations.iter().filter(|id| sources.contains(id)).count();
    counts.insert("citations", citations as u64);
}

async fn count_media(
    backend: &Backend,
    tree_id: Uuid,
    persons: &HashSet<Uuid>,
    counts: &mut BTreeMap<&'static str, u64>,
) -> Orphans {
    let db = &backend.db;
    let rows = media::Entity::find()
        .filter(media::Column::TreeId.eq(tree_id))
        .filter(media::Column::DeletedAt.is_null())
        .all(db)
        .await
        .unwrap();
    let ids: HashSet<Uuid> = rows.iter().map(|row| row.id).collect();
    let mine = |media_id: Uuid, person_id: Option<Uuid>| {
        ids.contains(&media_id) || person_id.is_some_and(|id| persons.contains(&id))
    };
    let links: Vec<_> = media_link::Entity::find().all(db).await.unwrap();
    let links: Vec<_> = links
        .into_iter()
        .filter(|link| mine(link.media_id, link.person_id))
        .collect();
    let vignettes: Vec<_> = vignette::Entity::find().all(db).await.unwrap();
    let vignettes: Vec<_> = vignettes
        .into_iter()
        .filter(|row| mine(row.media_id, row.person_id))
        .collect();
    let stored = |key: &Option<String>| {
        key.as_ref()
            .is_some_and(|key| backend.media_root.join(key).is_file())
    };
    let linked: HashSet<Uuid> = links.iter().map(|link| link.media_id).collect();

    counts.insert("media", rows.len() as u64);
    // The receipt's own definition, applied to what is stored.
    let records = oxidgene_api::service::gedcom::MediaCounts::of_rows(
        rows.iter().map(|row| (row.id, row.parent_media_id)),
    );
    counts.insert("images", records.images_count as u64);
    counts.insert("documents", records.documents_count as u64);
    counts.insert("document_pages", records.document_pages_count as u64);
    let held = rows.iter().filter(|row| row.storage_key.is_some());
    counts.insert("media_stored", held.count() as u64);
    let pages = rows.iter().filter(|row| row.parent_media_id.is_some());
    counts.insert("media_pages", pages.count() as u64);
    let with_link = rows.iter().filter(|row| linked.contains(&row.id));
    counts.insert("media_linked", with_link.count() as u64);
    counts.insert("media_links", links.len() as u64);
    counts.insert("vignettes", vignettes.len() as u64);
    Orphans {
        links_to_missing_media: links
            .iter()
            .filter(|link| !ids.contains(&link.media_id))
            .count(),
        links_to_missing_person: links
            .iter()
            .filter(|link| link.person_id.is_some_and(|id| !persons.contains(&id)))
            .count(),
        links_without_target: links
            .iter()
            .filter(|link| {
                link.person_id.is_none()
                    && link.event_id.is_none()
                    && link.source_id.is_none()
                    && link.family_id.is_none()
            })
            .count(),
        vignettes_to_missing_media: vignettes
            .iter()
            .filter(|row| !ids.contains(&row.media_id))
            .count(),
        // A GEDCOM names files it cannot carry: those rows have no key.
        media_without_file: rows
            .iter()
            .filter(|row| row.storage_key.is_some() && !stored(&row.storage_key))
            .count(),
        media_without_thumbnail_file: rows
            .iter()
            .filter(|row| row.thumbnail_key.is_some() && !stored(&row.thumbnail_key))
            .count(),
    }
}

/// Count tree `tree_id` and check it is whole: no orphan media, every
/// person projected and searchable, no history recorded by the import, and
/// the tree-wide reads answering.
async fn check_tree(
    backend: &Backend,
    tree_id: Uuid,
    label: &str,
    failures: &mut Failures,
) -> Counts {
    let db = &backend.db;
    let persons = live_persons(db, tree_id).await;
    let mut counts = BTreeMap::from([("persons", persons.len() as u64)]);
    count_records(db, tree_id, &mut counts).await;
    let orphans = count_media(backend, tree_id, &persons, &mut counts).await;
    let counts = Counts(counts);
    report(label, &format!("counts {:?}", counts.0));
    report(label, &format!("{orphans:?}"));
    failures.check(
        counts.get("persons") > 0,
        format!("{label}: no person imported"),
    );
    failures.check(
        orphans.total() == 0,
        format!("{label}: orphan media rows {orphans:?}"),
    );

    // Before any tree read, which would build what the import left out.
    let materialized = PersonDenormRepo::is_materialized(db, tree_id)
        .await
        .unwrap();
    let projections = PersonDenormRepo::count_tree(db, tree_id).await.unwrap();
    let search_rows = PersonSearchRepo::count_tree(db, tree_id).await.unwrap();
    report(
        label,
        &format!("materialized={materialized} projections={projections} search_rows={search_rows}"),
    );
    let persons = counts.get("persons");
    failures.check(
        materialized,
        format!("{label}: projections not materialized"),
    );
    failures.check(
        projections == persons,
        format!("{label}: {projections} projections for {persons} persons"),
    );
    failures.check(
        search_rows == persons,
        format!("{label}: {search_rows} search rows for {persons} persons"),
    );
    let versions = record_version::Entity::find()
        .filter(record_version::Column::TreeId.eq(tree_id))
        .count(db)
        .await
        .unwrap();
    failures.check(
        versions == 0,
        format!("{label}: the import stored {versions} record versions"),
    );

    tree_reads(&backend.app, tree_id, label, failures).await;
    counts
}

/// The tree-wide reads a user reaches right after an import.
async fn tree_reads(app: &Router, tree_id: Uuid, label: &str, failures: &mut Failures) {
    let base = format!("/api/v1/trees/{tree_id}");
    let tree = get(app, &base).await;
    let mut reads: Vec<String> = [
        "/statistics",
        "/statistics/growth",
        "/anomalies",
        "/duplicates",
        "/unlocated-places",
        "/dictionary/family-names",
        "/dictionary/places",
        "/dictionary/sources",
        "/dictionary/occupations",
        "/persons?first=100",
        "/persons/search?limit=50",
        "/media?first=100",
        "/portraits",
    ]
    .map(str::to_owned)
    .into();
    match tree["sosa_root_person_id"].as_str() {
        Some(root) => {
            reads.push(format!(
                "/pedigree/{root}?ancestor_depth=6&descendant_depth=3"
            ));
            reads.push("/ancestry-completeness".to_owned());
        }
        None => report(label, "no SOSA root set: pedigree not read"),
    }
    for read in reads {
        let uri = format!("{base}{read}");
        let started = Instant::now();
        let (status, value) = send(app, Method::GET, &uri, "application/json", Body::empty()).await;
        let route = route_of(&uri);
        let elapsed = started.elapsed().as_millis();
        report(label, &format!("GET {route} -> {status} in {elapsed} ms"));
        failures.check(
            status.is_success(),
            format!("{label}: {route} answered {status} ({})", value["error"]),
        );
    }
}

/// Flag every count the receipt and the database disagree on.
fn compare_receipt(
    label: &str,
    receipt: &Value,
    counts: &Counts,
    pairs: &[(&str, &str)],
    failures: &mut Failures,
) {
    for (field, key) in pairs {
        let announced = receipt[field].as_u64().unwrap_or(0);
        let stored = counts.get(key);
        failures.check(
            announced == stored,
            format!("{label}: receipt {field}={announced} but {stored} {key} stored"),
        );
    }
}

const RECEIPT_COUNTS: [(&str, &str); 9] = [
    ("persons_count", "persons"),
    ("families_count", "families"),
    ("events_count", "events"),
    ("sources_count", "sources"),
    ("places_count", "places"),
    ("notes_count", "notes"),
    ("images_count", "images"),
    ("documents_count", "documents"),
    ("document_pages_count", "document_pages"),
];

// ── Scenarios ───────────────────────────────────────────────────────────

/// Upload `path` as an import job of `format` into a new tree, follow it to
/// its end and check the tree; the tree and its counts, or `None` when the
/// job failed.
async fn import_file(
    backend: &Backend,
    format: &str,
    path: &Path,
    label: &str,
    failures: &mut Failures,
) -> Option<(Uuid, Counts)> {
    let app = &backend.app;
    report(label, &format!("source {}", mib(size_of(path))));
    let tree_id = new_tree(app, &format!("Real import {label}")).await;
    let uri =
        format!("/api/v1/trees/{tree_id}/import-jobs?format={format}&filename=import.{format}");
    let started = phase(
        &format!("{label}.upload"),
        expect_ok(
            app,
            Method::POST,
            &uri,
            "application/octet-stream",
            file_body(path),
        ),
    )
    .await;
    let job_id = started["job_id"].as_str().unwrap().to_owned();
    let status = phase(
        &format!("{label}.import_job"),
        follow_job(app, tree_id, "import-jobs", &job_id),
    )
    .await;
    let completed = status["phase"] == "completed";
    failures.check(
        completed,
        format!("{label}: import job failed with code {}", status["error"]),
    );
    if !completed {
        return None;
    }
    let receipt = &status["result"];
    report(label, &format!("receipt {}", numbers(receipt)));
    let counts = phase(
        &format!("{label}.checks"),
        check_tree(backend, tree_id, label, failures),
    )
    .await;
    compare_receipt(label, receipt, &counts, &RECEIPT_COUNTS, failures);
    Some((tree_id, counts))
}

/// Export tree `tree_id` as GEDZIP, re-import the archive into a new tree,
/// and flag every count that changed on the way.
async fn round_trip(backend: &Backend, tree_id: Uuid, original: &Counts, failures: &mut Failures) {
    let app = &backend.app;
    let base = format!("/api/v1/trees/{tree_id}/export-jobs");
    let started = expect_ok(app, Method::POST, &base, "application/json", Body::empty()).await;
    let job_id = started["job_id"].as_str().unwrap().to_owned();
    let status = phase(
        "roundtrip.export_job",
        follow_job(app, tree_id, "export-jobs", &job_id),
    )
    .await;
    failures.check(
        status["phase"] == "completed",
        format!("roundtrip: export job failed with code {}", status["error"]),
    );
    let Some(download_url) = status["download_url"].as_str() else {
        return;
    };
    report("roundtrip", &format!("export {}", numbers(&status)));
    let archive = backend.dir.join("export.gdz");
    let size = phase("roundtrip.download", download(app, download_url, &archive)).await;
    report("roundtrip", &format!("archive {}", mib(size)));
    let copied = import_file(backend, "gedzip", &archive, "roundtrip", failures).await;
    let _ = std::fs::remove_file(&archive);
    let Some((_, copied)) = copied else {
        return;
    };
    for (key, before) in &original.0 {
        let after = copied.get(key);
        failures.check(
            *before == after,
            format!("roundtrip: {key} {before} -> {after}"),
        );
    }
}

fn media_fidelity(archives: &[PathBuf]) -> &'static str {
    match std::env::var("OXIDGENE_REAL_FIDELITY").as_deref() {
        Ok("renditions") => "renditions",
        Ok("originals") => "originals",
        _ if archives.is_empty() => "renditions",
        _ => "originals",
    }
}

/// Step 2 of the wizard: index the archives in place.
async fn index_archives(app: &Router, archive_paths: &[String], failures: &mut Failures) {
    let index = phase(
        "geneanet.archives",
        post_json(
            app,
            "/api/v1/geneanet/archives",
            &json!({ "paths": archive_paths }),
        ),
    )
    .await;
    let rows = index["archives"].as_array().cloned().unwrap_or_default();
    let unreadable = rows.iter().filter(|row| !row["error"].is_null()).count();
    report(
        "geneanet.archives",
        &format!("file_count={} unreadable={unreadable}", index["file_count"]),
    );
    failures.check(
        unreadable == 0,
        format!("geneanet: {unreadable} archive(s) unreadable"),
    );
}

/// Step 3, from a saved file: decode the session.
async fn decode_session(app: &Router, session_path: &Path) -> Value {
    let session = phase(
        "geneanet.session_decode",
        expect_ok(
            app,
            Method::POST,
            "/api/v1/geneanet/session/decode",
            "application/octet-stream",
            file_body(session_path),
        ),
    )
    .await;
    let media = session["media"].as_object().map_or(0, serde_json::Map::len);
    let deposits = session["deposit_sizes"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    report(
        "geneanet.session_decode",
        &format!(
            "photo_count={} media={media} deposits={deposits}",
            session["photo_count"]
        ),
    );
    session
}

/// Step 4: preview and plan; reports what the login window would still
/// have to fetch.
async fn preview_and_plan(
    app: &Router,
    body: &Value,
    session: &Value,
    failures: &mut Failures,
) -> Value {
    let preview = phase(
        "geneanet.preview",
        post_json(app, "/api/v1/geneanet/preview", body),
    )
    .await;
    report("geneanet.preview", &numbers(&preview));
    failures.check(
        preview["mismatch"] != true,
        "geneanet: the preview reports a mismatch",
    );

    let plan = phase(
        "geneanet.plan",
        post_json(app, "/api/v1/geneanet/plan", body),
    )
    .await;
    let needed: HashSet<&str> = plan["needed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["url"].as_str())
        .collect();
    let held = session["media"].as_object();
    let missing = needed
        .iter()
        .filter(|url| held.is_none_or(|media| !media.contains_key(**url)))
        .count();
    report(
        "geneanet.plan",
        &format!("needed={} missing_from_session={missing}", needed.len()),
    );
    if missing > 0 {
        report(
            "geneanet.plan",
            "the login window would fetch the missing media; this run imports without them",
        );
    }
    preview
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs your own exports: run by `just real-import`"]
async fn geneanet_wizard_import_survives_a_gedzip_round_trip() {
    let (Some(gw_path), Some(session_path)) = (
        path_var("OXIDGENE_REAL_GW"),
        path_var("OXIDGENE_REAL_SESSION"),
    ) else {
        report("geneanet", SKIP);
        return;
    };
    let archives = list_var("OXIDGENE_REAL_ARCHIVES");
    let fidelity = media_fidelity(&archives);
    let backend = Backend::start("geneanet").await;
    let app = &backend.app;
    let mut failures = Failures::default();
    let gw = std::fs::read(&gw_path).expect("reads the .gw");
    let archive_size = archives.iter().map(|path| size_of(path)).sum();
    report(
        "inputs",
        &format!(
            "gw {}, session {}, {} archive(s) {}, fidelity {fidelity}",
            mib(gw.len() as u64),
            mib(size_of(&session_path)),
            archives.len(),
            mib(archive_size),
        ),
    );

    let inspection = phase(
        "geneanet.inspect",
        expect_ok(
            app,
            Method::POST,
            "/api/v1/geneweb/inspect?filename=export.gw",
            "application/octet-stream",
            Body::from(gw.clone()),
        ),
    )
    .await;
    report("geneanet.inspect", &numbers(&inspection));
    let archive_paths: Vec<String> = archives
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    if fidelity == "originals" {
        index_archives(app, &archive_paths, &mut failures).await;
    }
    let session = decode_session(app, &session_path).await;
    let mut body = json!({
        "gw_base64": base64::engine::general_purpose::STANDARD.encode(&gw),
        "file_name": "export.gw",
        "collection": session["collection"],
        "deposit_sizes": session["deposit_sizes"],
        "archive_paths": archive_paths,
        "media_fidelity": fidelity,
    });
    let preview = preview_and_plan(app, &body, &session, &mut failures).await;

    // Step 5: the wizard hands back every medium the session staged.
    body["fetched"] = session["media"].clone();
    drop(session);
    let tree_id = new_tree(app, "Real import geneanet").await;
    let started = phase(
        "geneanet.import_request",
        post_json(
            app,
            &format!("/api/v1/trees/{tree_id}/geneanet/import"),
            &body,
        ),
    )
    .await;
    drop(body);
    let job_id = started["job_id"].as_str().unwrap().to_owned();
    let status = phase(
        "geneanet.import_job",
        follow_job(app, tree_id, "import-jobs", &job_id),
    )
    .await;
    let completed = status["phase"] == "completed";
    failures.check(
        completed,
        format!("geneanet: import job failed with code {}", status["error"]),
    );
    if !completed {
        failures.assert_none();
    }
    let receipt = &status["geneanet_result"];
    report("geneanet", &format!("receipt {}", numbers(receipt)));
    let counts = phase(
        "geneanet.checks",
        check_tree(&backend, tree_id, "geneanet", &mut failures),
    )
    .await;
    check_geneanet_receipt(receipt, &preview, &counts, &mut failures);
    report(
        "geneanet",
        &format!("media root {}", mib(tree_size(&backend.media_root))),
    );

    round_trip(&backend, tree_id, &counts, &mut failures).await;
    failures.assert_none();
}

/// The Geneanet receipt against the database and against what the preview
/// announced.
fn check_geneanet_receipt(
    receipt: &Value,
    preview: &Value,
    counts: &Counts,
    failures: &mut Failures,
) {
    // Its persons are the `.gw`'s, the isolated ones counted apart; its
    // media are counted as every import's are.
    compare_receipt(
        "geneanet",
        receipt,
        counts,
        &[
            ("families_count", "families"),
            ("events_count", "events"),
            ("sources_count", "sources"),
            ("places_count", "places"),
            ("notes_count", "notes"),
            ("images_count", "images"),
            ("documents_count", "documents"),
            ("document_pages_count", "document_pages"),
            ("links_count", "media_links"),
            ("vignettes_count", "vignettes"),
        ],
        failures,
    );
    let isolated = receipt["isolated_count"].as_u64().unwrap_or(0);
    let announced = preview["person_count"].as_u64().unwrap_or(0);
    let imported = receipt["persons_count"].as_u64().unwrap_or(0);
    failures.check(
        imported == announced,
        format!("geneanet: preview announced {announced} persons, receipt {imported}"),
    );
    failures.check(
        imported + isolated == counts.get("persons"),
        format!(
            "geneanet: receipt {imported} persons + {isolated} isolated, {} stored",
            counts.get("persons")
        ),
    );
    failures.check(counts.get("media") > 0, "geneanet: no media imported");
    failures.check(
        counts.get("media_links") > 0,
        "geneanet: no media link imported",
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs your own exports: run by `just real-import`"]
async fn gedcom_files_import_whole() {
    let files = list_var("OXIDGENE_REAL_GED");
    if files.is_empty() {
        report("gedcom", SKIP);
        return;
    }
    let backend = Backend::start("gedcom").await;
    let mut failures = Failures::default();
    for (index, path) in files.iter().enumerate() {
        let label = format!("gedcom{}", index + 1);
        import_file(&backend, "gedcom", path, &label, &mut failures).await;
    }
    failures.assert_none();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs your own exports: run by `just real-import`"]
async fn gedzip_file_imports_whole_and_round_trips() {
    let Some(path) = path_var("OXIDGENE_REAL_GDZ") else {
        report("gedzip", SKIP);
        return;
    };
    let backend = Backend::start("gedzip").await;
    let mut failures = Failures::default();
    let imported = import_file(&backend, "gedzip", &path, "gedzip", &mut failures).await;
    if let Some((tree_id, counts)) = imported {
        report(
            "gedzip",
            &format!("media root {}", mib(tree_size(&backend.media_root))),
        );
        round_trip(&backend, tree_id, &counts, &mut failures).await;
    }
    failures.assert_none();
}
