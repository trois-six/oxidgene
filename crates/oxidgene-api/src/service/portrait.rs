//! Portraits: choosing one, and the batched resolution shared by REST and
//! GraphQL.
//!
//! Addresses, not pictures. A pedigree asks for every person on screen at once,
//! and this used to read and cut each of their portraits and return the lot
//! base64-encoded — so a chart of a hundred people read a hundred pictures out
//! of the store before it could draw anything, and the reader's engine could
//! neither cache one nor skip one that never scrolled into view.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{ImageCrop, ImageSource, Person, Portrait, PortraitRef, is_remote_url};
use oxidgene_db::repo::{PersonRepo, PortraitRow};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::history::Change;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

const MAX_PORTRAITS_PER_REQUEST: usize = 1_024;

/// What is to represent a person: a whole media, a region of one — a face in
/// a group photograph — or, both absent, nothing.
///
/// At most one of the two may be given.
#[derive(Debug, Default, Deserialize)]
pub struct PortraitChoice {
    #[serde(default)]
    pub media_id: Option<Uuid>,
    /// A region of a larger image.
    #[serde(default)]
    pub vignette_id: Option<Uuid>,
}

impl PortraitChoice {
    /// The choice as one value, refusing the state the model cannot hold.
    fn portrait(&self) -> Result<Portrait, OxidGeneError> {
        match (self.media_id, self.vignette_id) {
            (Some(_), Some(_)) => Err(OxidGeneError::Validation(
                "a portrait is a media or a vignette, never both".to_string(),
            )),
            (Some(id), None) => Ok(Portrait::Media(id)),
            (None, Some(id)) => Ok(Portrait::Vignette(id)),
            (None, None) => Ok(Portrait::None),
        }
    }
}

/// Choose what represents person `person_id` of `tree_id`: a whole media, a
/// region of one, or nothing. Every record named must belong to the tree.
pub async fn set_person_portrait(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    choice: PortraitChoice,
) -> Result<Person, OxidGeneError> {
    let portrait = choice.portrait()?;
    let txn = begin_tx(db).await?;
    for (resource, id) in [
        (TreeResource::Person, Some(person_id)),
        (TreeResource::Media, choice.media_id),
        (TreeResource::Vignette, choice.vignette_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id).await?;
        }
    }
    // A portrait is media, never versioned; the person is the subject.
    let pending = Change::update(tree_id, AuditEntity::Portrait, person_id)
        .person(person_id)
        .prepare(&txn)
        .await?;
    let person = PersonRepo::set_portrait(&txn, person_id, portrait).await?;
    // The portrait is embedded in `person_denorm`, so the projection has to be
    // rebuilt or the tree keeps drawing the old one.
    profiles.rebuild_person(&txn, tree_id, person_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(person)
}

/// Where one person's portrait comes from.
#[derive(Debug, Clone, Serialize)]
pub struct PortraitImage {
    pub person_id: Uuid,
    pub source: ImageSource,
    /// Set when `source` is a whole picture the client must crop itself — a
    /// face identified on a photograph we do not hold, which we never fetch to
    /// cut. Absent for every portrait the backend cuts for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
}

/// Resolve locally-held and remote portraits for a bounded set of people.
pub async fn load_portrait_images(
    db: &DatabaseConnection,
    tree_id: Uuid,
    person_ids: &[Uuid],
) -> Result<Vec<PortraitImage>, OxidGeneError> {
    if person_ids.len() > MAX_PORTRAITS_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_PORTRAITS_PER_REQUEST} portraits can be loaded at once"
        )));
    }

    Ok(PersonRepo::list_portraits_for(db, tree_id, person_ids)
        .await?
        .into_iter()
        .filter_map(portrait_image)
        .collect())
}

/// Where each of `person_ids` that has a portrait draws it from, read in one
/// query (one per thousand people).
pub async fn portrait_refs(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
    person_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, PortraitRef>, OxidGeneError> {
    if person_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    Ok(PersonRepo::list_portraits_for(db, tree_id, person_ids)
        .await?
        .into_iter()
        .filter_map(portrait_image)
        .map(|image| {
            (
                image.person_id,
                PortraitRef {
                    source: image.source,
                    crop: image.crop,
                },
            )
        })
        .collect())
}

/// Give each search row the portrait its person has, if any.
pub async fn with_portraits(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
    entries: &mut [oxidgene_core::projection::SearchEntry],
) -> Result<(), OxidGeneError> {
    let ids: Vec<Uuid> = entries.iter().map(|entry| entry.person_id).collect();
    let mut portraits = portrait_refs(db, tree_id, &ids).await?;
    for entry in entries {
        entry.portrait = portraits.remove(&entry.person_id);
    }
    Ok(())
}

fn portrait_image(row: PortraitRow) -> Option<PortraitImage> {
    let mut crop = None;
    // A region we hold is cut by the crop endpoint; a whole picture we hold is
    // drawn from the thumbnail we generated.
    let source = if let (Some(vignette_id), true) = (row.vignette_id, row.storage_key.is_some()) {
        ImageSource::Crop { vignette_id }
    } else if row.thumbnail_key.is_some() {
        ImageSource::Thumbnail {
            media_id: row.media_id?,
        }
    } else if is_remote_url(&row.file_path) {
        // A face identified on a photograph we do not hold. We never fetch it
        // to cut the face out, so the whole picture travels with the rectangle
        // to take out of it and the client does the cutting. Where nobody has
        // measured the picture yet there is no scale to cut at, and the whole
        // photograph stands as the portrait.
        if let Some(rect) = row.crop {
            crop = ImageCrop::new(rect, row.source_size);
        }
        ImageSource::Remote { url: row.file_path }
    } else {
        return None;
    };

    Some(PortraitImage {
        person_id: row.person_id,
        source,
        crop,
    })
}
