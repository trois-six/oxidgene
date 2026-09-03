//! Batched read model for media gallery tiles.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use base64::Engine as _;
use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{MediaLinkRepo, MediaRepo, VignetteRepo};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

use crate::media::MediaStore;

const MAX_ITEMS_PER_REQUEST: usize = 1_024;
const BLOB_READ_CONCURRENCY: usize = 8;
type CropRect = (i32, i32, i32, i32);
type CropJob = (Uuid, String, CropRect);

#[derive(Debug, Clone, Serialize)]
pub struct GalleryBundle {
    pub media: Vec<GalleryMedia>,
    pub vignettes: Vec<GalleryVignette>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryMedia {
    pub media_id: Uuid,
    pub source: Option<String>,
    pub event_ids: Vec<Uuid>,
    pub document_previews: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryVignette {
    pub vignette_id: Uuid,
    pub source: String,
}

pub async fn load_gallery_bundle(
    db: &DatabaseConnection,
    store: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    media_ids: &[Uuid],
    vignette_ids: &[Uuid],
) -> Result<GalleryBundle, OxidGeneError> {
    if media_ids.len() + vignette_ids.len() > MAX_ITEMS_PER_REQUEST {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_ITEMS_PER_REQUEST} gallery items can be loaded at once"
        )));
    }

    let media = MediaRepo::get_many(db, media_ids)
        .await?
        .into_iter()
        .filter(|item| item.tree_id == tree_id)
        .collect::<Vec<_>>();
    let valid_media_ids = media.iter().map(|item| item.id).collect::<Vec<_>>();
    let document_ids = media
        .iter()
        .filter(|item| item.is_document)
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let pages = MediaRepo::list_pages_for(db, &document_ids)
        .await?
        .into_iter()
        .filter(|page| page.tree_id == tree_id)
        .collect::<Vec<_>>();
    let links = MediaLinkRepo::list_by_medias(db, &valid_media_ids).await?;

    let vignettes = VignetteRepo::get_many(db, vignette_ids).await?;
    let vignette_media = MediaRepo::get_many(
        db,
        &vignettes
            .iter()
            .map(|vignette| vignette.media_id)
            .collect::<Vec<_>>(),
    )
    .await?
    .into_iter()
    .filter(|item| item.tree_id == tree_id)
    .map(|item| (item.id, item))
    .collect::<HashMap<_, _>>();

    let thumbnail_jobs = media
        .iter()
        .chain(pages.iter())
        .filter_map(|item| item.thumbnail_key.clone().map(|key| (item.id, key)))
        .collect::<Vec<_>>();
    let thumbnails = load_thumbnails(store, thumbnail_jobs).await;

    let crop_jobs = vignettes
        .iter()
        .filter_map(|vignette| {
            let media = vignette_media.get(&vignette.media_id)?;
            let key = media.storage_key.clone()?;
            Some((
                vignette.id,
                key,
                (vignette.x, vignette.y, vignette.width, vignette.height),
            ))
        })
        .collect::<Vec<_>>();
    let crops = load_crops(store, crop_jobs).await;

    let event_ids =
        links
            .into_iter()
            .fold(HashMap::<Uuid, Vec<Uuid>>::new(), |mut grouped, link| {
                if let Some(event_id) = link.event_id {
                    grouped.entry(link.media_id).or_default().push(event_id);
                }
                grouped
            });
    let mut previews = HashMap::<Uuid, Vec<String>>::new();
    for page in pages {
        let Some(document_id) = page.parent_media_id else {
            continue;
        };
        let document_previews = previews.entry(document_id).or_default();
        if document_previews.len() < 4
            && let Some(source) = thumbnails.get(&page.id)
        {
            document_previews.push(source.clone());
        }
    }

    Ok(GalleryBundle {
        media: media
            .into_iter()
            .map(|item| GalleryMedia {
                media_id: item.id,
                source: thumbnails.get(&item.id).cloned(),
                event_ids: event_ids.get(&item.id).cloned().unwrap_or_default(),
                document_previews: previews.remove(&item.id).unwrap_or_default(),
            })
            .collect(),
        vignettes: crops
            .into_iter()
            .map(|(vignette_id, source)| GalleryVignette {
                vignette_id,
                source,
            })
            .collect(),
    })
}

async fn load_thumbnails(
    store: &Arc<dyn MediaStore>,
    jobs: Vec<(Uuid, String)>,
) -> HashMap<Uuid, String> {
    let jobs = deduplicate_jobs(jobs);
    stream::iter(jobs)
        .map(|(id, key)| {
            let store = Arc::clone(store);
            async move {
                let bytes = store.get(&key).await.ok()?;
                let mime_type = if key.ends_with(".png") {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                Some((id, data_url(mime_type, &bytes)))
            }
        })
        .buffer_unordered(BLOB_READ_CONCURRENCY)
        .filter_map(|result| async move { result })
        .collect()
        .await
}

async fn load_crops(store: &Arc<dyn MediaStore>, jobs: Vec<CropJob>) -> HashMap<Uuid, String> {
    stream::iter(jobs)
        .map(|(id, key, rect)| {
            let store = Arc::clone(store);
            async move {
                let bytes = store.get(&key).await.ok()?;
                let cropped = tokio::task::spawn_blocking(move || {
                    crate::media::thumbnail::crop(&bytes, rect)
                })
                .await
                .ok()?
                .ok()?;
                Some((id, data_url("image/jpeg", &cropped)))
            }
        })
        .buffer_unordered(BLOB_READ_CONCURRENCY)
        .filter_map(|result| async move { result })
        .collect()
        .await
}

fn deduplicate_jobs(jobs: Vec<(Uuid, String)>) -> Vec<(Uuid, String)> {
    let mut seen = HashSet::new();
    jobs.into_iter()
        .filter(|(id, _)| seen.insert(*id))
        .collect()
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime_type};base64,{encoded}")
}
