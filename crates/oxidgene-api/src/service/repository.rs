//! Repository writes and reads, and a source's links to the repositories
//! holding it: the same steps whether REST or GraphQL asked.
//!
//! A repository is known by its name, so it may never be blank. Each write
//! records the change in the transaction that makes it: a repository is a
//! versioned record of its own, and a link is part of its source's state.

use oxidgene_core::enums::SourceMediaType;
use oxidgene_core::history::AuditEntity;
use std::collections::HashMap;

use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{Connection, Repository, Source, SourceRepository};
use oxidgene_db::repo::{
    PaginationParams, RepositoryFields, RepositoryPatch as RepoPatch, RepositoryRepo, SourceRepo,
    SourceRepositoryFields, SourceRepositoryPatch as LinkRepoPatch, SourceRepositoryRepo,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Deserialize;
use uuid::Uuid;

use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A repository to create.
#[derive(Debug, Deserialize)]
pub struct NewRepository {
    pub name: String,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
}

/// The fields a repository update changes: `None` keeps a field, and
/// `Some(None)` clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct RepositoryPatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub address: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub phone: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub website: Option<Option<String>>,
}

/// A source's link to a repository holding it, to create.
#[derive(Debug, Deserialize)]
pub struct NewSourceRepository {
    pub repository_id: Uuid,
    #[serde(default)]
    pub call_number: Option<String>,
    #[serde(default)]
    pub media_type: Option<SourceMediaType>,
    /// Where it goes among the source's links; after them when omitted.
    #[serde(default)]
    pub sort_order: Option<i32>,
}

/// The fields a link update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct SourceRepositoryPatch {
    /// Points the link at another repository.
    pub repository_id: Option<Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    pub call_number: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub media_type: Option<Option<SourceMediaType>>,
    pub sort_order: Option<i32>,
}

/// The repositories of `tree_id`, a page at a time.
pub async fn list_repositories(
    db: &DatabaseConnection,
    tree_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<Repository>, OxidGeneError> {
    RepositoryRepo::list(db, tree_id, params).await
}

/// Repository `id` of `tree_id`.
pub async fn get_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
) -> Result<Repository, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Repository, id).await?;
    RepositoryRepo::get(db, id).await
}

/// Create a repository in `tree_id`. A blank name is refused.
pub async fn create_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    new: NewRepository,
) -> Result<Repository, OxidGeneError> {
    let name = required_name(&new.name)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(tree_id, AuditEntity::Repository, id)
        .repository(id)
        .prepare(&txn)
        .await?;
    let repository = RepositoryRepo::create(
        &txn,
        id,
        tree_id,
        RepositoryFields {
            name,
            address: blank_to_none(new.address),
            phone: blank_to_none(new.phone),
            email: blank_to_none(new.email),
            website: blank_to_none(new.website),
        },
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(repository)
}

/// Update repository `id` of `tree_id`. A blank name is refused.
pub async fn update_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    patch: RepositoryPatch,
) -> Result<Repository, OxidGeneError> {
    let name = patch.name.as_deref().map(required_name).transpose()?;
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Repository, id).await?;
    let pending = Change::update(tree_id, AuditEntity::Repository, id)
        .repository(id)
        .prepare(&txn)
        .await?;
    let repository = RepositoryRepo::update(
        &txn,
        id,
        RepoPatch {
            name,
            address: patch.address.map(blank_to_none),
            phone: patch.phone.map(blank_to_none),
            email: patch.email.map(blank_to_none),
            website: patch.website.map(blank_to_none),
        },
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(repository)
}

/// Delete repository `id` of `tree_id` (a soft delete); whether it was
/// deleted.
///
/// With `only_if_unused` the delete is a cleanup: a repository still holding
/// a live source, or still the subject of a note, is kept, and `false` says
/// so.
pub async fn delete_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    only_if_unused: bool,
) -> Result<bool, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Repository, id).await?;
    if only_if_unused && RepositoryRepo::is_used(&txn, id).await? {
        commit_tx(txn).await?;
        return Ok(false);
    }
    let pending = Change::delete(tree_id, AuditEntity::Repository, id)
        .repository(id)
        .prepare(&txn)
        .await?;
    RepositoryRepo::delete(&txn, id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(true)
}

