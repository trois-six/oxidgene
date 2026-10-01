//! Media-link writes: the same steps whether REST or GraphQL asked.
//!
//! A link attaches a media to a person, an event, a source or a family of the
//! same tree. A person's card draws their first linked photograph when they
//! have no chosen portrait, so linking or unlinking one rewrites their
//! projection, in the transaction that records the change.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::MediaLink;
use oxidgene_db::repo::MediaLinkRepo;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A media link to create: the media, and what it is attached to.
#[derive(Debug, Deserialize)]
pub struct NewMediaLink {
    pub media_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    #[serde(default)]
    pub sort_order: i32,
}

/// Link a media of `tree_id` to records of the same tree.
pub async fn create_media_link(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewMediaLink,
) -> Result<MediaLink, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, new.media_id).await?;
    for (resource, id) in [
        (TreeResource::Person, new.person_id),
        (TreeResource::Event, new.event_id),
        (TreeResource::Source, new.source_id),
        (TreeResource::Family, new.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id).await?;
        }
    }
    let id = Uuid::now_v7();
    let link = MediaLinkRepo::create(
        &txn,
        id,
        new.media_id,
        new.person_id,
        new.event_id,
        new.source_id,
        new.family_id,
        new.sort_order,
    )
    .await?;
    if let Some(person_id) = link.person_id {
        profiles.rebuild_person(&txn, tree_id, person_id).await?;
    }
    Change::create(tree_id, AuditEntity::MediaLink, id)
        .media(link.media_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(link)
}

/// Delete media link `id` of `tree_id`; the media stays.
pub async fn delete_media_link(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::MediaLink, id).await?;
    // Read before it goes: afterwards nothing says whose projection is wrong.
    let link = MediaLinkRepo::get(&txn, id).await?;
    MediaLinkRepo::delete(&txn, id).await?;
    if let Some(person_id) = link.person_id {
        profiles.rebuild_person(&txn, tree_id, person_id).await?;
    }
    Change::delete(tree_id, AuditEntity::MediaLink, id)
        .media(link.media_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}
