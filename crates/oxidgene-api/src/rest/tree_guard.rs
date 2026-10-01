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
//! One request skips it: polling the status of a job this process is running.
//! The job holds the SQLite writer for its whole write transaction, which on
//! an in-memory database is the only connection, readers included; the
//! status handler answers from the job's in-memory progress precisely so
//! that the poll does not wait for it, and a lookup here would make it wait
//! anyway. Elsewhere the lookup goes to the read pool. That answer carries only the job's
//! own phase and counters; once the job ends, the poll goes through the
//! lookup again, and a tree deleted meanwhile answers `404`.

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use oxidgene_db::repo::BackgroundJobKind;
use uuid::Uuid;

use crate::service::background_job::live_job_progress;

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
        if req.method() == Method::GET && is_running_job_status(tree_id, segments) {
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

/// Whether the path after the tree id, `rest`, is the status of a job of
/// that tree this process is running: `import-jobs/{id}` or
/// `export-jobs/{id}`, with nothing after the id.
fn is_running_job_status<'a>(tree_id: Uuid, mut rest: impl Iterator<Item = &'a str>) -> bool {
    let kind = match rest.next() {
        Some("import-jobs") => BackgroundJobKind::Import,
        Some("export-jobs") => BackgroundJobKind::Export,
        _ => return false,
    };
    let Some(Ok(job_id)) = rest.next().map(Uuid::parse_str) else {
        return false;
    };
    rest.next().is_none() && live_job_progress(tree_id, job_id, kind).is_some()
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
}
