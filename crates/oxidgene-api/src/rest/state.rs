//! Shared application state for Axum handlers.

use oxidgene_core::error::OxidGeneError;
use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use std::sync::Arc;

use crate::media::{FsStore, MediaStore};
use crate::profile::ProfileService;
use crate::service::purge::{self, PurgeQueue};

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalFileAccess(pub(crate) bool);

impl LocalFileAccess {
    pub(crate) fn require(self) -> Result<(), OxidGeneError> {
        if self.0 {
            Ok(())
        } else {
            Err(OxidGeneError::Validation(
                "local filesystem access is unavailable".to_string(),
            ))
        }
    }
}

/// Shared state available to all Axum handlers.
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    /// Denormalized person projections, search and pedigree assembly.
    pub profiles: Arc<ProfileService>,
    /// Hands soft-deleted trees to the background purge worker.
    pub purge: PurgeQueue,
    /// Where uploaded files and their thumbnails live.
    pub media: Arc<dyn MediaStore>,
    pub(crate) local_file_access: LocalFileAccess,
}

impl AppState {
    /// Create a new `AppState` storing media under `media_root`.
    ///
    /// There is no cache backend to select any more: projections live in the
    /// `person_denorm` table of the same database, so desktop (SQLite) and
    /// web (PostgreSQL) run the identical code path.
    ///
    /// Spawns the purge worker, which also sweeps trees left soft-deleted by a
    /// previous run — so this must be called from within a Tokio runtime.
    pub fn new(db: DatabaseConnection, media_root: impl Into<PathBuf>) -> Self {
        Self::with_media_store(db, Arc::new(FsStore::new(media_root)))
    }

    /// Create a new `AppState` using an explicitly selected media backend.
    pub fn with_media_store(db: DatabaseConnection, media: Arc<dyn MediaStore>) -> Self {
        let profiles = Arc::new(ProfileService::new(db.clone()));
        Self::with_parts(db, profiles, media)
    }

    /// Create a new `AppState` with explicit collaborators (for testing).
    pub fn with_parts(
        db: DatabaseConnection,
        profiles: Arc<ProfileService>,
        media: Arc<dyn MediaStore>,
    ) -> Self {
        let purge = purge::spawn_worker(db.clone(), Arc::clone(&profiles), Arc::clone(&media));
        Self {
            db,
            profiles,
            purge,
            media,
            local_file_access: LocalFileAccess(false),
        }
    }

    /// Allow handlers to consume filesystem paths supplied by the local desktop UI.
    ///
    /// That is also what lets this backend stage a Geneanet session's media
    /// in temporary files, so it starts what bounds their life (see
    /// [`crate::service::session_media`]).
    pub fn with_local_file_access(mut self) -> Self {
        self.local_file_access = LocalFileAccess(true);
        crate::service::session_media::start_janitor();
        self
    }
}