/// The repositories holding source `source_id` of `tree_id`, in order.
pub async fn list_source_repositories(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    source_id: Uuid,
) -> Result<Vec<SourceRepository>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Source, source_id).await?;
    SourceRepositoryRepo::list_by_source(db, source_id).await
}

/// The live sources repository `repository_id` of `tree_id` holds, one per
/// call number, each with its link.
pub async fn held_sources(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    repository_id: Uuid,
) -> Result<Vec<(SourceRepository, Source)>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Repository, repository_id).await?;
    let links = SourceRepositoryRepo::list_by_repository(db, repository_id).await?;
    let source_ids: Vec<Uuid> = links.iter().map(|l| l.source_id).collect();
    let sources: HashMap<Uuid, Source> = SourceRepo::get_many(db, tree_id, &source_ids)
        .await?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let mut held: Vec<(SourceRepository, Source)> = links
        .into_iter()
        .filter_map(|link| {
            let source = sources.get(&link.source_id)?.clone();
            Some((link, source))
        })
        .collect();
    held.sort_by_cached_key(|(link, source)| (source.title.to_lowercase(), link.sort_order));
    Ok(held)
}

/// Record that source `source_id` of `tree_id` is held at a repository of
/// the same tree.
pub async fn add_source_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    source_id: Uuid,
    new: NewSourceRepository,
) -> Result<SourceRepository, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Source, source_id).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Repository, new.repository_id).await?;
    let sort_order = match new.sort_order {
        Some(order) => order,
        None => next_sort_order(&txn, source_id).await?,
    };
    let id = Uuid::now_v7();
    let pending = Change::create(tree_id, AuditEntity::SourceRepository, id)
        .source(source_id)
        .prepare(&txn)
        .await?;
    let link = SourceRepositoryRepo::create(
        &txn,
        id,
        source_id,
        new.repository_id,
        SourceRepositoryFields {
            call_number: blank_to_none(new.call_number),
            media_type: new.media_type,
            sort_order,
        },
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(link)
}

/// Update link `id` of source `source_id` of `tree_id`. A link of another
/// source, or a repository of another tree, is not found.
pub async fn update_source_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    source_id: Uuid,
    id: Uuid,
    patch: SourceRepositoryPatch,
) -> Result<SourceRepository, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_link(&txn, tree_id, source_id, id).await?;
    if let Some(repository_id) = patch.repository_id {
        require_tree_resource(&txn, tree_id, TreeResource::Repository, repository_id).await?;
    }
    let pending = Change::update(tree_id, AuditEntity::SourceRepository, id)
        .source(source_id)
        .prepare(&txn)
        .await?;
    let link = SourceRepositoryRepo::update(
        &txn,
        id,
        LinkRepoPatch {
            repository_id: patch.repository_id,
            call_number: patch.call_number.map(blank_to_none),
            media_type: patch.media_type,
            sort_order: patch.sort_order,
        },
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(link)
}

/// Remove link `id` of source `source_id` of `tree_id`.
pub async fn remove_source_repository(
    db: &DatabaseConnection,
    tree_id: Uuid,
    source_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_link(&txn, tree_id, source_id, id).await?;
    let pending = Change::delete(tree_id, AuditEntity::SourceRepository, id)
        .source(source_id)
        .prepare(&txn)
        .await?;
    SourceRepositoryRepo::delete(&txn, id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await
}

/// Fail with `NotFound` unless link `id` belongs to source `source_id` of
/// `tree_id`.
async fn require_link(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    source_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Source, source_id).await?;
    let link = SourceRepositoryRepo::get(db, id).await?;
    if link.source_id != source_id {
        return Err(OxidGeneError::NotFound {
            entity: "SourceRepository",
            id,
        });
    }
    Ok(())
}

/// The order after the last of a source's links.
async fn next_sort_order(db: &impl ConnectionTrait, source_id: Uuid) -> Result<i32, OxidGeneError> {
    Ok(SourceRepositoryRepo::list_by_source(db, source_id)
        .await?
        .iter()
        .map(|l| l.sort_order + 1)
        .max()
        .unwrap_or(0))
}

/// `name` trimmed, unless it is blank.
fn required_name(name: &str) -> Result<String, OxidGeneError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(OxidGeneError::Validation(
            "name must not be empty".to_string(),
        ));
    }
    Ok(name.to_string())
}

/// `text` trimmed, unless it is blank.
fn blank_to_none(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}
