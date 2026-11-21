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
use crate::tree_cache::CacheSettings;
use crate::workdir::WorkDir;

#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalFileAccess(pub(crate) bool);

impl LocalFileAccess {
    /// Whether the backend reads paths its client names: the desktop's.
    pub(crate) fn is_granted(self) -> bool {
        self.0
    }

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
    /// How much the backend takes in: the server's bounds unless the
    /// desktop lifts them.
    pub limits: crate::limits::IntakeLimits,
    /// How long the embedded worker keeps access events
    /// (docs/authorization.md §9.2).
    pub access_log_retention: std::time::Duration,
    /// Whether the database file can be used: always, but for the desktop's
    /// file once its start or its daily check said otherwise
    /// (docs/architecture.md §8.4).
    pub database_health: crate::backup::DatabaseHealth,
    /// The web build's authentication: the `none` mode until the server
    /// configures it.
    #[cfg(feature = "auth")]
    pub auth: Arc<crate::auth::Auth>,
}

impl AppState {
    /// What `viewer` sees of the records of tree `tree_id`, read through
    /// the read pool (docs/privacy.md §3.2).
    pub async fn records(
        &self,
        viewer: &oxidgene_core::access::ViewerContext,
        tree_id: uuid::Uuid,
    ) -> Result<crate::authz::records::Records, OxidGeneError> {
        crate::authz::records::Records::of(&self.reader, &self.profiles, viewer, tree_id).await
    }

    /// Create a new `AppState` storing media under `media_root`.
    ///
    /// Projections live in the `person_denorm` table of the same database,
    /// so desktop (SQLite) and web (PostgreSQL) run the identical code path;
    /// the in-memory tree cache has the web's default settings until
    /// [`Self::with_cache`] changes them.
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
            limits: crate::limits::IntakeLimits::default(),
            access_log_retention: crate::service::background_job::DEFAULT_ACCESS_LOG_RETENTION,
            database_health: crate::backup::DatabaseHealth::default(),
            #[cfg(feature = "auth")]
            auth: Arc::new(crate::auth::Auth::none()),
        }
    }

    /// Authenticate requests as `auth` says (docs/authentication.md).
    #[cfg(feature = "auth")]
    #[must_use]
    pub fn with_auth(mut self, auth: Arc<crate::auth::Auth>) -> Self {
        self.auth = auth;
        self
    }

    /// Take in files, archives and media within `limits` rather than the
    /// standalone server's defaults.
    #[must_use]
    pub fn with_intake_limits(mut self, limits: crate::limits::IntakeLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Answer for the database file as `health` says: refuse what a file
    /// that failed its check, or holds another schema, cannot serve.
    #[must_use]
    pub fn with_database_health(mut self, health: crate::backup::DatabaseHealth) -> Self {
        self.database_health = health;
        self
    }

    /// Keep access events for `retention` (`OXIDGENE_ACCESS_LOG_RETENTION`).
    #[must_use]
    pub fn with_access_log_retention(mut self, retention: std::time::Duration) -> Self {
        self.access_log_retention = retention;
        self
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

    /// Keep in memory what `settings` allow between requests: one tree at a
    /// time on the desktop, trees within a budget on the web (see
    /// [`crate::tree_cache`]); the place dictionary's index, which the
    /// process shares, is released after the same idle delay.
    #[must_use]
    pub fn with_cache(self, settings: CacheSettings) -> Self {
        self.profiles.cache().configure(settings);
        crate::reference::release_index_after(settings.idle);
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
    /// `crate::service::session_media`).
    pub fn with_local_file_access(mut self) -> Self {
        self.local_file_access = LocalFileAccess(true);
        crate::service::session_media::start_janitor();
        self
    }
}
