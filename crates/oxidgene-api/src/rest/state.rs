//! Shared application state for Axum handlers.

use oxidgene_core::error::OxidGeneError;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement,
    TransactionTrait,
};
use std::path::PathBuf;
use std::sync::Arc;

use std::collections::HashMap;
use uuid::Uuid;

use crate::media::{FsStore, MediaStore};
use crate::profile::ProfileService;
use crate::service::geneanet::ImportProgress;
use crate::service::purge::{self, PurgeQueue};

/// Runs currently reporting their Geneanet import progress.
pub type ImportProgressRegistry = Arc<std::sync::Mutex<HashMap<Uuid, Arc<ImportProgress>>>>;

#[derive(Clone, Copy)]
pub(crate) enum TreeResource {
    Person,
    PersonName,
    Family,
    FamilySpouse,
    FamilyChild,
    Event,
    EventWitness,
    Place,
    Source,
    Citation,
    Note,
    Media,
    MediaLink,
    Vignette,
}

impl TreeResource {
    fn query(self) -> (&'static str, &'static str) {
        match self {
            Self::Person => ("person r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::PersonName => (
                "person_name r JOIN person p ON p.id = r.person_id",
                "p.tree_id = {tree} AND p.deleted_at IS NULL",
            ),
            Self::Family => ("family r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::FamilySpouse => (
                "family_spouse r JOIN family f ON f.id = r.family_id",
                "f.tree_id = {tree} AND f.deleted_at IS NULL",
            ),
            Self::FamilyChild => (
                "family_child r JOIN family f ON f.id = r.family_id",
                "f.tree_id = {tree} AND f.deleted_at IS NULL",
            ),
            Self::Event => ("event r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::EventWitness => (
                "event_witness r JOIN event e ON e.id = r.event_id",
                "e.tree_id = {tree} AND e.deleted_at IS NULL",
            ),
            Self::Place => ("place r", "r.tree_id = {tree}"),
            Self::Source => ("source r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::Citation => (
                "citation r JOIN source s ON s.id = r.source_id",
                "s.tree_id = {tree} AND s.deleted_at IS NULL",
            ),
            Self::Note => ("note r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::Media => ("media r", "r.tree_id = {tree} AND r.deleted_at IS NULL"),
            Self::MediaLink => (
                "media_link r JOIN media m ON m.id = r.media_id",
                "m.tree_id = {tree} AND m.deleted_at IS NULL",
            ),
            Self::Vignette => (
                "vignette r JOIN media m ON m.id = r.media_id",
                "m.tree_id = {tree} AND m.deleted_at IS NULL",
            ),
        }
    }

    fn entity(self) -> &'static str {
        match self {
            Self::Person => "Person",
            Self::PersonName => "PersonName",
            Self::Family => "Family",
            Self::FamilySpouse => "FamilySpouse",
            Self::FamilyChild => "FamilyChild",
            Self::Event => "Event",
            Self::EventWitness => "EventWitness",
            Self::Place => "Place",
            Self::Source => "Source",
            Self::Citation => "Citation",
            Self::Note => "Note",
            Self::Media => "Media",
            Self::MediaLink => "MediaLink",
            Self::Vignette => "Vignette",
        }
    }
}

pub(crate) async fn require_tree_resource(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    resource: TreeResource,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let backend = db.get_database_backend();
    let (id_param, tree_param) = match backend {
        DbBackend::Postgres => ("$1", "$2"),
        _ => ("?", "?"),
    };
    let (from, condition) = resource.query();
    let condition = condition.replace("{tree}", tree_param);
    let sql =
        format!("SELECT 1 AS present FROM {from} WHERE r.id = {id_param} AND {condition} LIMIT 1");
    let found = db
        .query_one_raw(Statement::from_sql_and_values(
            backend,
            sql,
            vec![id.into(), tree_id.into()],
        ))
        .await
        .map_err(|error| OxidGeneError::Database(error.to_string()))?;
    if found.is_none() {
        return Err(OxidGeneError::NotFound {
            entity: resource.entity(),
            id,
        });
    }
    Ok(())
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
    /// How far each running Geneanet import has got.
    ///
    /// An import holds its request open for minutes, so it cannot report
    /// progress in its own response. The wizard names the run when it starts
    /// it and asks a second endpoint how it is going.
    pub imports: ImportProgressRegistry,
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
            imports: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
