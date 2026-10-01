//! What every write shares, whichever surface asked: a transaction around
//! the write and the projection refresh it triggers, and the check that each
//! record it names belongs to the tree.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::db_err;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbBackend, Statement,
    TransactionTrait,
};
use uuid::Uuid;

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
        .map_err(db_err)?;
    if found.is_none() {
        return Err(OxidGeneError::NotFound {
            entity: resource.entity(),
            id,
        });
    }
    Ok(())
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
    db.begin().await.map_err(db_err)
}

/// Commit a transaction opened with [`begin_tx`].
pub async fn commit_tx(txn: DatabaseTransaction) -> Result<(), OxidGeneError> {
    txn.commit().await.map_err(db_err)
}
