//! Middleware rejecting requests scoped to a tree that no longer exists.
//!
//! Tree deletion is asynchronous: the request flags `deleted_at` and a
//! background worker removes the rows a few seconds later (see
//! [`crate::service::purge`]). Without this guard the tree's children stay
//! readable for that window — `GET /api/v1/trees/{id}/persons` would still
//! answer for a tree the client was just told was deleted.
//!
//! It also closes a gap that predates that change: the tree-scoped handlers
//! take `tree_id` from the path but most never check it, so a request naming a
//! tree that never existed got a `200` with empty results rather than a `404`.
//!
//! One indexed primary-key lookup per tree-scoped request, applied in one
//! place instead of repeated across fifteen handlers.
//!
//! Two requests skip it: polling the status of a job this process knows in
//! memory — it runs it, or holds a cancellation for it — and cancelling such
//! a job. The job holds the SQLite writer for its whole write transaction,
//! which on an in-memory database is the only connection, readers included;
//! the status and cancel handlers answer from memory precisely so that they
//! do not wait for it, and a lookup here would make them wait anyway.
//! Elsewhere the lookup goes to the read pool. That answer carries only the
//! job's own phase and counters; once the job ends, the poll goes through
//! the lookup again, and a tree deleted meanwhile answers `404`.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use oxidgene_db::repo::BackgroundJobKind;
use uuid::Uuid;

use crate::service::background_job::live_state;

use super::error::ApiError;
use super::state::AppState;

/// Reject the request when its path names a tree that is missing or deleted.
///
/// Runs inside the `/api/v1/trees` nest, so the path it sees is either `/`
/// (list and create, which name no tree) or `/{tree_id}/...`. Anything whose
/// first segment is not a UUID is passed through untouched — routing will
/// produce its own `404`.
pub async fn require_live_tree(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let mut segments = req.uri().path().trim_start_matches('/').split('/');
    let first_segment = segments.next().unwrap_or_default();

    if let Ok(tree_id) = Uuid::parse_str(first_segment) {
        if is_live_job_request(req.method(), tree_id, segments) {
            return next.run(req).await;
        }
        // `get` already filters on `deleted_at`, so a soft-deleted tree is a
        // NotFound here just as it is in the tree list. Reusing `ApiError`
        // keeps the body identical to the one the handlers produce.
        match crate::service::scope::require_live_tree(&state.reader, tree_id).await {
            Ok(()) => {}
            Err(e @ oxidgene_core::OxidGeneError::NotFound { .. }) => {
                return ApiError(e).into_response();
            }
            // A database failure is not the client's fault — let the handler
            // run and report the real error rather than masking it as a 404.
            Err(_) => {}
        }
    }

    next.run(req).await
}

