//! Background purge of soft-deleted trees.
//!
//! Deleting a tree is a two-stage operation. The request handler only flips
//! `tree.deleted_at` and returns — a single-row UPDATE, instant whatever the
//! tree's size. The rows the tree owns are removed here, off the request path.
//!
//! There is no job table: `deleted_at IS NOT NULL` *is* the queue. It lives in
//! the database, so a purge cut short by a crash or a quit is simply found
//! again by the startup sweep. Purging is idempotent, so re-running one that
//! partially completed is harmless.
//!
//! Ordering matters in one place only: `person_search_fts` (an FTS5 virtual
//! table on SQLite) and its SQLite key table `person_search_key` have no
//! foreign keys, so the cascade cannot reach them and they have to be cleared
//! explicitly.

use std::sync::Arc;
use std::time::Instant;

use oxidgene_db::repo::{BackgroundJobRepo, PersonSearchRepo, TreeRepo};
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;
use tracing::{Instrument as _, error, info, warn};
use uuid::Uuid;

use crate::media::MediaStore;
use crate::profile::ProfileService;

/// Handle used by request handlers to hand a soft-deleted tree to the worker.
///
/// Cloning is cheap and every clone feeds the same single worker task, so two
/// concurrent deletes can never purge the same tree at once.
#[derive(Debug, Clone)]
pub struct PurgeQueue {
    tx: mpsc::UnboundedSender<PurgeRequest>,
}

/// A tree to purge, with the W3C trace context of the request that deleted
/// it: the purge outlives that request, and its `purge.tree` span continues
/// the request's trace rather than starting one of its own.
#[derive(Debug)]
struct PurgeRequest {
    tree_id: Uuid,
    /// `traceparent` and `tracestate`, when the build exports traces.
    #[cfg(feature = "telemetry-context")]
    trace: (Option<String>, Option<String>),
}

impl PurgeQueue {
    /// Ask for `tree_id` to be purged. Returns immediately.
    ///
    /// A failure to enqueue is not an error for the caller: the tree stays
    /// soft-deleted and therefore invisible, and the next startup sweep will
    /// purge it.
    pub fn enqueue(&self, tree_id: Uuid) {
        let request = PurgeRequest {
            tree_id,
            #[cfg(feature = "telemetry-context")]
            trace: oxidgene_observability::current_trace_context(),
        };
        if self.tx.send(request).is_err() {
            warn!("purge worker stopped; tree stays soft-deleted until next start");
        }
    }
}

/// Start the purge worker and return the handle used to feed it.
///
/// The worker first sweeps trees left over from a previous run, then serves
/// the queue. It owns its own connection, so a purge never borrows a request's
/// transaction and can outlive the request that triggered it.
pub fn spawn_worker(
    db: DatabaseConnection,
    profiles: Arc<ProfileService>,
    media: Arc<dyn MediaStore>,
) -> PurgeQueue {
    let (tx, mut rx) = mpsc::unbounded_channel::<PurgeRequest>();

    // A detached worker: the startup sweep is a root of its own, and each
    // queued purge continues the trace of the request that deleted the tree.
    tokio::spawn(async move {
        resume_purges(&db, &profiles, &*media)
            .instrument(tracing::info_span!(parent: None, "purge.sweep"))
            .await;

        while let Some(request) = rx.recv().await {
            purge_tree(&db, &profiles, &*media, request.tree_id)
                .instrument(request.span())
                .await;
        }
    });

    PurgeQueue { tx }
}

impl PurgeRequest {
    /// The `purge.tree` span: a child of the deleting request's span, or a
    /// root when that request carried no trace.
    fn span(&self) -> tracing::Span {
        let span = tracing::info_span!(parent: None, "purge.tree");
        #[cfg(feature = "telemetry-context")]
        oxidgene_observability::set_parent_from_trace_context(
            &span,
            self.trace.0.as_deref(),
            self.trace.1.as_deref(),
        );
        span
    }
}

