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

/// The API router over `db`.
///
/// Media lands in a throwaway directory: these tests never upload, but
/// `AppState` needs a root and it must not be the developer's.
pub fn app_on(db: DatabaseConnection) -> Router {
    build_router(AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-media"),
    ))
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
