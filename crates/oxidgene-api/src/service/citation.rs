//! Citation writes: the same steps whether REST or GraphQL asked.
//!
//! Each write checks that every record it names belongs to the tree, rewrites
//! the projection of the person the citation is attached to, and records the
//! change, in one transaction.

use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{Citation, Connection};
use oxidgene_core::{Confidence, OxidGeneError};
use oxidgene_db::repo::{CitationFilter, CitationRepo, PaginationParams};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A citation to create: the source it cites, what it is attached to, and
/// what it says.
#[derive(Debug, Deserialize)]
pub struct NewCitation {
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub page: Option<String>,
    pub confidence: Confidence,
    pub text: Option<String>,
}

/// The fields a citation update changes: `None` keeps a field, and
/// `Some(None)` clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct CitationPatch {
    /// Repoints the citation at another source.
    pub source_id: Option<Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    pub page: Option<Option<String>>,
    pub confidence: Option<Confidence>,
    #[serde(default, deserialize_with = "double_option")]
    pub text: Option<Option<String>>,
}

/// The citations of `tree_id`, narrowed by `filter`, a page at a time. A
/// filter naming a record of another tree is not found.
pub async fn list_citations(
    db: &DatabaseConnection,
    tree_id: Uuid,
    filter: &CitationFilter,
    params: &PaginationParams,
) -> Result<Connection<Citation>, OxidGeneError> {
    for (resource, id) in [
        (TreeResource::Source, filter.source_id),
        (TreeResource::Person, filter.person_id),
        (TreeResource::Event, filter.event_id),
        (TreeResource::Family, filter.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(db, tree_id, resource, id).await?;
        }
    }
    CitationRepo::list(db, tree_id, filter, params).await
}

/// Create a citation in `tree_id`.
pub async fn create_citation(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewCitation,
) -> Result<Citation, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Source, new.source_id).await?;
    for (resource, id) in [
        (TreeResource::Person, new.person_id),
        (TreeResource::Event, new.event_id),
        (TreeResource::Family, new.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id).await?;
        }
    }
    let id = Uuid::now_v7();
    let pending = Change::create(tree_id, AuditEntity::Citation, id)
        .owner(new.person_id, new.event_id, new.family_id, None)
        .prepare(&txn)
        .await?;
    let citation = CitationRepo::create(
        &txn,
        id,
        new.source_id,
        new.person_id,
        new.event_id,
        new.family_id,
        new.page,
        new.confidence,
        new.text,
    )
    .await?;
    if let Some(person_id) = citation.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(citation)
}

/// Update citation `id` of `tree_id`.
pub async fn update_citation(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    patch: CitationPatch,
) -> Result<Citation, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Citation, id).await?;
    let previous = CitationRepo::get(&txn, id).await?;
    if let Some(source_id) = patch.source_id {
        require_tree_resource(&txn, tree_id, TreeResource::Source, source_id).await?;
    }
    let pending = change_of(
        Change::update(tree_id, AuditEntity::Citation, id),
        &previous,
    )
    .prepare(&txn)
    .await?;
    let citation = CitationRepo::update(
        &txn,
        id,
        patch.source_id,
        patch.page,
        patch.confidence,
        patch.text,
    )
    .await?;
    if let Some(person_id) = previous.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(citation)
}

/// Delete citation `id` of `tree_id`.
pub async fn delete_citation(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Citation, id).await?;
    let citation = CitationRepo::get(&txn, id).await?;
    let pending = change_of(
        Change::delete(tree_id, AuditEntity::Citation, id),
        &citation,
    )
    .prepare(&txn)
    .await?;
    CitationRepo::delete(&txn, id).await?;
    if let Some(person_id) = citation.person_id {
        profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await?;
    }
    pending.record(&txn).await?;
    commit_tx(txn).await
}

/// `change`, versioning what `citation` hangs off.
fn change_of(change: Change, citation: &Citation) -> Change {
    change.owner(
        citation.person_id,
        citation.event_id,
        citation.family_id,
        None,
    )
}
