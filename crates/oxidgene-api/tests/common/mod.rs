//! What the integration tests share: a fresh database, a router over it, and
//! a JSON request against that router.
//!
//! Every test file is its own crate and compiles its own copy of this module,
//! using only some of it; hence the one `dead_code` allowance below.
#![allow(
    dead_code,
    reason = "each integration-test binary compiles this module and uses only some helpers"
)]

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use std::sync::Arc;

use oxidgene_api::media::FsStore;
use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::Value;
use tower::ServiceExt;

/// A migrated, empty in-memory SQLite database.
pub async fn setup_db() -> DatabaseConnection {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    db
}

/// Where [`app_on`] keeps media and staged job inputs.
///
/// A throwaway directory: `AppState` needs a root and it must not be the
/// developer's.
fn test_media_root() -> std::path::PathBuf {
    std::env::temp_dir().join("oxidgene-test-media")
}

/// The API router over `db`.
pub fn app_on(db: DatabaseConnection) -> Router {
    build_router(AppState::new(db, test_media_root()))
}

/// A background job worker over `db` and the media root of [`app_on`].
///
/// No worker runs on its own in these tests: one runs a job only when a
/// test asks it to, so a queued job stays queued until then.
pub fn worker_on(db: &DatabaseConnection) -> BackgroundJobWorker {
    BackgroundJobWorker::new(
        db.clone(),
        Arc::new(ProfileService::new(db.clone())),
        Arc::new(FsStore::new(test_media_root())),
        "test-worker",
    )
}

/// Import `bytes` into `tree_id` the way a client does: upload them as an
/// import job (`query` is the job's query string, `format=…` and an
/// optional `filename=…`), have `worker` run it, and return the job's final
/// status, `completed` with its summary in `result` or `failed` with its
/// `error`.
pub async fn import_job(
    app: &Router,
    worker: &BackgroundJobWorker,
    tree_id: &str,
    query: &str,
    bytes: impl Into<Vec<u8>>,
) -> Value {
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/trees/{tree_id}/import-jobs?{query}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes.into()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let started: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "import job not started: {started}"
    );
    let job_id = started["job_id"].as_str().expect("job id").to_owned();
    assert!(worker.run_once().await.expect("runs the import job"));
    ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await
}

/// [`import_job`] of GEDCOM text into `tree_id` of the [`app_on`] router
/// over `db`, which must complete; the import's summary.
pub async fn import_gedcom(
    app: &Router,
    db: &DatabaseConnection,
    tree_id: &str,
    gedcom: &str,
) -> Value {
    let status = import_job(
        app,
        &worker_on(db),
        tree_id,
        "format=gedcom",
        gedcom.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status["phase"], "completed", "import failed: {status}");
    status["result"].clone()
}

/// The API router over a fresh database.
pub async fn setup_app() -> Router {
    app_on(setup_db().await)
}

