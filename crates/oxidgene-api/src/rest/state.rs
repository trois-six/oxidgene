//! Shared application state for Axum handlers.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::Connections;
use sea_orm::DatabaseConnection;
use std::path::PathBuf;
use std::sync::Arc;

use crate::media::{FsStore, MediaStore};
use crate::profile::ProfileService;
use crate::service::archive::ArchivePortals;
use crate::service::purge::{self, PurgeQueue};
use crate::workdir::WorkDir;

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
    /// The single writer: every write, with its projection refresh, and
    /// whatever must see a write still in progress.
    pub db: DatabaseConnection,
    /// What handlers that only read use: the read pool of a file-backed
    /// SQLite database, so that they are answered while an import holds the
    /// writer; the writer itself elsewhere (see [`Connections`]).
    pub reader: DatabaseConnection,
    /// Denormalized person projections, search and pedigree assembly.
    pub profiles: Arc<ProfileService>,
    /// Hands soft-deleted trees to the background purge worker.
    pub purge: PurgeQueue,
    /// Where uploaded files and their thumbnails live.
    pub media: Arc<dyn MediaStore>,
    /// Where requests stage the files they hand to a job.
    pub work_dir: WorkDir,
    /// The archive resolution every request shares, with its session cache.
    pub archives: Arc<ArchivePortals>,
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
    pub fn new(db: impl Into<Connections>, media_root: impl Into<PathBuf>) -> Self {
        Self::with_media_store(db, Arc::new(FsStore::new(media_root)))
    }

    /// Create a new `AppState` using an explicitly selected media backend.
    pub fn with_media_store(db: impl Into<Connections>, media: Arc<dyn MediaStore>) -> Self {
        let connections = db.into();
        let profiles = Arc::new(ProfileService::new(connections.clone()));
        Self::with_parts(connections, profiles, media)
    }

    /// Create a new `AppState` with explicit collaborators (for testing).
    pub fn with_parts(
        db: impl Into<Connections>,
        profiles: Arc<ProfileService>,
        media: Arc<dyn MediaStore>,
    ) -> Self {
        let Connections { writer, reader } = db.into();
        // The purge deletes: it works on the writer.
        let purge = purge::spawn_worker(writer.clone(), Arc::clone(&profiles), Arc::clone(&media));
        Self {
            db: writer,
            reader,
            profiles,
            purge,
            media,
            work_dir: WorkDir::temporary(),
            archives: Arc::new(ArchivePortals::native()),
            local_file_access: LocalFileAccess(false),
        }
    }

    /// Resolve archive citations with `archives` rather than over the
    /// native transport: a recorded portal, in tests.
    #[must_use]
    pub fn with_archive_portals(mut self, archives: ArchivePortals) -> Self {
        self.archives = Arc::new(archives);
        self
    }

    /// Stage working files under `work_dir` rather than the system's
    /// temporary directory, which every binary does (see [`crate::workdir`]).
    #[must_use]
    pub fn with_work_dir(mut self, work_dir: WorkDir) -> Self {
        self.work_dir = work_dir;
        self
    }

    /// The writer and the reader, for the GraphQL schema.
    pub fn connections(&self) -> Connections {
        Connections {
            writer: self.db.clone(),
            reader: self.reader.clone(),
        }
    }

    /// Allow handlers to consume filesystem paths supplied by the local desktop UI.
    ///
    /// That is also what lets this backend stage a Geneanet session's media
    /// in working files, so it starts what bounds their life (see
    /// [`crate::service::session_media`]).
    pub fn with_local_file_access(mut self) -> Self {
        self.local_file_access = LocalFileAccess(true);
        crate::service::session_media::start_janitor();
        self
    }
}
