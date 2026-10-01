//! Tree writes and the tree list: the same steps whether REST or GraphQL
//! asked.
//!
//! A tree's settings name persons of their own — the SOSA root, the user's
//! own record — and those must belong to the tree: every write checks them in
//! the transaction that stores them.

use std::collections::HashMap;

use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::TreeDefaultPrivacy;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{Connection, Tree};
use oxidgene_db::repo::{BackgroundJobRepo, PaginationParams, TreeChanges, TreeRepo};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::gedcom;
use crate::service::history::{self, Change};
use crate::service::patch::double_option;
use crate::service::purge::PurgeQueue;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A tree as the tree list shows it, with the import running into it.
///
/// The import fields come from the job table, not the tree: they are true
/// while a job is queued or running and vanish once it completes or fails.
#[derive(Debug, Clone, Serialize)]
pub struct TreeListItem {
    #[serde(flatten)]
    pub tree: Tree,
    /// Whether an import is queued or running into the tree.
    pub import_in_progress: bool,
    /// That import's job, while it is.
    pub import_job_id: Option<Uuid>,
}

/// A tree to create.
#[derive(Debug, Deserialize)]
pub struct NewTree {
    pub name: String,
    pub description: Option<String>,
}

/// The settings a tree update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct TreePatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// The person SOSA numbers count from.
    #[serde(default, deserialize_with = "double_option")]
    pub sosa_root_person_id: Option<Option<Uuid>>,
    /// The person identifying the current user.
    #[serde(default, deserialize_with = "double_option")]
    pub self_person_id: Option<Option<Uuid>>,
    /// What `privacy: "default"` resolves to for everything in the tree.
    pub default_privacy: Option<TreeDefaultPrivacy>,
    /// Whether entry fields suggest values as the user types.
    pub entry_suggestions: Option<bool>,
}

/// Every tree, a page at a time, each with the import running into it.
pub async fn list_trees(
    db: &DatabaseConnection,
    params: &PaginationParams,
) -> Result<Connection<TreeListItem>, OxidGeneError> {
    let trees = TreeRepo::list(db, params).await?;
    let active: HashMap<Uuid, Uuid> = BackgroundJobRepo::active_imports(db)
        .await?
        .into_iter()
        .map(|job| (job.tree_id, job.id))
        .collect();
    Ok(trees.map(|tree| {
        let import_job_id = active.get(&tree.id).copied();
        TreeListItem {
            tree,
            import_in_progress: import_job_id.is_some(),
            import_job_id,
        }
    }))
}

/// The import queued or running into tree `tree_id`, if any.
pub async fn active_import(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<Option<Uuid>, OxidGeneError> {
    Ok(BackgroundJobRepo::active_imports(db)
        .await?
        .into_iter()
        .find(|job| job.tree_id == tree_id)
        .map(|job| job.id))
}

/// Create a tree. A blank name is refused.
pub async fn create_tree(db: &DatabaseConnection, new: NewTree) -> Result<Tree, OxidGeneError> {
    require_name(&new.name)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(id, AuditEntity::Tree, id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    let tree = TreeRepo::create(&txn, id, new.name, new.description).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(tree)
}

/// Update tree `tree_id`. A blank name is refused, and a SOSA root or own
/// record from another tree is not found.
pub async fn update_tree(
    db: &DatabaseConnection,
    tree_id: Uuid,
    patch: TreePatch,
) -> Result<Tree, OxidGeneError> {
    if let Some(name) = &patch.name {
        require_name(name)?;
    }
    let txn = begin_tx(db).await?;
    for person_id in [patch.sosa_root_person_id, patch.self_person_id]
        .into_iter()
        .flatten()
        .flatten()
    {
        require_tree_resource(&txn, tree_id, TreeResource::Person, person_id).await?;
    }
    let pending = Change::update(tree_id, AuditEntity::Tree, tree_id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    let tree = TreeRepo::update(
        &txn,
        tree_id,
        TreeChanges {
            name: patch.name,
            description: patch.description,
            sosa_root_person_id: patch.sosa_root_person_id,
            self_person_id: patch.self_person_id,
            default_privacy: patch.default_privacy,
            entry_suggestions: patch.entry_suggestions,
        },
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(tree)
}

/// Delete tree `tree_id`: flag it, and leave its rows to the purge worker.
///
/// Removing them here took seconds on a tree of any size — SQLite walks the
/// `ON DELETE CASCADE` graph one row at a time — so the request only flags
/// the tree and the worker does the rest. The purge is queued once the flag
/// is committed, so it can never outrun it.
pub async fn delete_tree(
    db: &DatabaseConnection,
    purge: &PurgeQueue,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    let pending = Change::delete(tree_id, AuditEntity::Tree, tree_id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    TreeRepo::soft_delete(&txn, tree_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    purge.enqueue(tree_id);
    Ok(())
}

/// Copy tree `source_tree_id` into a new tree named `name`, through a
/// lossless GEDCOM round trip.
///
/// The copy never enables the export's compatibility options: those trade
/// fidelity for third-party importers, which a copy inside OxidGene has no
/// use for. Both trees record the duplication in their audit log.
pub async fn duplicate_tree(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    source_tree_id: Uuid,
    name: String,
) -> Result<Tree, OxidGeneError> {
    require_name(&name)?;
    let source_name = TreeRepo::get(db, source_tree_id).await?.name;
    let export = gedcom::load_and_export(db, source_tree_id, false, false, false).await?;
    let new_tree_id = Uuid::now_v7();
    let tree = TreeRepo::create(db, new_tree_id, name, None).await?;
    let summary = gedcom::import_and_persist(db, new_tree_id, &export.gedcom).await?;
    profiles.rebuild_tree_full(db, new_tree_id).await?;
    history::record_import(
        db,
        new_tree_id,
        history::DUPLICATE_FORMAT,
        Some(source_name),
        summary.persons_count,
    )
    .await?;
    history::record_export(
        db,
        source_tree_id,
        history::DUPLICATE_FORMAT,
        Some(tree.name.clone()),
    )
    .await?;
    Ok(tree)
}

fn require_name(name: &str) -> Result<(), OxidGeneError> {
    if name.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "name must not be empty".to_string(),
        ));
    }
    Ok(())
}
