//! Source writes: the same steps whether REST or GraphQL asked.
//!
//! A source is known by its title, so it may never be blank. Each write
//! records the change in the transaction that makes it.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::Source;
use oxidgene_db::repo::{DictionaryRepo, SourceRepo, SourceRepositoryRepo};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A source to create.
#[derive(Debug, Deserialize)]
pub struct NewSource {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    /// The organisation responsible for the source's data.
    #[serde(default)]
    pub agency: Option<String>,
}

/// The fields a source update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct SourcePatch {
    pub title: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub author: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub publisher: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub abbreviation: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub agency: Option<Option<String>>,
}

/// Create a source in `tree_id`. A blank title is refused.
pub async fn create_source(
    db: &DatabaseConnection,
    tree_id: Uuid,
    new: NewSource,
) -> Result<Source, OxidGeneError> {
    require_title(&new.title)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(tree_id, AuditEntity::Source, id)
        .source(id)
        .prepare(&txn)
        .await?;
    let source = SourceRepo::create(
        &txn,
        id,
        tree_id,
        new.title,
        new.author,
        new.publisher,
        new.abbreviation,
        new.agency,
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(source)
}

/// Update source `id` of `tree_id`. A blank title is refused.
pub async fn update_source(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    patch: SourcePatch,
) -> Result<Source, OxidGeneError> {
    if let Some(title) = &patch.title {
        require_title(title)?;
    }
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Source, id).await?;
    let pending = Change::update(tree_id, AuditEntity::Source, id)
        .source(id)
        .prepare(&txn)
        .await?;
    let source = SourceRepo::update(
        &txn,
        id,
        patch.title,
        patch.author,
        patch.publisher,
        patch.abbreviation,
        patch.agency,
    )
    .await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(source)
}

/// Delete source `id` of `tree_id` (a soft delete); whether it was deleted.
///
/// With `only_if_unused` the delete is a cleanup: a source still cited by a
/// citation, note, media link or repository link is kept, and `false` says so.
pub async fn delete_source(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    only_if_unused: bool,
) -> Result<bool, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Source, id).await?;
    let pending = Change::delete(tree_id, AuditEntity::Source, id)
        .source(id)
        .prepare(&txn)
        .await?;
    let deleted = if only_if_unused {
        SourceRepo::delete_if_unused(&txn, id).await?
    } else {
        SourceRepo::delete(&txn, id).await?;
        true
    };
    // A source kept as still used was not written: nothing to record.
    if deleted {
        pending.record(&txn).await?;
    }
    commit_tx(txn).await?;
    Ok(deleted)
}

/// The sources of `tree_id` whose title starts with `prefix`, each with its
/// citation count and the names of the repositories holding it.
pub async fn dictionary_sources(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    prefix: &str,
) -> Result<Vec<SourceDictionaryEntry>, OxidGeneError> {
    let entries = DictionaryRepo::sources_with_usage_by_prefix(db, tree_id, prefix).await?;
    let ids: Vec<Uuid> = entries.iter().map(|(s, _)| s.id).collect();
    let mut names = SourceRepositoryRepo::repository_names(db, &ids).await?;
    Ok(entries
        .into_iter()
        .map(|(source, count)| SourceDictionaryEntry {
            repositories: names.remove(&source.id).unwrap_or_default(),
            source,
            count,
        })
        .collect())
}

/// A source paired with its citation count and the repositories holding it.
#[derive(Debug, Serialize)]
pub struct SourceDictionaryEntry {
    #[serde(flatten)]
    pub source: Source,
    pub count: i64,
    /// The names of the repositories holding the source, each once.
    pub repositories: Vec<String>,
}

fn require_title(title: &str) -> Result<(), OxidGeneError> {
    if title.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "title must not be empty".to_string(),
        ));
    }
    Ok(())
}