/// Whether `method` on the path after the tree id, `rest`, reads the status
/// of a job of that tree this process knows in memory, `GET
/// import-jobs/{id}` or `GET export-jobs/{id}`, or cancels it, the same
/// paths followed by `/cancel` with `POST`.
fn is_live_job_request<'a>(
    method: &Method,
    tree_id: Uuid,
    mut rest: impl Iterator<Item = &'a str>,
) -> bool {
    let kind = match rest.next() {
        Some("import-jobs") => BackgroundJobKind::Import,
        Some("export-jobs") => BackgroundJobKind::Export,
        _ => return false,
    };
    let Some(Ok(job_id)) = rest.next().map(Uuid::parse_str) else {
        return false;
    };
    let request = match (method, rest.next()) {
        (&Method::GET, None) => true,
        (&Method::POST, Some("cancel")) => rest.next().is_none(),
        _ => false,
    };
    request && live_state(tree_id, job_id, kind).is_some()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use oxidgene_db::repo::{BackgroundJobKind, TreeRepo, run_migrations};
    use sea_orm::{ConnectOptions, Database, TransactionTrait};
    use tower::ServiceExt;
    use uuid::Uuid;

    use crate::service::background_job::LiveJobGuard;
    use crate::{AppState, build_router};

    #[tokio::test]
    async fn a_running_job_status_does_not_wait_for_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (segment, kind) in [
            ("import-jobs", BackgroundJobKind::Import),
            ("export-jobs", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let _live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            // As an import holds it for its whole write transaction.
            let held = db.begin().await.expect("holds the only connection");
            let request = Request::get(format!("/api/v1/trees/{tree_id}/{segment}/{job_id}"))
                .body(Body::empty())
                .unwrap();
            let response =
                tokio::time::timeout(Duration::from_secs(2), app.clone().oneshot(request))
                    .await
                    .expect("the status poll does not wait for the connection")
                    .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{segment}");
            held.rollback().await.unwrap();
        }
    }

    /// The JSON body of `request` sent to `app`, which must answer `200`
    /// within `limit` although the running job holds the only connection.
    async fn answered_within(
        app: &axum::Router,
        request: Request<Body>,
        limit: Duration,
    ) -> serde_json::Value {
        use http_body_util::BodyExt as _;
        let response = tokio::time::timeout(limit, app.clone().oneshot(request))
            .await
            .expect("the request does not wait for the connection")
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).expect("a JSON body")
    }

    #[tokio::test]
    async fn a_running_job_is_cancelled_and_waited_on_without_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (segment, kind) in [
            ("import-jobs", BackgroundJobKind::Import),
            ("export-jobs", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            // As an import holds it for its whole write transaction.
            let held = db.begin().await.expect("holds the only connection");
            let job = format!("/api/v1/trees/{tree_id}/{segment}/{job_id}");

            let cancelled = answered_within(
                &app,
                Request::post(format!("{job}/cancel"))
                    .body(Body::empty())
                    .unwrap(),
                Duration::from_secs(2),
            )
            .await;
            assert_eq!(cancelled["phase"], "cancelling", "{segment}");
            assert!(live_job.cancel_token().is_cancelled());
            // A wait for a change reads memory too, then answers when its
            // time runs out.
            let version = cancelled["version"].as_str().unwrap();
            let waited = answered_within(
                &app,
                Request::get(format!("{job}?since={version}&wait=1"))
                    .body(Body::empty())
                    .unwrap(),
                Duration::from_secs(4),
            )
            .await;
            assert_eq!(waited["version"], cancelled["version"]);
            held.rollback().await.unwrap();
        }
    }

    #[cfg(feature = "graphql")]
    #[tokio::test]
    async fn a_running_job_is_cancelled_over_graphql_without_the_database() {
        let mut options = ConnectOptions::new("sqlite::memory:");
        options.max_connections(1);
        let db = Database::connect(options).await.expect("connects");
        run_migrations(&db).await.expect("migrates");
        let tree_id = Uuid::now_v7();
        TreeRepo::create(&db, tree_id, "Guard Fixture".into(), None)
            .await
            .expect("creates tree");
        let media_root = tempfile::tempdir().expect("creates media root");
        let app = build_router(AppState::new(db.clone(), media_root.path()));

        for (field, kind) in [
            ("cancelImportJob", BackgroundJobKind::Import),
            ("cancelExportJob", BackgroundJobKind::Export),
        ] {
            let job_id = Uuid::now_v7();
            let live_job = LiveJobGuard::for_test(job_id, tree_id, kind);
            let held = db.begin().await.expect("holds the only connection");
            let query = format!(
                r#"mutation {{ {field}(treeId: "{tree_id}", jobId: "{job_id}") {{ phase }} }}"#
            );
            let response = answered_within(
                &app,
                Request::post("/graphql")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "query": query }).to_string(),
                    ))
                    .unwrap(),
                Duration::from_secs(2),
            )
            .await;
            assert_eq!(response["data"][field]["phase"], "cancelling", "{response}");
            assert!(live_job.cancel_token().is_cancelled());
            held.rollback().await.unwrap();
        }
    }
}
