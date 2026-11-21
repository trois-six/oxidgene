//! Starting the backend: the steps the web server, the background worker and
//! the desktop app's embedded server share.
//!
//! At startup there is nobody to hand an error back to, so each step either
//! succeeds or logs the failure under a stable `error` code and ends the
//! process with status 1.

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use oxidgene_core::OxidGeneError;
pub use oxidgene_db::repo::DatabaseSettings;
use oxidgene_db::repo::{
    BackgroundJobRepo, Connections, MigrationError, SchemaState, apply_migrations, connect_with,
    run_migrations, schema_state,
};
use oxidgene_db::sea_orm::{DatabaseConnection, DbErr};
use tokio::task::{JoinError, JoinHandle};
use tracing::error;

use crate::AppState;
use crate::backup::{self, DatabaseHealth, SnapshotOutcome, SnapshotPolicy};
use crate::error_contract::error_kind;
use crate::service::background_job::BackgroundJobWorker;

/// The value of `result`, or the end of the process: `message` is logged
/// under the `error` code `code`, with the [`FailureKind`] of the error as
/// `error.kind`, and the process exits with status 1.
pub fn or_exit<T, E: FailureKind>(
    result: Result<T, E>,
    code: &'static str,
    message: &'static str,
) -> T {
    result.unwrap_or_else(|failure| {
        error!(
            error = code,
            error.kind = failure.failure_kind(),
            "{message}"
        );
        std::process::exit(1)
    })
}

/// The bounded category of a startup failure's cause, logged beside its
/// code: never the cause's message, which may hold a connection string, a
/// path or a credential.
pub trait FailureKind {
    fn failure_kind(&self) -> &'static str;
}

impl FailureKind for OxidGeneError {
    fn failure_kind(&self) -> &'static str {
        error_kind(self)
    }
}

impl FailureKind for DbErr {
    fn failure_kind(&self) -> &'static str {
        oxidgene_db::repo::db_error_kind(self).as_str()
    }
}

impl FailureKind for MigrationError {
    fn failure_kind(&self) -> &'static str {
        match self {
            Self::Database(error) => error.failure_kind(),
            Self::SchemaMismatch => "schema_mismatch",
        }
    }
}

impl FailureKind for std::io::Error {
    fn failure_kind(&self) -> &'static str {
        error_kind(&OxidGeneError::Io(self.kind().into()))
    }
}

impl FailureKind for JoinError {
    fn failure_kind(&self) -> &'static str {
        if self.is_panic() {
            "panic"
        } else {
            "cancelled"
        }
    }
}

impl FailureKind for axum::http::header::InvalidHeaderValue {
    fn failure_kind(&self) -> &'static str {
        "invalid_header_value"
    }
}

#[cfg(feature = "auth")]
impl FailureKind for crate::auth::StoreUnavailable {
    fn failure_kind(&self) -> &'static str {
        "unavailable"
    }
}

/// A configuration error: its text is the caller's to log, if anything.
impl FailureKind for String {
    fn failure_kind(&self) -> &'static str {
        "configuration"
    }
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

