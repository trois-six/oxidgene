//! Batched portrait image resolution shared by REST and GraphQL.

use std::sync::Arc;

use base64::Engine as _;
use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{PersonRepo, PortraitRow};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::media::MediaStore;

const MAX_PORTRAITS_PER_REQUEST: usize = 1_024;
const BLOB_READ_CONCURRENCY: usize = 8;

/// A portrait source ready for an image element.
#[derive(Debug, Clone, Serialize)]
pub struct PortraitImage {
    pub person_id: Uuid,
    pub source: String,
}

/// Resolve locally-held and remote portraits for a bounded set of people.
pub async fn load_portrait_images(
    db: &DatabaseConnection,
    media: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    person_ids: &[Uuid],
) -> Result<Vec<PortraitImage>, OxidGeneError> {
    if person_ids.len() > MAX_PORTRAITS_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_PORTRAITS_PER_REQUEST} portraits can be loaded at once"
        )));
    }

    let rows = PersonRepo::list_portraits_for(db, tree_id, person_ids).await?;
    let results = stream::iter(rows)
        .map(|row| {
            let media = Arc::clone(media);
            async move { load_portrait_image(media, row).await }
        })
        .buffer_unordered(BLOB_READ_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    Ok(results
        .into_iter()
        .filter_map(|result| match result {
            Ok(image) => image,
            Err(error) => {
                tracing::warn!(%error, "portrait image could not be loaded");
                None
            }
        })
        .collect())
}

async fn load_portrait_image(
    media: Arc<dyn MediaStore>,
    row: PortraitRow,
) -> Result<Option<PortraitImage>, OxidGeneError> {
    let source = if let (Some(key), Some(rect)) = (row.storage_key.as_deref(), row.crop) {
        let bytes = media.get(key).await?;
        let cropped =
            tokio::task::spawn_blocking(move || crate::media::thumbnail::crop(&bytes, rect))
                .await
                .map_err(|error| OxidGeneError::Internal(format!("crop panicked: {error}")))??;
        data_url("image/jpeg", &cropped)
    } else if let Some(key) = row.thumbnail_key.as_deref() {
        let bytes = media.get(key).await?;
        let mime_type = if key.ends_with(".png") {
            "image/png"
        } else {
            "image/jpeg"
        };
        data_url(mime_type, &bytes)
    } else if row.file_path.starts_with("http://") || row.file_path.starts_with("https://") {
        row.file_path
    } else {
        return Ok(None);
    };

    Ok(Some(PortraitImage {
        person_id: row.person_id,
        source,
    }))
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime_type};base64,{encoded}")
}