/// Purge the trees a previous run left soft-deleted, each under a
/// `purge.tree` child of the sweep's span.
async fn resume_purges(db: &DatabaseConnection, profiles: &ProfileService, media: &dyn MediaStore) {
    let Ok(ids) = TreeRepo::list_purgeable(db).await else {
        log_unlisted_purges();
        return;
    };
    log_resumed_purges(ids.len());
    for id in ids {
        purge_tree(db, profiles, media, id)
            .instrument(swept_tree_span())
            .await;
    }
}

fn log_unlisted_purges() {
    error!(
        error = "purgeable_tree_listing",
        "could not list soft-deleted trees; skipping startup sweep"
    );
}

fn log_resumed_purges(count: usize) {
    if count != 0 {
        info!(count, "resuming purge of soft-deleted trees");
    }
}

/// A `purge.tree` span under the current one: the startup sweep's.
fn swept_tree_span() -> tracing::Span {
    tracing::info_span!("purge.tree")
}

/// Remove every row belonging to a soft-deleted tree.
///
/// Deliberately not wrapped in one transaction: the tree is already invisible,
/// nothing may observe the intermediate state, and a single transaction would
/// hold the SQLite write lock for the whole cascade. Each step commits on its
/// own, so an interrupted purge leaves less work for the next run instead of
/// rolling everything back.
///
/// Errors are logged, not propagated — there is no caller left to handle them,
/// and the tree stays flagged so the next sweep retries.
///
/// Runs under the `purge.tree` span its caller opens, the parent of the
/// cascade's database calls.
async fn purge_tree(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    media: &dyn MediaStore,
    tree_id: Uuid,
) {
    if let Err((code, message)) = purge_steps(db, profiles, media, tree_id).await {
        error!(error = code, "{message}");
    }
}

/// Runs the steps of [`purge_tree`] in order and logs how long they took,
/// stopping at the first that fails with its error code and log message.
async fn purge_steps(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    media: &dyn MediaStore,
    tree_id: Uuid,
) -> Result<(), (&'static str, &'static str)> {
    let started = Instant::now();

    // Projections first: the search table has no FK to cascade through.
    profiles.invalidate_tree(db, tree_id).await.map_err(|_| {
        (
            "projection_invalidation",
            "could not drop projections; retrying at next start",
        )
    })?;

    // Files before rows. Media keys are scoped per tree, so this is one
    // directory removal and nothing outside the tree can reference what it
    // holds. Doing it first is what keeps a crash mid-purge recoverable: the
    // tree row survives, so the next sweep finds it again and finishes the
    // job. The reverse order would drop the row and strand the bytes with
    // nothing left pointing at them.
    media.delete_tree(tree_id).await.map_err(|_| {
        (
            "media_deletion",
            "could not remove media files; retrying at next start",
        )
    })?;
    // Job objects live under `jobs/`, outside the tree's prefix: an export
    // artifact is a full copy of the tree. The job rows cascade with the
    // tree, so this is the last moment anything points at them.
    delete_job_objects(db, media, tree_id).await.map_err(|_| {
        (
            "job_object_deletion",
            "could not remove the tree's job files; retrying at next start",
        )
    })?;

    TreeRepo::purge(db, tree_id)
        .await
        .map_err(|_| ("tree_purge", "purge failed; retrying at next start"))?;
    erase_purged_content(db).await;
    info!(
        elapsed_ms = started.elapsed().as_millis(),
        "purged soft-deleted tree"
    );
    Ok(())
}

/// Remove the stored inputs and artifacts of every job of `tree_id`.
async fn delete_job_objects(
    db: &DatabaseConnection,
    media: &dyn MediaStore,
    tree_id: Uuid,
) -> Result<(), oxidgene_core::OxidGeneError> {
    for job_id in BackgroundJobRepo::ids_in_tree(db, tree_id).await? {
        media.delete_job(job_id).await?;
    }
    Ok(())
}

/// Leave nothing of the purged rows readable in the database file: the words
/// the full-text index still holds, then the freed pages and the log.
async fn erase_purged_content(db: &DatabaseConnection) {
    if PersonSearchRepo::merge_index(db).await.is_err() {
        warn!(
            error = "search_index_merge",
            "could not drop the purged tree's words from the search index"
        );
    }
    oxidgene_db::repo::erase_deleted_content(db).await;
}
