//! Shared application state for Axum handlers.

use oxidgene_core::error::OxidGeneError;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait};
use std::sync::Arc;

use crate::profile::ProfileService;

/// Shared state available to all Axum handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// Denormalized person projections, search and pedigree assembly.
    pub profiles: Arc<ProfileService>,
}

impl AppState {
    /// Create a new `AppState`.
    ///
    /// There is no cache backend to select any more: projections live in the
    /// `person_denorm` table of the same database, so desktop (SQLite) and
    /// web (PostgreSQL) run the identical code path.
    pub fn new(db: DatabaseConnection) -> Self {
        let profiles = Arc::new(ProfileService::new(db.clone()));
        Self { db, profiles }
    }

    /// Create a new `AppState` with an explicit profile service (for testing).
    pub fn with_profiles(db: DatabaseConnection, profiles: Arc<ProfileService>) -> Self {
        Self { db, profiles }
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
