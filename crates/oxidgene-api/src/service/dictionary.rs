//! The Dictionary's reads, REST's, GraphQL's and MCP's alike, as the
//! viewer sees the tree (docs/privacy.md §7): values and counts from the
//! records the viewer's [`Redaction`] shows, usage lists without the persons
//! it hides.
//!
//! The owner's redaction is unrestricted: each read is then the repository's
//! own, with no statement more. A restricted viewer's reads go through the
//! repository's `*_for` twins.

use std::sync::Arc;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::redaction::Redaction;
use oxidgene_core::types::Place;
use oxidgene_db::repo::{DictionaryRepo, DictionaryValueEntry, PersonUsageEntry};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::scope::{TreeResource, require_tree_resource};

/// What `viewer` sees of tree `tree_id`: built once per request.
///
/// # Errors
///
/// What resolving the viewer's access answers, and database failures.
pub async fn seen_by(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    viewer: &ViewerContext,
    tree_id: Uuid,
) -> Result<Arc<Redaction>, OxidGeneError> {
    crate::authz::redaction::redaction(db, profiles, viewer, tree_id).await
}

/// The family names the viewer sees, with their bearers.
pub async fn family_names(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
    DictionaryRepo::family_names_for(db, tree_id, redaction).await
}

/// The occupations the viewer sees, with their holders.
pub async fn occupations(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
    DictionaryRepo::occupations_for(db, tree_id, redaction).await
}

/// The places the viewer sees, with their visible uses.
pub async fn places(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
) -> Result<Vec<(Place, i64)>, OxidGeneError> {
    DictionaryRepo::places_with_usage_for(db, tree_id, redaction).await
}

/// The persons the viewer sees among those `ids` lists, resolved into the
/// usage panel's entries.
async fn usage_entries(
    db: &DatabaseConnection,
    redaction: &Redaction,
    ids: Vec<Uuid>,
) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
    let ids: Vec<Uuid> = ids.into_iter().filter(|id| redaction.shows(*id)).collect();
    DictionaryRepo::resolve_person_usage_entries(db, &ids).await
}

/// The bearers of family name `value` the viewer sees.
pub async fn family_name_usage(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    value: &str,
) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
    let ids = DictionaryRepo::family_name_usage_person_ids(db, tree_id, value).await?;
    usage_entries(db, redaction, ids).await
}

/// The holders of occupation `value` the viewer sees.
pub async fn occupation_usage(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    value: &str,
) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
    let ids = DictionaryRepo::occupation_usage_person_ids(db, tree_id, value).await?;
    usage_entries(db, redaction, ids).await
}

/// The persons source `source_id` of `tree_id` documents, through the
/// citations the viewer sees.
pub async fn source_usage(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    source_id: Uuid,
) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Source, source_id).await?;
    let ids =
        DictionaryRepo::source_usage_person_ids_for(db, tree_id, source_id, redaction).await?;
    usage_entries(db, redaction, ids).await
}

/// The persons place `place_id` of `tree_id` concerns, through the events
/// and media the viewer sees.
pub async fn place_usage(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    place_id: Uuid,
) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Place, place_id).await?;
    let ids = DictionaryRepo::place_usage_person_ids_for(db, tree_id, place_id, redaction).await?;
    usage_entries(db, redaction, ids).await
}

/// The Sources tab's drill-down from `prefix` over the sources the viewer
/// sees: the resolved prefix, the sources under it, and the next branch
/// choices, or the final list once they are few enough.
pub async fn source_drill(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    prefix: &str,
) -> Result<SourceDrill, OxidGeneError> {
    let (prefix, total, groups) = DictionaryRepo::resolve_source_drill_down_for(
        db,
        tree_id,
        prefix,
        oxidgene_db::repo::SOURCE_DRILL_THRESHOLD,
        redaction,
    )
    .await?;
    let sources = if groups.is_empty() {
        Some(
            crate::service::source::dictionary_sources(db, redaction, tree_id, &prefix, false)
                .await?,
        )
    } else {
        None
    };
    Ok(SourceDrill {
        prefix,
        total,
        groups,
        sources,
    })
}

/// One level of the Sources tab's drill-down.
#[derive(Debug)]
pub struct SourceDrill {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<(String, i64)>,
    pub sources: Option<Vec<crate::service::source::SourceDictionaryEntry>>,
}
