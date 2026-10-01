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

use oxidgene_db::repo::{BackgroundJobRepo, TreeRepo};
use sea_orm::DatabaseConnection;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::media::MediaStore;
use crate::profile::ProfileService;

/// Handle used by request handlers to hand a soft-deleted tree to the worker.
///
/// Cloning is cheap and every clone feeds the same single worker task, so two
/// concurrent deletes can never purge the same tree at once.
#[derive(Debug, Clone)]
pub struct PurgeQueue {
    tx: mpsc::UnboundedSender<Uuid>,
}

impl PurgeQueue {
    /// Ask for `tree_id` to be purged. Returns immediately.
    ///
    /// A failure to enqueue is not an error for the caller: the tree stays
    /// soft-deleted and therefore invisible, and the next startup sweep will
    /// purge it.
    pub fn enqueue(&self, tree_id: Uuid) {
        if self.tx.send(tree_id).is_err() {
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
    let (tx, mut rx) = mpsc::unbounded_channel::<Uuid>();

    tokio::spawn(async move {
        match TreeRepo::list_purgeable(&db).await {
            Ok(ids) if !ids.is_empty() => {
                info!(count = ids.len(), "resuming purge of soft-deleted trees");
                for id in ids {
                    purge_tree(&db, &profiles, &*media, id).await;
                }
            }
            Ok(_) => {}
            Err(_) => error!(
                error = "purgeable_tree_listing",
                "could not list soft-deleted trees; skipping startup sweep"
            ),
        }

        while let Some(tree_id) = rx.recv().await {
            purge_tree(&db, &profiles, &*media, tree_id).await;
        }
    });

    PurgeQueue { tx }
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
