//! Algorithmic complexity of the tree-wide computations, measured in time.
//!
//! Each computation runs on two trees built from the same family block, one
//! eight times the size of the other. Linear work takes eight times as long
//! on the larger, quadratic work sixty-four times — in theory. In practice
//! the smaller tree fits in the processor's caches better than the larger,
//! which alone makes linear work take up to twice as long again. So each
//! growth is measured against the growth of a reference pass of known linear
//! cost over the same projections, one that reads them at random as the
//! computations do: linear work grows about as fast as the reference,
//! quadratic work several times faster once the trees are large enough for
//! the quadratic term to dominate — hence tens of thousands of persons for
//! the in-memory computations, replicated from one imported tree.
//!
//! A quadratic term with a small constant can still hide below that bound;
//! where one is known to lurk, a deterministic count of the work done guards
//! it in the normal suite instead (the duplicate candidates, for one). Timing
//! depends on the machine and wants an optimised build, so these tests are
//! opt-in: `just scaling` runs them in release mode. The SQL side of the
//! same question is answered deterministically by `query_scaling_test`.

use crate::common;

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::{Duration, Instant};

use axum::Router;
use axum::http::Method;
use chrono::{Datelike, NaiveDate};
use common::{
    app_on, family_blocks_gedcom, family_blocks_tree, import_job, ok, setup_db, worker_on,
};
use oxidgene_api::service::{ancestry, anomalies, duplicates, statistics};
use oxidgene_core::projection::PersonProfile;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use uuid::Uuid;

/// Family blocks of the smaller tree of the requests: 2 000 persons.
const SMALL: usize = 200;
/// Copies of that tree making the smaller tree of the in-memory
/// computations: 8 000 persons.
const SMALL_COPIES: usize = 4;
/// How many times larger the larger tree is.
const FACTOR: usize = 8;
/// Largest growth accepted, relative to the reference pass's: linear work
/// sits near 1, `n log n` work a little above, quadratic work near
/// [`FACTOR`].
const MAX_RELATIVE_GROWTH: f64 = 3.0;
/// Runs per measure; the median is kept.
const RUNS: usize = 3;

/// A tree of the survey: its id, its SOSA root and its projections.
struct Tree {
    id: String,
    root: Uuid,
    profiles: Vec<PersonProfile>,
}

async fn tree(app: &Router, db: &DatabaseConnection, blocks: usize) -> Tree {
    let (id, anchor) = family_blocks_tree(app, db, blocks).await;
    let json = common::all_profiles(app, &id).await;
    Tree {
        id,
        root: anchor.parse().unwrap(),
        profiles: serde_json::from_value(serde_json::Value::Array(json)).unwrap(),
    }
}

/// The smaller and the larger tree, over one router.
async fn trees() -> (Router, DatabaseConnection, Tree, Tree) {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let small = tree(&app, &db, SMALL).await;
    let large = tree(&app, &db, SMALL * FACTOR).await;
    (app, db, small, large)
}

/// Years a tree of [`SMALL`] blocks spans, four blocks a year.
const SPAN_YEARS: i64 = (SMALL / 4) as i64;

