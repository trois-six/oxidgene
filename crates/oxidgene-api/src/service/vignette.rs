//! Vignette writes — named rectangles cut out of a media file: the same steps
//! whether REST or GraphQL asked.
//!
//! A vignette may be attributed to a person and stand as evidence for an
//! event, and both must belong to the vignette's tree: those columns cascade
//! on delete, so a vignette pointing into another tree would silently vanish
//! with a record of that tree. Each write records the change in its
//! transaction; deleting a vignette someone uses as their portrait rewrites
//! their projection.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::Vignette;
use oxidgene_db::repo::{MediaRepo, PersonRepo, VignetteInput, VignettePatch, VignetteRepo};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A region to cut out of a media file, and what it shows.
#[derive(Debug, Deserialize)]
pub struct NewVignette {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
}

/// A vignette move or re-attribution. The four rectangle fields travel
/// together — all of them or none — and `Some(None)` clears an attribution.
#[derive(Debug, Default, Deserialize)]
pub struct VignetteUpdate {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub person_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub event_id: Option<Option<Uuid>>,
}

/// Cut a vignette out of media `media_id` of `tree_id`.
pub async fn create_vignette(
    db: &DatabaseConnection,
    tree_id: Uuid,
    media_id: Uuid,
    new: NewVignette,
) -> Result<Vignette, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, media_id).await?;
    require_subjects(&txn, tree_id, new.person_id, new.event_id).await?;
    let media = MediaRepo::get(&txn, media_id).await?;
    crate::media::validate_crop(&media, new.x, new.y, new.width, new.height)?;
    let vignette = VignetteRepo::create(
        &txn,
        Uuid::now_v7(),
        VignetteInput {
            media_id,
            x: new.x,
            y: new.y,
            width: new.width,
            height: new.height,
            person_id: new.person_id,
            event_id: new.event_id,
        },
    )
    .await?;
    Change::create(tree_id, AuditEntity::Vignette, vignette.id)
        .media(media_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(vignette)
}

/// Move or re-attribute vignette `id` of `tree_id`.
pub async fn update_vignette(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    update: VignetteUpdate,
) -> Result<Vignette, OxidGeneError> {
    // A rectangle is four numbers that only mean anything together: moving
    // one edge alone would let a client build a crop the media cannot hold.
    let rect = match (update.x, update.y, update.width, update.height) {
        (None, None, None, None) => None,
        (Some(x), Some(y), Some(width), Some(height)) => Some((x, y, width, height)),
        _ => {
            return Err(OxidGeneError::Validation(
                "x, y, width and height must be sent together".into(),
            ));
        }
    };
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Vignette, id).await?;
    require_subjects(
        &txn,
        tree_id,
        update.person_id.flatten(),
        update.event_id.flatten(),
    )
    .await?;
    let existing = VignetteRepo::get(&txn, id).await?;
    if let Some((x, y, width, height)) = rect {
        let media = MediaRepo::get(&txn, existing.media_id).await?;
        crate::media::validate_crop(&media, x, y, width, height)?;
    }
    let vignette = VignetteRepo::update(
        &txn,
        id,
        VignettePatch {
            rect,
            person_id: update.person_id,
            event_id: update.event_id,
        },
    )
    .await?;
    Change::update(tree_id, AuditEntity::Vignette, id)
        .media(existing.media_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(vignette)
}

/// Delete vignette `id` of `tree_id`; the media it cropped is untouched. A
/// person using it as their portrait falls back to their other pictures.
pub async fn delete_vignette(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Vignette, id).await?;
    let media_id = VignetteRepo::get(&txn, id).await?.media_id;
    // Read while the portrait still points at it.
    let affected = PersonRepo::portrayed_by_vignette(&txn, id).await?;
    VignetteRepo::delete(&txn, id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::delete(tree_id, AuditEntity::Vignette, id)
        .media(media_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}

/// Fail with `NotFound` unless the person and the event a vignette names
/// belong to tree `tree_id`.
async fn require_subjects(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    person_id: Option<Uuid>,
    event_id: Option<Uuid>,
) -> Result<(), OxidGeneError> {
    if let Some(person_id) = person_id {
        require_tree_resource(db, tree_id, TreeResource::Person, person_id).await?;
    }
    if let Some(event_id) = event_id {
        require_tree_resource(db, tree_id, TreeResource::Event, event_id).await?;
    }
    Ok(())
}
