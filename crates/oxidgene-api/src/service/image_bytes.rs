//! Reading held pictures back as inline data, for clients that cannot serve
//! them from an origin of their own.
//!
//! A picture normally travels as an [`ImageSource`] and its bytes over their
//! own request — see `service::gallery`. A client whose shell can answer for a
//! path (the desktop) resolves those itself and never comes here.
//!
//! The web build has no such shell. Until authentication ships it may not put a
//! backend address in its markup either (`docs/specifications/cross-cutting.md`
//! §7.1), so it fetches the bytes and hands them to the engine as `data:` URLs.
//! Doing that one picture at a time is a request per portrait on a pedigree,
//! which is what this exists to avoid: it answers for a whole screen at once.

use std::sync::Arc;

use base64::Engine as _;
use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::ImageSource;
use oxidgene_db::repo::{MediaRepo, VignetteRepo};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::media::MediaStore;

/// Bounded like every other batch operation. A screen asks for what it draws.
pub const MAX_IMAGES_PER_REQUEST: usize = 1_024;

/// How many blobs are read at once, so one request cannot saturate the store.
const BLOB_READ_CONCURRENCY: usize = 8;

/// Resolve each source to a `data:` URL, in request order.
///
/// A slot is `None` when the picture cannot be produced: a remote source, whose
/// bytes are somebody else's and which the client fetches for itself; a media
/// we no longer hold; a crop that fails to cut. The client draws nothing there,
/// exactly as it would have had the source never arrived.
pub async fn load_image_data_urls(
    db: &DatabaseConnection,
    store: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    sources: &[ImageSource],
) -> Result<Vec<Option<String>>, OxidGeneError> {
    if sources.len() > MAX_IMAGES_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_IMAGES_PER_REQUEST} images can be loaded at once"
        )));
    }

    let mut resolved = stream::iter(sources.iter().cloned().enumerate())
        .map(|(index, source)| {
            let store = Arc::clone(store);
            async move { (index, load_one(db, &store, tree_id, source).await) }
        })
        .buffer_unordered(BLOB_READ_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    resolved.sort_by_key(|(index, _)| *index);
    Ok(resolved.into_iter().map(|(_, data)| data).collect())
}

async fn load_one(
    db: &DatabaseConnection,
    store: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    source: ImageSource,
) -> Option<String> {
    match source {
        // We never proxy somebody else's bandwidth: the client already has the
        // address and fetches it directly.
        ImageSource::Remote { .. } => None,
        ImageSource::Thumbnail { media_id } => {
            let media = MediaRepo::get(db, media_id).await.ok()?;
            if media.tree_id != tree_id {
                return None;
            }
            let key = media.thumbnail_key?;
            let bytes = store.get(&key).await.ok()?;
            // `ingest` only ever writes `jpg` or `png`, and the key says which.
            let mime_type = if key.ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            };
            Some(data_url(mime_type, &bytes))
        }
        ImageSource::Crop { vignette_id } => {
            let vignette = VignetteRepo::get(db, vignette_id).await.ok()?;
            let media = MediaRepo::get(db, vignette.media_id).await.ok()?;
            if media.tree_id != tree_id {
                return None;
            }
            let key = media.storage_key?;
            if !crate::media::thumbnail::can_thumbnail(&media.mime_type) {
                return None;
            }
            let bytes = store.get(&key).await.ok()?;
            let rect = (vignette.x, vignette.y, vignette.width, vignette.height);
            let cropped =
                tokio::task::spawn_blocking(move || crate::media::thumbnail::crop(&bytes, rect))
                    .await
                    .ok()?
                    .ok()?;
            Some(data_url("image/jpeg", &cropped))
        }
    }
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime_type};base64,{encoded}")
}