/// `copies` copies of `base`, each with ids of its own and its dates moved
/// [`SPAN_YEARS`] earlier than the previous copy's, so that the tree grows
/// back in time as an imported one does, homonyms no denser. Copy 0 is
/// `base` itself.
fn replicated(base: &Tree, copies: usize) -> Tree {
    fn remap(value: &mut Value, copy: u128, years: i64) {
        match value {
            Value::String(text) => {
                if let Ok(id) = text.parse::<Uuid>() {
                    *text = Uuid::from_u128(id.as_u128() ^ (copy << 64)).to_string();
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|v| remap(v, copy, years)),
            Value::Object(fields) => {
                for (key, field) in fields.iter_mut() {
                    match (key.as_str(), field.as_str()) {
                        ("date_sort", Some(date)) => {
                            let date: NaiveDate = date.parse().unwrap();
                            let moved = date.with_year(date.year() - years as i32).unwrap();
                            *field = Value::String(moved.to_string());
                        }
                        ("date_value", Some(year)) if year.parse::<i64>().is_ok() => {
                            *field =
                                Value::String((year.parse::<i64>().unwrap() - years).to_string());
                        }
                        _ => remap(field, copy, years),
                    }
                }
            }
            _ => {}
        }
    }
    let json = serde_json::to_value(&base.profiles).unwrap();
    let profiles = (0..copies)
        .flat_map(|copy| {
            let mut copy_json = json.clone();
            remap(&mut copy_json, copy as u128, copy as i64 * SPAN_YEARS);
            serde_json::from_value::<Vec<PersonProfile>>(copy_json).unwrap()
        })
        .collect();
    Tree {
        id: base.id.clone(),
        root: base.root,
        profiles,
    }
}

/// The smaller and the larger tree of the in-memory computations.
async fn replicated_trees() -> (Tree, Tree) {
    let db = setup_db().await;
    let base = tree(&app_on(db.clone()), &db, SMALL).await;
    (
        replicated(&base, SMALL_COPIES),
        replicated(&base, SMALL_COPIES * FACTOR),
    )
}

/// Median duration of [`RUNS`] runs of `f`.
fn median(mut f: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    times.sort();
    times[RUNS / 2]
}

/// The reference pass: every person's parents, spouses and children looked
/// up by id, as the computations under test do.
fn reference(profiles: &[PersonProfile]) {
    let by_id: HashMap<Uuid, &PersonProfile> = profiles.iter().map(|p| (p.person_id, p)).collect();
    let mut found = 0usize;
    for profile in profiles {
        let parents = profile
            .family_as_child
            .iter()
            .flat_map(|l| [l.father_id, l.mother_id]);
        let unions = profile.families_as_spouse.iter().flat_map(|l| {
            l.children_ids
                .iter()
                .copied()
                .map(Some)
                .chain([l.spouse_id])
        });
        found += parents
            .chain(unions)
            .flatten()
            .filter(|id| by_id.contains_key(id))
            .count();
    }
    black_box(found);
}

/// How many times longer the reference pass takes on the larger tree.
fn reference_growth(small: &Tree, large: &Tree) -> f64 {
    growth(
        median(|| reference(&small.profiles)),
        median(|| reference(&large.profiles)),
    )
}

fn growth(small: Duration, large: Duration) -> f64 {
    large.as_secs_f64() / small.as_secs_f64().max(1e-9)
}

/// Asserts that `what` grew at most [`MAX_RELATIVE_GROWTH`] times as fast as
/// the reference pass.
fn assert_scales(what: &str, small: Duration, large: Duration, reference: f64) {
    let growth = growth(small, large);
    let relative = growth / reference;
    eprintln!(
        "{what}: {small:?} → {large:?}, ×{growth:.1} for ×{FACTOR} persons \
         (reference ×{reference:.1}, relative ×{relative:.2})"
    );
    assert!(
        relative <= MAX_RELATIVE_GROWTH,
        "{what} grows faster than n log n: ×{growth:.1} for ×{FACTOR} persons, \
         ×{relative:.2} the reference pass's growth"
    );
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
}

/// Times `compute` on the projections of both trees.
async fn scaling(what: &str, compute: impl Fn(&[PersonProfile], Uuid)) {
    let (small, large) = replicated_trees().await;
    let reference = reference_growth(&small, &large);
    assert_scales(
        what,
        median(|| compute(&small.profiles, small.root)),
        median(|| compute(&large.profiles, large.root)),
        reference,
    );
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_duplicates() {
    scaling("duplicates", |profiles, _| {
        black_box(duplicates::potential_duplicates(profiles, &HashSet::new()));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_anomalies() {
    scaling("anomalies", |profiles, _| {
        black_box(anomalies::compute(
            profiles,
            &anomalies::Records::default(),
            today(),
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_statistics() {
    scaling("statistics", |profiles, _| {
        black_box(statistics::compute(
            &profiles.iter().collect::<Vec<_>>(),
            &[],
            &[],
            0,
            today(),
            false,
            |_| Vec::new(),
        ));
    })
    .await;
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_ancestry() {
    scaling("ancestry completeness", |profiles, root| {
        black_box(ancestry::compute(profiles, root, 8, today()));
    })
    .await;
}

/// Median duration of [`RUNS`] of the request.
async fn timed_request(
    app: &Router,
    method: &Method,
    uri: &str,
    body: Option<serde_json::Value>,
) -> Duration {
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let start = Instant::now();
        ok(app, method.clone(), uri, body.clone()).await;
        runs.push(start.elapsed());
    }
    runs.sort();
    runs[RUNS / 2]
}

/// Times each request on both trees; `{tree}` is substituted.
async fn scaling_requests(requests: &[(Method, &str)]) {
    let (app, _db, small, large) = trees().await;
    let reference = reference_growth(&small, &large);
    for (method, uri) in requests {
        let on = |tree: &Tree| uri.replace("{tree}", &tree.id);
        assert_scales(
            &format!("{method} {uri}"),
            timed_request(&app, method, &on(&small), None).await,
            timed_request(&app, method, &on(&large), None).await,
            reference,
        );
    }
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_import() {
    let (app, db, small, large) = trees().await;
    let reference = reference_growth(&small, &large);
    let worker = worker_on(&db);
    let mut times = Vec::new();
    for blocks in [SMALL, SMALL * FACTOR] {
        let gedcom = family_blocks_gedcom(blocks);
        let mut runs = Vec::new();
        for _ in 0..RUNS {
            let tree = ok(
                &app,
                Method::POST,
                "/api/v1/trees",
                Some(json!({ "name": "Scaling" })),
            )
            .await;
            let tree_id = tree["id"].as_str().unwrap();
            let start = Instant::now();
            let status = import_job(&app, &worker, tree_id, "format=gedcom", gedcom.clone()).await;
            runs.push(start.elapsed());
            assert_eq!(status["phase"], "completed", "{status}");
        }
        runs.sort();
        times.push(runs[RUNS / 2]);
    }
    assert_scales("GEDCOM import", times[0], times[1], reference);
}

#[tokio::test]
#[ignore = "timing: run by `just scaling` in release mode"]
async fn scaling_tree_wide_requests() {
    scaling_requests(&[
        (Method::POST, "/api/v1/trees/{tree}/profiles/rebuild"),
        (Method::GET, "/api/v1/trees/{tree}/gedcom/export"),
        (Method::GET, "/api/v1/trees/{tree}/statistics?lang=en"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/family-names"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/places"),
        (Method::GET, "/api/v1/trees/{tree}/dictionary/occupations"),
    ])
    .await;
}

/// What the application costs on a large generated tree, step by step, in
/// time and resident memory.
///
/// The relative growths above compare trees of a few thousand persons with
/// trees eight times larger; they cannot see what only bites at a hundred
/// thousand persons or a million — memory above all. Each run here imports
/// one [`population`] of `OXIDGENE_LARGE_TREE_PERSONS` persons the way the
/// application does (an upload, the worker's job, a SQLite file with its
/// read pool, or the PostgreSQL database `OXIDGENE_TEST_POSTGRES_URL`
/// names), then times a full rebuild, an edit, the tree-wide pages before
/// and after it, a search, two kinships, the widest pedigree, a place
/// renamed under 60 and 2,000 births, and an export. It prints every
/// figure and fails over the budgets of [`BUDGETS`] for the sizes that have
/// some, in a release build. A run without the variable skips itself.
mod large_tree {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use http_body_util::BodyExt as _;
    use oxidgene_api::service::background_job::BackgroundJobWorker;
    use oxidgene_api::tree_cache::{CacheSettings, DEFAULT_IDLE};
    use oxidgene_api::{AppState, build_router};
    use oxidgene_db::repo::Connections;
    use serde_json::{Value, json};
    use tower::ServiceExt as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    use crate::common::measure::{self, Cost, PhaseMemory, measured};
    use crate::common::population::{self, COUSIN, FOUNDER, PROBAND, STRANGER};
    use crate::common::{offline, ok, send, setup_db, setup_file_db};

    /// The spans whose time and peak memory a run reports as they close.
    const PHASES: &[&str] = &[
        "import.job",
        "import.parse",
        "import.persist",
        "import.projections",
        "profile.decode",
        "profile.patch",
        "export.job",
    ];

    /// The most a step may take and hold, for a tree of `persons` persons.
    struct Budget {
        persons: usize,
        /// Per step, by the name it is reported under: seconds, and MiB of
        /// peak resident memory (0 for a step whose memory is not gated).
        steps: &'static [(&'static str, f64, u64)],
    }

    /// The budgets, about a quarter above what the runs measured on a
    /// 16-thread machine under SQLite, in a release build (docs/development.md
    /// §2.7 has the measurements). A size without an entry is reported only.
    const BUDGETS: &[Budget] = &[];

    /// Deletes the run's directory, whatever the outcome.
    struct WorkDir(PathBuf);

    impl Drop for WorkDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Run {
        app: Router,
        worker: BackgroundJobWorker,
        tree: String,
        persons: usize,
        costs: Vec<(&'static str, Cost)>,
    }

    impl Run {
        /// Measure `step` and report it as `name`.
        async fn step<T>(&mut self, name: &'static str, step: impl Future<Output = T>) -> T {
            let (out, cost) = measured(step).await;
            eprintln!(
                "[large-tree] {name}: {:.2} s ({:.2} s of processor), peak RSS {} MiB, \
                 RSS after {} MiB, {} statements",
                cost.seconds, cost.cpu_seconds, cost.peak_mib, cost.after_mib, cost.statements
            );
            self.costs.push((name, cost));
            out
        }

        fn uri(&self, path: &str) -> String {
            format!("/api/v1/trees/{}{path}", self.tree)
        }

        /// `GET path` of the tree, measured as `name`; the answer's size in
        /// bytes and the answer.
        async fn get(&mut self, name: &'static str, path: &str) -> (usize, Value) {
            let uri = self.uri(path);
            let app = self.app.clone();
            self.step(name, async move {
                let request = Request::builder()
                    .method(Method::GET)
                    .uri(&uri)
                    .body(Body::empty())
                    .unwrap();
                let response = app.oneshot(request).await.unwrap();
                let status = response.status();
                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                assert!(status.is_success(), "{uri}: {status}");
                (
                    bytes.len(),
                    serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                )
            })
            .await
        }

        /// The person whose given name is `given`.
        async fn person(&self, given: &str) -> String {
            let found = ok(
                &self.app,
                Method::GET,
                &self.uri(&format!("/persons/search?q={given}")),
                None,
            )
            .await;
            found["entries"][0]["person_id"]
                .as_str()
                .unwrap_or_else(|| panic!("no person named {given}"))
                .to_owned()
        }

        /// The place named `name`.
        async fn place(&self, name: &str) -> String {
            let found = ok(
                &self.app,
                Method::GET,
                &self.uri(&format!("/places?name={}", name.replace(' ', "%20"))),
                None,
            )
            .await;
            found["edges"][0]["node"]["id"]
                .as_str()
                .unwrap_or_else(|| panic!("no place named {name}"))
                .to_owned()
        }

        /// Rename place `name`, measured as `step`.
        async fn rename_place(&mut self, step: &'static str, name: &str, renamed: &str) {
            let place = self.place(name).await;
            let uri = self.uri(&format!("/places/{place}"));
            let app = self.app.clone();
            let body = json!({ "name": renamed });
            self.step(step, async move {
                ok(&app, Method::PUT, &uri, Some(body)).await;
            })
            .await;
        }

        /// Run the job the worker finds queued, measured as `name`.
        async fn job(&mut self, name: &'static str) {
            let worker = self.worker.clone();
            self.step(name, async move {
                assert!(worker.run_once().await.expect("runs the job"), "no job");
            })
            .await;
        }
    }

    /// The population's GEDCOM, written to `path`.
    fn write_population(path: &Path, persons: usize) -> population::Population {
        let file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        population::write_gedcom(file, persons).expect("writes the population")
    }

    /// The router and the worker over a fresh database: a SQLite file under
    /// `work` with its read pool and the desktop's one-tree cache, or a
    /// schema of the PostgreSQL database of the run with the server's.
    async fn application(work: &Path) -> (Router, BackgroundJobWorker) {
        let postgres = std::env::var("OXIDGENE_TEST_POSTGRES_URL").is_ok();
        let connections: Connections = if postgres {
            setup_db().await.into()
        } else {
            setup_file_db(work).await
        };
        let state = offline(AppState::new(connections, work.join("media")));
        if !postgres {
            state
                .profiles
                .cache()
                .configure(CacheSettings::one_tree(DEFAULT_IDLE));
        }
        let worker = BackgroundJobWorker::new(
            state.db.clone(),
            Arc::clone(&state.profiles),
            Arc::clone(&state.media),
            "large-tree",
        );
        (build_router(state), worker)
    }

    /// Upload the GEDCOM at `path` as an import job of `tree`, streamed
    /// from the file as a browser sends it.
    async fn upload(app: &Router, tree: &str, path: &Path) {
        let file = tokio::fs::File::open(path).await.unwrap();
        let chunks = futures_util::stream::unfold(file, |mut file| async move {
            use tokio::io::AsyncReadExt as _;
            let mut chunk = vec![0u8; 1 << 20];
            match file.read(&mut chunk).await {
                Ok(0) => None,
                Ok(read) => {
                    chunk.truncate(read);
                    Some((Ok::<_, std::io::Error>(chunk), file))
                }
                Err(error) => Some((Err(error), file)),
            }
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/trees/{tree}/import-jobs?format=gedcom"))
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(Body::from_stream(chunks))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "upload refused");
    }

    fn check_budgets(run: &Run) {
        let Some(budget) = BUDGETS.iter().find(|b| b.persons == run.persons) else {
            return;
        };
        if cfg!(debug_assertions) {
            eprintln!("[large-tree] debug build: budgets not checked");
            return;
        }
        let mut over = Vec::new();
        for (name, seconds, mib) in budget.steps {
            let Some((_, cost)) = run.costs.iter().find(|(step, _)| step == name) else {
                over.push(format!("{name}: not measured"));
                continue;
            };
            if cost.seconds > *seconds {
                over.push(format!("{name}: {:.2} s, over {seconds} s", cost.seconds));
            }
            if *mib > 0 && cost.peak_mib > *mib {
                over.push(format!(
                    "{name}: peak {} MiB, over {mib} MiB",
                    cost.peak_mib
                ));
            }
        }
        assert!(over.is_empty(), "over budget:\n{}", over.join("\n"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "imports a tree of OXIDGENE_LARGE_TREE_PERSONS persons and measures it: run by `just scaling` with that variable set"]
    async fn a_large_tree_stays_within_its_budgets() {
        let Some(persons) = std::env::var("OXIDGENE_LARGE_TREE_PERSONS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
        else {
            eprintln!("[large-tree] OXIDGENE_LARGE_TREE_PERSONS is unset: skipped");
            return;
        };
        // As every binary's `main` does first.
        oxidgene_api::memory::tune();
        tracing_subscriber::registry()
            .with(PhaseMemory::new("large-tree", PHASES))
            .init();
        let work = WorkDir(
            PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("large-tree-{persons}")),
        );
        let _ = std::fs::remove_dir_all(&work.0);
        std::fs::create_dir_all(&work.0).unwrap();

        let source = work.0.join("population.ged");
        let written = write_population(&source, persons);
        eprintln!(
            "[large-tree] population: {} persons, {} families, GEDCOM {} MiB",
            written.persons,
            written.families,
            written.bytes >> 20
        );
        assert_eq!(written.persons, persons);

        let (app, worker) = application(&work.0).await;
        let tree = ok(
            &app,
            Method::POST,
            "/api/v1/trees",
            Some(json!({ "name": "Large" })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut run = Run {
            app,
            worker,
            tree,
            persons,
            costs: Vec::new(),
        };
        oxidgene_api::memory::release_free_memory();
        eprintln!(
            "[large-tree] before the import: RSS {} MiB",
            measure::status_mib("VmRSS:")
        );

        upload(&run.app, &run.tree, &source).await;
        measure::reset_highest_peak();
        run.job("import").await;
        eprintln!(
            "[large-tree] import: highest peak {} MiB",
            measure::highest_peak_mib()
        );
        let found = ok(&run.app, Method::GET, &run.uri("/persons/search?q="), None).await;
        assert_eq!(found["total_count"], persons, "every person is imported");

        let proband = run.person(PROBAND).await;
        let cousin = run.person(COUSIN).await;
        let founder = run.person(FOUNDER).await;
        let stranger = run.person(STRANGER).await;
        ok(
            &run.app,
            Method::PUT,
            &format!("/api/v1/trees/{}", run.tree),
            Some(json!({ "sosa_root_person_id": proband })),
        )
        .await;

        let app = run.app.clone();
        let uri = run.uri("/profiles/rebuild");
        run.step("rebuild", async move {
            ok(&app, Method::POST, &uri, None).await;
        })
        .await;

        run.get("statistics (cold)", "/statistics?lang=en").await;
        run.get("statistics (warm)", "/statistics?lang=en").await;
        run.get("anomalies", "/anomalies").await;
        run.get("duplicates", "/duplicates").await;
        run.get("ancestry", "/ancestry-completeness?generations=12")
            .await;

        // One edit, then the tree-wide page again: what editing costs the
        // next read.
        let app = run.app.clone();
        let uri = run.uri(&format!("/persons/{cousin}"));
        run.step("edit", async move {
            ok(&app, Method::PUT, &uri, Some(json!({ "sex": "unknown" }))).await;
        })
        .await;
        run.get("statistics (after an edit)", "/statistics?lang=en")
            .await;

        // Ten persons edited, then the page again: the rows a read has to
        // bring its projections up to date with.
        let found = ok(
            &run.app,
            Method::GET,
            &run.uri("/persons/search?q=Arthur&limit=10"),
            None,
        )
        .await;
        let edited: Vec<String> = found["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["person_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(edited.len(), 10);
        let app = run.app.clone();
        let uris: Vec<String> = edited
            .iter()
            .map(|person| run.uri(&format!("/persons/{person}")))
            .collect();
        run.step("ten edits", async move {
            for uri in uris {
                ok(&app, Method::PUT, &uri, Some(json!({ "sex": "unknown" }))).await;
            }
        })
        .await;
        run.get("statistics (after ten edits)", "/statistics?lang=en")
            .await;

        run.get("search", "/persons/search?q=Arthur%20Ashdown")
            .await;
        run.get(
            "kinship (cousins)",
            &format!("/persons/{proband}/kinship/{cousin}"),
        )
        .await;
        run.get(
            "kinship (through unions)",
            &format!("/persons/{founder}/kinship/{stranger}"),
        )
        .await;
        let (bytes, pedigree) = run
            .get(
                "pedigree (10 generations down)",
                &format!("/pedigree/{founder}?ancestor_depth=0&descendant_depth=10"),
            )
            .await;
        eprintln!(
            "[large-tree] pedigree (10 generations down): {} persons, {} KiB",
            pedigree["persons"].as_object().map_or(0, |p| p.len()),
            bytes >> 10
        );
        let (bytes, pedigree) = run
            .get(
                "pedigree (10 generations up)",
                &format!("/pedigree/{proband}?ancestor_depth=10&descendant_depth=0"),
            )
            .await;
        eprintln!(
            "[large-tree] pedigree (10 generations up): {} persons, {} KiB",
            pedigree["persons"].as_object().map_or(0, |p| p.len()),
            bytes >> 10
        );

        run.rename_place(
            "place renamed (60 births)",
            population::SMALL_HUB_PLACE,
            "Sixtyford Magna, Shire",
        )
        .await;
        run.rename_place(
            "place renamed (2,000 births)",
            population::HUB_PLACE,
            "Hubminster Magna, Shire",
        )
        .await;

        let (status, started) = send(
            &run.app,
            Method::POST,
            &run.uri("/export-jobs?include_media=false"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "{started}");
        measure::reset_highest_peak();
        run.job("export").await;
        let job = started["job_id"].as_str().unwrap();
        let exported = ok(
            &run.app,
            Method::GET,
            &run.uri(&format!("/export-jobs/{job}")),
            None,
        )
        .await;
        assert_eq!(exported["phase"], "completed", "{exported}");

        check_budgets(&run);
    }
}
