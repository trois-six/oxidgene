//! Media-link writes: the same steps whether REST or GraphQL asked.
//!
//! A link attaches a media to a person, an event, a source or a family of the
//! same tree. A person's card draws their first linked photograph when they
//! have no chosen portrait, so linking or unlinking one rewrites their
//! projection, in the transaction that records the change.

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{Media, MediaLink, RecordKind};
use oxidgene_db::repo::{MediaLinkRepo, MediaLinkRow, MediaLinkTarget};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::authz::records::Records;
use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// The media of `tree_id` linked to entity `entity_id` of kind `target`,
/// each with its link, as the viewer of `records` sees them: a hidden link
/// or medium is left out.
pub async fn entity_media(
    db: &DatabaseConnection,
    records: &Records,
    tree_id: Uuid,
    target: MediaLinkTarget,
    entity_id: Uuid,
) -> Result<Vec<(MediaLink, Media)>, OxidGeneError> {
    let resource = match target {
        MediaLinkTarget::Person => TreeResource::Person,
        MediaLinkTarget::Family => TreeResource::Family,
        MediaLinkTarget::Event => TreeResource::Event,
        MediaLinkTarget::Source => TreeResource::Source,
    };
    require_tree_resource(db, tree_id, resource, entity_id).await?;
    let rows = MediaLinkRepo::list_with_media(db, target, entity_id).await?;
    Ok(rows
        .into_iter()
        .filter(|(link, _)| !records.hides_media_link(link.id))
        .filter_map(|(link, media)| Some((link, records.show(media)?)))
        .collect())
}

/// The links of medium `media_id` of `tree_id` the viewer of `records` sees;
/// `not_found` for a medium they may not see.
pub async fn links_of_media(
    db: &DatabaseConnection,
    records: &Records,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<Vec<MediaLink>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Media, media_id).await?;
    records.require_id(RecordKind::Media, media_id)?;
    Ok(records.media_links(MediaLinkRepo::list_by_media(db, media_id).await?))
}

/// The person and event links of `tree_id` the viewer of `records` sees.
pub async fn tree_links(
    db: &DatabaseConnection,
    records: &Records,
    tree_id: Uuid,
) -> Result<Vec<MediaLinkRow>, OxidGeneError> {
    let mut rows = MediaLinkRepo::list_for_tree(db, tree_id).await?;
    rows.retain(|r| {
        !records.hides_media_link(r.link_id) && !records.hides(RecordKind::Media, r.media_id)
    });
    Ok(rows)
}

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
    viewer: &ViewerContext,
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
    Change::create(viewer, tree_id, AuditEntity::MediaLink, id)
        .media(link.media_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(link)
}

/// Delete media link `id` of `tree_id`; the media stays.
pub async fn delete_media_link(
    viewer: &ViewerContext,
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
    Change::delete(viewer, tree_id, AuditEntity::MediaLink, id)
        .media(link.media_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await
}
