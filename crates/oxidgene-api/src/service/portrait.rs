//! Batched portrait resolution shared by REST and GraphQL.
//!
//! Addresses, not pictures. A pedigree asks for every person on screen at once,
//! and this used to read and cut each of their portraits and return the lot
//! base64-encoded — so a chart of a hundred people read a hundred pictures out
//! of the store before it could draw anything, and the reader's engine could
//! neither cache one nor skip one that never scrolled into view.

use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{ImageCrop, ImageSource, is_remote_url};
use oxidgene_db::repo::{PersonRepo, PortraitRow};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

const MAX_PORTRAITS_PER_REQUEST: usize = 1_024;

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
