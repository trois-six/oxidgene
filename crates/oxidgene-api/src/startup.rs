//! Starting the backend: the steps the web server, the background worker and
//! the desktop app's embedded server share.
//!
//! At startup there is nobody to hand an error back to, so each step either
//! succeeds or logs the failure under a stable `error` code and ends the
//! process with status 1.

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use oxidgene_db::repo::{BackgroundJobRepo, connect, run_migrations};
use oxidgene_db::sea_orm::DatabaseConnection;
use tokio::task::JoinHandle;
use tracing::error;

use crate::AppState;
use crate::service::background_job::BackgroundJobWorker;

/// The value of `result`, or the end of the process: `message` is logged
/// under the `error` code `code` and the process exits with status 1.
pub fn or_exit<T, E>(result: Result<T, E>, code: &'static str, message: &'static str) -> T {
    result.unwrap_or_else(|_| {
        error!(error = code, "{message}");
        std::process::exit(1)
    })
}

/// The static reference tables, loading on a blocking thread.
///
/// Decompressing and indexing them takes tens of milliseconds. Done lazily it
/// lands on whichever request serves the first tooltip lookup and blocks it;
/// started first, it overlaps with connecting to the database.
pub struct ReferenceWarmup(JoinHandle<()>);

impl ReferenceWarmup {
    /// Start loading the tables.
    pub fn start() -> Self {
        Self(tokio::task::spawn_blocking(crate::reference::preheat))
    }

    /// Wait until the tables are loaded.
    pub async fn finish(self) {
        or_exit(
            self.0.await,
            "reference_warmup",
            "Failed to load static reference tables",
        );
    }
}

/// Connect to the database and bring its schema up to date.
pub async fn connect_and_migrate(database_url: &str) -> DatabaseConnection {
    let db = or_exit(
        connect(database_url).await,
        "database_connection",
        "Failed to connect to database",
    );
    or_exit(
        run_migrations(&db).await,
        "database_migration",
        "Failed to run migrations",
    );
    db
}

/// Run the background job worker inside this process.
///
/// With `requeue_running`, the jobs a previous run left marked as running go
/// back to the queue first: when this process is the only worker, nothing
/// else is still running them.
pub async fn spawn_background_worker(state: &AppState, requeue_running: bool, worker_id: &str) {
    if requeue_running {
        or_exit(
            BackgroundJobRepo::requeue_running(&state.db).await,
            "background_job_recovery",
            "Failed to recover background jobs",
        );
    }
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        Arc::clone(&state.profiles),
        Arc::clone(&state.media),
        worker_id,
    );
    tokio::spawn(worker.run());
}

/// `api` behind a `GET /healthz` answering `200 OK` with `{"status":"ok"}`.
pub fn with_health_check(api: Router) -> Router {
    Router::new().route("/healthz", get(healthz)).merge(api)
}

async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
