//! What the application costs on a large generated tree, step by step, in
//! time and resident memory.
//!
//! The relative growths of `algorithm_scaling_test` compare trees of a few
//! thousand persons with trees eight times larger; they cannot see what only
//! bites at a hundred thousand persons or a million — memory above all. Each
//! run here imports one [`population`] of `OXIDGENE_LARGE_TREE_PERSONS`
//! persons the way the application does (an upload, the worker's job, a
//! SQLite file with its read pool, or the PostgreSQL database
//! `OXIDGENE_TEST_POSTGRES_URL` names), then times a full rebuild, the
//! tree-wide pages cold, warm, after one edit and after ten, a search and
//! the person searches of [`SEARCHES`] with the search tables' sizes, two
//! kinships, the widest pedigrees, a place renamed under 60 and 2,000
//! births, and an export. It prints every figure and fails over the budgets
//! of [`BUDGETS`] for the sizes that have some, in a release build.
//!
//! Opt-in: `just scale-large` runs it for 100,000 persons, `just
//! scale-large 1000000` for a million (docs/development.md §2.1). A run
//! without the variable skips itself.

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
use oxidgene_db::sea_orm::{ConnectionTrait as _, DatabaseConnection, DbBackend, Statement};
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

/// The budgets, from what the run measured under SQLite in a release
/// build (docs/development.md §2.1): memory a quarter above it, time four
/// times it, since a shared runner is slower than the machine measured on
/// and a time budget only has to catch a change of order — the place
/// renamed took 28 s before its refresh stopped reading the tree. A size
/// without an entry is reported only.
const BUDGETS: &[Budget] = &[Budget {
    persons: 100_000,
    steps: &[
        ("import", 250.0, 1_300),
        ("rebuild", 210.0, 160),
        ("statistics (cold)", 20.0, 1_100),
        ("statistics (warm)", 10.0, 0),
        ("statistics (after an edit)", 10.0, 0),
        ("statistics (after ten edits)", 10.0, 0),
        ("anomalies", 10.0, 0),
        ("duplicates", 3.0, 0),
        ("ancestry", 1.0, 0),
        ("search", 1.0, 0),
        ("search (surname prefix)", 1.0, 0),
        ("search (given name and surname)", 1.0, 0),
        ("search (surname prefix and birth years)", 1.0, 0),
        ("search (free text)", 1.0, 0),
        ("search (browse)", 1.0, 0),
        ("kinship (cousins)", 7.0, 0),
        ("kinship (through unions)", 6.0, 0),
        ("pedigree (10 generations down)", 2.0, 0),
        ("pedigree (10 generations up)", 1.0, 0),
        ("place renamed (60 births)", 1.0, 0),
        ("place renamed (2,000 births)", 8.0, 0),
        ("export", 80.0, 1_950),
    ],
}];

/// The person searches the search page, its suggestions and the pickers
/// send, by the name each is reported under: a surname typed, a given name
/// and a surname, a surname with birth years, free text, and the browse
/// of the whole tree by name.
const SEARCHES: &[(&str, &str)] = &[
    ("search (surname prefix)", "/persons/search?surname=Ash"),
    (
        "search (given name and surname)",
        "/persons/search?surname=Ashdown&given_names=Arthur",
    ),
    (
        "search (surname prefix and birth years)",
        "/persons/search?surname=Ash&birth_from=1800&birth_to=1850",
    ),
    ("search (free text)", "/persons/search?q=Arthur%20Ash"),
    ("search (browse)", "/persons/search?q=&sort=name_asc"),
];

/// Runs of each of [`SEARCHES`] measured, after one that warms the caches;
/// the median is the step's figure.
const SEARCH_RUNS: usize = 7;

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

    /// Time each of [`SEARCHES`]: [`SEARCH_RUNS`] runs after a warm one,
    /// the median reported as its step.
    async fn searches(&mut self) {
        for (name, path) in SEARCHES {
            let uri = self.uri(path);
            let found = ok(&self.app, Method::GET, &uri, None).await;
            let mut costs = Vec::with_capacity(SEARCH_RUNS);
            for _ in 0..SEARCH_RUNS {
                let app = self.app.clone();
                let uri = uri.clone();
                let ((), cost) = measured(async move {
                    ok(&app, Method::GET, &uri, None).await;
                })
                .await;
                costs.push(cost);
            }
            costs.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
            let runs: Vec<String> = costs
                .iter()
                .map(|cost| format!("{:.1}", cost.seconds * 1000.0))
                .collect();
            let median = costs.swap_remove(SEARCH_RUNS / 2);
            eprintln!(
                "[large-tree] {name}: median {:.1} ms over {SEARCH_RUNS} runs ({} ms), \
                 {} matches, {} statements",
                median.seconds * 1000.0,
                runs.join(", "),
                found["total_count"],
                median.statements
            );
            self.costs.push((name, median));
        }
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
async fn application(work: &Path) -> (Router, BackgroundJobWorker, DatabaseConnection) {
    let postgres = std::env::var("OXIDGENE_TEST_POSTGRES_URL").is_ok();
    let connections: Connections = if postgres {
        setup_db().await.into()
    } else {
        setup_file_db(work).await
    };
    let db = connections.writer.clone();
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
    (build_router(state), worker, db)
}

/// The bytes each table and index of the person search holds on disk,
/// by name: SQLite's pages (`dbstat`), PostgreSQL's relations.
async fn search_storage(db: &DatabaseConnection) -> Vec<(String, i64)> {
    let sql = match db.get_database_backend() {
        DbBackend::Sqlite => {
            "SELECT name, SUM(pgsize) AS bytes FROM dbstat \
             WHERE name LIKE '%person_search%' GROUP BY name ORDER BY name"
        }
        _ => {
            "SELECT c.relname AS name, pg_relation_size(c.oid) AS bytes FROM pg_class c \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE n.nspname = current_schema() AND c.relname LIKE '%person_search%' \
             ORDER BY c.relname"
        }
    };
    db.query_all_raw(Statement::from_string(db.get_database_backend(), sql))
        .await
        .expect("reads the search's storage")
        .iter()
        .map(|row| {
            (
                row.try_get::<String>("", "name").unwrap(),
                row.try_get::<i64>("", "bytes").unwrap(),
            )
        })
        .collect()
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
#[ignore = "imports a generated tree of OXIDGENE_LARGE_TREE_PERSONS persons and measures it: run by `just scale-large`"]
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
    let work =
        WorkDir(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("large-tree-{persons}")));
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

    let (app, worker, db) = application(&work.0).await;
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
    run.job("import").await;
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
    run.searches().await;
    let storage = search_storage(&db).await;
    for (name, bytes) in &storage {
        eprintln!("[large-tree] search storage: {name} {} KiB", bytes >> 10);
    }
    eprintln!(
        "[large-tree] search storage: {} KiB in all",
        storage.iter().map(|(_, bytes)| bytes).sum::<i64>() >> 10
    );
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