/// Connect to the database, within the limits `settings` gives, and bring
/// its schema up to date.
pub async fn connect_and_migrate(
    database_url: &str,
    settings: &DatabaseSettings,
) -> DatabaseConnection {
    let db = or_exit(
        connect_with(database_url, settings).await,
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

/// The database an API server runs on: connected within the limits
/// `settings` gives and migrated, then, for a SQLite file, with its read
/// pool (see [`Connections`]). The worker, which serves no request, connects
/// with [`connect_and_migrate`] alone.
pub async fn open_database(database_url: &str, settings: &DatabaseSettings) -> Connections {
    let db = connect_and_migrate(database_url, settings).await;
    or_exit(
        Connections::with_read_pool(db, database_url).await,
        "database_read_pool",
        "Failed to open the read-only database pool",
    )
}

/// The desktop's database, and whether its file can be used.
pub struct LocalDatabase {
    pub connections: Connections,
    /// What the router answers for: see [`DatabaseHealth`].
    pub health: DatabaseHealth,
}

/// Open the desktop's database file `database` (at `database_url`), which
/// is the only copy of its user's genealogy (docs/architecture.md §8.4):
///
/// - a file created with another schema than this build's is left as it
///   is, and every request is answered `503 database_incompatible`;
/// - a file with a migration to apply is copied first, into `snapshots`,
///   and one that fails its integrity check then is left as it is, every
///   request answered `503 database_damaged`;
/// - once the file is open, the day's snapshot is taken in the background.
///
/// Where the file is not opened the process still needs a database to
/// build its state on: an empty one in memory, which no request reaches.
pub async fn open_local_database(
    database_url: &str,
    database: &Path,
    snapshots: SnapshotPolicy,
) -> LocalDatabase {
    let settings = DatabaseSettings::default();
    let db = or_exit(
        connect_with(database_url, &settings).await,
        "database_connection",
        "Failed to connect to database",
    );
    let health = match local_schema(&db, database, &snapshots).await {
        Ok(state) => {
            or_exit(
                apply_migrations(&db, state).await,
                "database_migration",
                "Failed to run migrations",
            );
            let connections = or_exit(
                Connections::with_read_pool(db, database_url).await,
                "database_read_pool",
                "Failed to open the read-only database pool",
            );
            let health = DatabaseHealth::default();
            backup::spawn_daily_snapshot(database.to_owned(), snapshots, health.clone());
            return LocalDatabase {
                connections,
                health,
            };
        }
        Err(health) => health,
    };
    let _ = db.close().await;
    let empty = connect_and_migrate("sqlite::memory:", &settings).await;
    LocalDatabase {
        connections: empty.into(),
        health,
    }
}

/// What the desktop's file holds, once it is safe to migrate: its schema
/// is this build's, and a file about to be changed by a migration has been
/// copied. Otherwise, why it is not opened.
async fn local_schema(
    db: &DatabaseConnection,
    database: &Path,
    snapshots: &SnapshotPolicy,
) -> Result<SchemaState, DatabaseHealth> {
    let state = match schema_state(db).await {
        Ok(state) => state,
        Err(MigrationError::SchemaMismatch) => {
            error!(
                error = "database_incompatible",
                "The database file was created with another schema than this version's: \
                 it is left as it is, to be recreated and its genealogy reimported"
            );
            return Err(DatabaseHealth::incompatible());
        }
        Err(failure) => or_exit(
            Err(failure),
            "database_migration",
            "Failed to run migrations",
        ),
    };
    if state == (SchemaState::Existing { pending: true }) {
        let now = chrono::Utc::now();
        let outcome = backup::take_snapshot(database, snapshots, now).await;
        let damaged = matches!(outcome, Ok(SnapshotOutcome::Damaged(_)));
        backup::log_outcome(outcome, &DatabaseHealth::default());
        if damaged {
            return Err(DatabaseHealth::closed());
        }
    }
    Ok(state)
}

/// The background job worker running inside this process: what stops it
/// cleanly when the process does.
#[must_use = "a worker that is not stopped leaves its running job to its lease"]
pub struct EmbeddedWorker {
    stop: tokio::sync::oneshot::Sender<()>,
    stopped: JoinHandle<()>,
}

impl EmbeddedWorker {
    /// Stop the worker and wait for it: it claims no more job, and the one
    /// it runs ends within the grace period it was started with or goes
    /// back to the queue (see [`BackgroundJobWorker::run_until`]).
    pub async fn stop(self) {
        // An error means the worker is gone already.
        let _ = self.stop.send(());
        let _ = self.stopped.await;
    }
}

/// Run the background job worker inside this process, until the handle
/// returned stops it, giving a running job `shutdown_grace` to end then.
///
/// With `only_worker`, the jobs a previous run left marked as running are
/// taken over first: when this process is the only worker, nothing else is
/// still running them, and their worker disappeared.
pub async fn spawn_background_worker(
    state: &AppState,
    only_worker: bool,
    worker_id: &str,
    shutdown_grace: std::time::Duration,
) -> EmbeddedWorker {
    if only_worker {
        or_exit(
            BackgroundJobRepo::abandon_running(&state.db).await,
            "background_job_recovery",
            "Failed to recover background jobs",
        );
    }
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        Arc::clone(&state.profiles),
        Arc::clone(&state.media),
        worker_id,
    )
    .with_work_dir(state.work_dir.clone())
    .with_intake_limits(state.limits)
    .with_access_log_retention(state.access_log_retention);
    #[cfg(feature = "auth")]
    let worker = worker.with_mailing(state.auth.mailing().0);
    let (stop, asked) = tokio::sync::oneshot::channel();
    let stopped = tokio::spawn(worker.run_until(
        async move {
            // A handle dropped without `stop` leaves the worker running.
            if asked.await.is_err() {
                std::future::pending::<()>().await;
            }
        },
        shutdown_grace,
    ));
    EmbeddedWorker { stop, stopped }
}

/// The relay and the `oidc` deployment a process mails for, when
/// `OXIDGENE_SMTP_HOST` names a relay. The `none` mode mails nothing and
/// requires no relay: its settings are not read.
#[cfg(feature = "auth")]
pub struct MailSetup {
    pub smtp: crate::mail::SmtpConfig,
    /// Where links start and what unsubscribe tokens are signed with.
    pub context: crate::mail::MailContext,
}

#[cfg(feature = "auth")]
impl MailSetup {
    /// The mail of the deployment `auth` and `mail` describe, if any: a
    /// relay in `oidc` mode, with the public URL and the session keys. The
    /// sign-in settings are not read, so that a worker needs none of them.
    /// An invalid setting ends the process with its category and variable,
    /// never its value; a `danger-insecure` relay is warned about.
    pub fn resolve(
        auth: &crate::auth::AuthSettings,
        mail: &crate::mail::MailSettings,
    ) -> Option<Self> {
        if auth.auth_mode.trim() == "none" {
            return None;
        }
        let smtp = match mail.resolve() {
            Ok(smtp) => smtp?,
            Err(failure) => invalid(failure.category, failure.variable),
        };
        let (public_url, keys) = match auth.mail_signing() {
            Ok(signing) => signing,
            Err(failure) => invalid(failure.category, failure.variable),
        };
        if smtp.tls == crate::mail::SmtpTls::DangerInsecure {
            tracing::warn!(
                mail = "smtp_without_tls",
                "Mail goes to the SMTP relay without TLS: for a development relay only"
            );
        }
        Some(Self {
            smtp,
            context: crate::mail::context(&public_url, &keys),
        })
    }

    /// Start this process's mail sender over `db`.
    pub fn spawn_sender(&self, db: DatabaseConnection) {
        let sender = or_exit(
            crate::mail::sender(db, &self.smtp, self.context.clone()),
            "mail_config_invalid",
            "Failed to configure the SMTP relay",
        );
        tokio::spawn(sender.run());
    }
}

/// End the process over an invalid setting, named by its category and
/// variable, never its value.
#[cfg(feature = "auth")]
fn invalid(category: &'static str, variable: &'static str) -> ! {
    error!(error = category, variable, "Invalid mail configuration");
    std::process::exit(1)
}

#[cfg(feature = "auth")]
impl FailureKind for crate::mail::RelayFailure {
    fn failure_kind(&self) -> &'static str {
        self.category
    }
}

/// `api` behind a `GET /healthz` answering `200 OK` with `{"status":"ok"}`.
pub fn with_health_check(api: Router) -> Router {
    Router::new().route("/healthz", get(healthz)).merge(api)
}

async fn healthz() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_startup_failure_keeps_the_kind_of_its_cause() {
        let refused = DbErr::Conn(oxidgene_db::sea_orm::RuntimeErr::Internal(
            "refused".to_string(),
        ));
        assert_eq!(refused.failure_kind(), "connection");

        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(denied.failure_kind(), "io_permission_denied");

        let missing = OxidGeneError::Validation("absent".to_string());
        assert_eq!(missing.failure_kind(), "validation");
    }
}