/// Send `body` as JSON and return the status with the JSON answer, or
/// `Value::Null` when the answer is empty or not JSON.
pub async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = match body {
        Some(json) => Body::from(serde_json::to_vec(&json).unwrap()),
        None => Body::empty(),
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
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

/// [`send`], requiring a success status; the JSON answer.
pub async fn ok(app: &Router, method: Method, uri: &str, body: Option<Value>) -> Value {
    let (status, json) = send(app, method, uri, body).await;
    assert!(status.is_success(), "{uri}: {status} {json}");
    json
}

/// Run a GraphQL operation; the whole response, data and errors alike.
pub async fn gql(app: &Router, query: &str, variables: Value) -> Value {
    let body = serde_json::json!({ "query": query, "variables": variables });
    let (status, json) = send(app, Method::POST, "/graphql", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "GraphQL transport failed: {json}");
    json
}

/// The `data` of a GraphQL response that must have succeeded.
pub async fn gql_ok(app: &Router, query: &str, variables: Value) -> Value {
    let json = gql(app, query, variables).await;
    assert!(json.get("errors").is_none(), "GraphQL errors: {json}");
    json["data"].clone()
}

/// The `extensions.code` of the first error of a GraphQL response, which must
/// have failed.
pub fn gql_error_code(response: &Value) -> String {
    response["errors"][0]["extensions"]["code"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a GraphQL error: {response}"))
        .to_string()
}

/// A new tree named `name`; its id.
pub async fn new_tree(app: &Router, name: &str) -> String {
    ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": name })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A new person of unknown sex in `tree_id`; its id.
pub async fn new_person(app: &Router, tree_id: &str) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "unknown" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

// ── Generated trees ─────────────────────────────────────────────────────

const SURNAMES: [&str; 7] = [
    "Ashdown",
    "Birchley",
    "Coldwell",
    "Dunmore",
    "Elmsworth",
    "Fernhill",
    "Greywood",
];
const MEN: [&str; 4] = ["Arthur", "Bernard", "Cyril", "Dorian"];
const WOMEN: [&str; 4] = ["Alice", "Beatrice", "Clara", "Delia"];
const PLACES: [&str; 5] = [
    "Northfield",
    "Southmere",
    "Eastbrook",
    "Westford",
    "Millbury",
];

/// One person of a family block: (sex, surname offset, generation).
const BLOCK: [(char, usize, i32); 10] = [
    ('M', 0, 0), // 0 the block's root
    ('M', 0, 1), // 1 father
    ('F', 1, 1), // 2 mother
    ('M', 0, 2), // 3 paternal grandfather
    ('F', 2, 2), // 4 paternal grandmother
    ('M', 1, 2), // 5 maternal grandfather
    ('F', 3, 2), // 6 maternal grandmother
    ('F', 0, 0), // 7 sister
    ('M', 0, 0), // 8 brother
    ('F', 4, 0), // 9 wife
];

/// The block's families: (husband, wife, children).
const FAMILIES: [(usize, usize, &[usize]); 4] =
    [(1, 2, &[0, 7, 8]), (3, 4, &[1]), (5, 6, &[2]), (0, 9, &[])];

/// A fictitious GEDCOM of `blocks` unrelated three-generation families, ten
/// persons each, drawing on small pools of names and places so that a larger
/// tree repeats them as a real one does. Block 0's root is named "Anchor"
/// so the tests can find it.
pub fn family_blocks_gedcom(blocks: usize) -> String {
    let mut out =
        String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n");
    for b in 0..blocks {
        out += &format!("0 @S{b}@ SOUR\n1 TITL Parish register {b}\n");
        for (k, &(sex, surname, generation)) in BLOCK.iter().enumerate() {
            let given = match (b, k, sex) {
                (0, 0, _) => "Anchor",
                (_, _, 'M') => MEN[(b + k) % MEN.len()],
                _ => WOMEN[(b + k) % WOMEN.len()],
            };
            let surname = SURNAMES[(b + surname) % SURNAMES.len()];
            // Four blocks a year: a larger tree reaches further back, as a
            // real one grown by its research does.
            let year = 1900 - 30 * generation - (b / 4) as i32;
            let place = PLACES[(b + k) % PLACES.len()];
            out += &format!(
                "0 @I{b}_{k}@ INDI\n1 NAME {given} /{surname}/\n1 SEX {sex}\n\
                 1 BIRT\n2 DATE {year}\n2 PLAC {place}, Shire\n2 SOUR @S{b}@\n"
            );
            if generation == 1 {
                out += "1 OCCU Weaver\n";
            }
            if generation == 2 {
                out += &format!("1 DEAT\n2 DATE {}\n", year + 60);
            }
            for (f, &(husband, wife, children)) in FAMILIES.iter().enumerate() {
                if husband == k || wife == k {
                    out += &format!("1 FAMS @F{b}_{f}@\n");
                }
                if children.contains(&k) {
                    out += &format!("1 FAMC @F{b}_{f}@\n");
                }
            }
        }
        for (f, &(husband, wife, children)) in FAMILIES.iter().enumerate() {
            out += &format!("0 @F{b}_{f}@ FAM\n1 HUSB @I{b}_{husband}@\n1 WIFE @I{b}_{wife}@\n");
            for child in children {
                out += &format!("1 CHIL @I{b}_{child}@\n");
            }
            out += &format!("1 MARR\n2 DATE 18{:02}\n2 PLAC Northfield, Shire\n", 50 + f);
        }
    }
    out + "0 TRLR\n"
}

/// A tree of [`family_blocks_gedcom`]`(blocks)`, with block 0's root as its SOSA root,
/// in the [`app_on`] router over `db`; the tree's id and that root's.
pub async fn family_blocks_tree(
    app: &Router,
    db: &DatabaseConnection,
    blocks: usize,
) -> (String, String) {
    let tree = ok(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Scaling" })),
    )
    .await;
    let tree_id = tree["id"].as_str().unwrap().to_owned();
    import_gedcom(app, db, &tree_id, &family_blocks_gedcom(blocks)).await;
    let profiles = ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles"),
        None,
    )
    .await;
    let anchor = profiles
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["primary_name"]["given_names"] == "Anchor")
        .expect("block 0's root")["person_id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(
        app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(serde_json::json!({ "sosa_root_person_id": anchor })),
    )
    .await;
    (tree_id, anchor)
}
