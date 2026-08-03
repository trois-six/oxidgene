//! Shared application state for Axum handlers.

use oxidgene_core::error::OxidGeneError;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait};
use std::sync::Arc;

use crate::profile::ProfileService;
use crate::service::purge::{self, PurgeQueue};

/// Shared state available to all Axum handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// Denormalized person projections, search and pedigree assembly.
    pub profiles: Arc<ProfileService>,
    /// Hands soft-deleted trees to the background purge worker.
    pub purge: PurgeQueue,
}

impl AppState {
    /// Create a new `AppState`.
    ///
    /// There is no cache backend to select any more: projections live in the
    /// `person_denorm` table of the same database, so desktop (SQLite) and
    /// web (PostgreSQL) run the identical code path.
    ///
    /// Spawns the purge worker, which also sweeps trees left soft-deleted by a
    /// previous run — so this must be called from within a Tokio runtime.
    pub fn new(db: DatabaseConnection) -> Self {
        let profiles = Arc::new(ProfileService::new(db.clone()));
        Self::with_profiles(db, profiles)
    }

    /// Create a new `AppState` with an explicit profile service (for testing).
    pub fn with_profiles(db: DatabaseConnection, profiles: Arc<ProfileService>) -> Self {
        let purge = purge::spawn_worker(db.clone(), Arc::clone(&profiles));
        Self {
            db,
            profiles,
            purge,
        }
    }
}

/// Begin a transaction spanning a mutation and the projection refresh it
/// triggers.
///
/// The refresh reads the *post-mutation* state (family links, names) to build
/// the projections, so it has to see the write — which means both must run on
/// the same connection, inside one transaction. Committing together is what
/// makes a projection impossible to observe out of step with its data.
///
/// A dropped transaction rolls back, so any `?` in the handler undoes the
/// mutation and the refresh as a unit.
pub async fn begin_tx(db: &DatabaseConnection) -> Result<DatabaseTransaction, OxidGeneError> {
    db.begin()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))
}

/// Commit a transaction opened with [`begin_tx`].
pub async fn commit_tx(txn: DatabaseTransaction) -> Result<(), OxidGeneError> {
    txn.commit()
        .await
        .map_err(|e| OxidGeneError::Database(e.to_string()))
}
