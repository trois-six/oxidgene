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
//! Ordering matters in one place only: `person_search_fts` is an FTS5 virtual
//! table on SQLite with no foreign keys, so the cascade cannot reach it and it
//! has to be cleared explicitly.

use std::sync::Arc;
use std::time::Instant;

use oxidgene_db::repo::TreeRepo;
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
            warn!(
                %tree_id,
                "purge worker stopped; tree stays soft-deleted until next start"
            );
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
            Err(e) => error!(%e, "could not list soft-deleted trees; skipping startup sweep"),
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
    let started = Instant::now();

    // Projections first: `person_search_fts` has no FK to cascade through.
    if let Err(e) = profiles.invalidate_tree(db, tree_id).await {
        error!(%tree_id, %e, "could not drop projections; retrying at next start");
        return;
    }

    // Files before rows. Media keys are scoped per tree, so this is one
    // directory removal and nothing outside the tree can reference what it
    // holds. Doing it first is what keeps a crash mid-purge recoverable: the
    // tree row survives, so the next sweep finds it again and finishes the
    // job. The reverse order would drop the row and strand the bytes with
    // nothing left pointing at them.
    if let Err(e) = media.delete_tree(tree_id).await {
        error!(%tree_id, %e, "could not remove media files; retrying at next start");
        return;
    }

    match TreeRepo::purge(db, tree_id).await {
        Ok(()) => info!(
            %tree_id,
            elapsed_ms = started.elapsed().as_millis(),
            "purged soft-deleted tree"
        ),
        Err(e) => error!(%tree_id, %e, "purge failed; retrying at next start"),
    }
}
