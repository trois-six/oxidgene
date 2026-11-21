//! What every write shares, whichever surface asked: a transaction around
//! the write and the projection refresh it triggers, and the check that each
//! record it names belongs to the tree.

use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{TreeRepo, db_err};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Statement};
use uuid::Uuid;

#[derive(Clone, Copy)]
pub(crate) enum TreeResource {
    Person,
    Family,
    Event,
    EventWitness,
    Place,
    Source,
    Repository,
    Citation,
    Note,
    Media,
    MediaLink,
    Vignette,
}

impl TreeResource {
    /// The table of the resource: each of them names its tree in its own
    /// `tree_id`.
    fn table(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Family => "family",
            Self::Event => "event",
            Self::EventWitness => "event_witness",
            Self::Place => "place",
            Self::Source => "source",
            Self::Repository => "repository",
            Self::Citation => "citation",
            Self::Note => "note",
            Self::Media => "media",
            Self::MediaLink => "media_link",
            Self::Vignette => "vignette",
        }
    }

    fn entity(self) -> &'static str {
        match self {
            Self::Person => "Person",
            Self::Family => "Family",
            Self::Event => "Event",
            Self::EventWitness => "EventWitness",
            Self::Place => "Place",
            Self::Source => "Source",
            Self::Repository => "Repository",
            Self::Citation => "Citation",
            Self::Note => "Note",
            Self::Media => "Media",
            Self::MediaLink => "MediaLink",
            Self::Vignette => "Vignette",
        }
    }
}

/// Fail with `NotFound` unless tree `tree_id` exists and is not deleted.
///
/// A deleted tree keeps its rows until the background purge removes them, so
/// every tree-scoped operation checks this first: REST in its tree guard,
/// GraphQL in each resolver.
pub async fn require_live_tree(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    TreeRepo::get(db, tree_id).await.map(|_| ())
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
    let sql = format!(
        "SELECT 1 AS present FROM {} WHERE id = {id_param} AND tree_id = {tree_param} LIMIT 1",
        resource.table()
    );
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

/// How a service begins the transaction of a write: `begin_tree_write` for
/// a write to a tree, `begin_write` for one that belongs to none
/// (docs/architecture.md §4, Write transactions). It spans the mutation and
/// the projection refresh it triggers.
///
/// The refresh reads the *post-mutation* state (family links, names) to build
/// the projections, so it has to see the write — which means both must run on
/// the same connection, inside one transaction. Committing together is what
/// makes a projection impossible to observe out of step with its data.
///
/// A dropped transaction rolls back, so any `?` in the handler undoes the
/// mutation and the refresh as a unit.
pub use oxidgene_db::repo::{begin_tree_write, begin_write};

/// Commit a transaction opened with [`begin_tree_write`] or [`begin_write`].
pub async fn commit_tx(txn: DatabaseTransaction) -> Result<(), OxidGeneError> {
    txn.commit().await.map_err(db_err)
}
