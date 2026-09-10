//! Batched read model for media gallery tiles.
//!
//! Addresses, not pictures. This used to read every thumbnail out of the store,
//! cut every crop, and return the lot base64-encoded in one JSON document — so
//! opening a profile read and encoded its whole album before anything could be
//! drawn, and the reader's engine could neither cache a picture nor skip one it
//! never scrolled to. Now each tile says where its picture lives and the client
//! asks for the ones it actually draws.

use std::collections::{HashMap, HashSet};

use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{ImageCrop, ImageSource};
use oxidgene_db::repo::{MediaLinkRepo, MediaRepo, VignetteRepo};
use sea_orm::DatabaseConnection;
use serde::Serialize;
use uuid::Uuid;

const MAX_ITEMS_PER_REQUEST: usize = 1_024;
/// How many pages a document tile draws. The grid is two by two.
const MAX_DOCUMENT_PREVIEWS: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct GalleryBundle {
    pub media: Vec<GalleryMedia>,
    pub vignettes: Vec<GalleryVignette>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryMedia {
    pub media_id: Uuid,
    pub source: Option<ImageSource>,
    pub event_ids: Vec<Uuid>,
    pub document_previews: Vec<ImageSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GalleryVignette {
    pub vignette_id: Uuid,
    pub source: ImageSource,
    /// Set when `source` is a whole picture the client must crop itself — a
    /// region of a file we do not hold, which we never fetch to cut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
}

pub async fn load_gallery_bundle(
    db: &DatabaseConnection,
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
        .filter(|item| item.is_document())
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

    // Which vignettes we can cut ourselves. Whether the cut then succeeds is
    // the crop endpoint's business: reading and decoding every picture here
    // just to find out is exactly the work this bundle no longer does.
    let cuttable = vignettes
        .iter()
        .filter(|vignette| {
            vignette_media.get(&vignette.media_id).is_some_and(|media| {
                media.storage_key.is_some()
                    && crate::media::thumbnail::can_thumbnail(&media.mime_type)
            })
        })
        .map(|vignette| vignette.id)
        .collect::<HashSet<_>>();
    // A region drawn on a page we only have a URL for cannot be cut here:
    // cutting means re-decoding our own copy, and we never fetch somebody
    // else's file. So the whole picture is sent with the rectangle to take out
    // of it, and the client does the cutting. Where the page's pixel size was
    // never recorded there is no scale to cut at, and the whole picture stands
    // on its own — the crop badge already says it is a region.
    let remote_crops = vignettes
        .iter()
        .filter(|vignette| !cuttable.contains(&vignette.id))
        .filter_map(|vignette| {
            let media = vignette_media.get(&vignette.media_id)?;
            is_remote_image(media).then_some(())?;
            Some((
                vignette.id,
                media.file_path.trim().to_string(),
                ImageCrop::for_remote(
                    media,
                    vignette.x,
                    vignette.y,
                    vignette.width,
                    vignette.height,
                ),
            ))
        })
        .collect::<Vec<_>>();

    let event_ids =
        links
            .into_iter()
            .fold(HashMap::<Uuid, Vec<Uuid>>::new(), |mut grouped, link| {
                if let Some(event_id) = link.event_id {
                    grouped.entry(link.media_id).or_default().push(event_id);
                }
                grouped
            });
    let mut previews = HashMap::<Uuid, Vec<ImageSource>>::new();
    for page in pages {
        let Some(document_id) = page.parent_media_id else {
            continue;
        };
        let document_previews = previews.entry(document_id).or_default();
        if document_previews.len() >= MAX_DOCUMENT_PREVIEWS {
            continue;
        }
        // A page we hold is drawn from the thumbnail we rasterised. A page we
        // only have a URL for is drawn by the browser from that URL — it is a
        // picture like any other, and the alternative is a document tile
        // showing a file icon for a photograph the viewer can see perfectly
        // well one click away.
        if page.thumbnail_key.is_some() {
            document_previews.push(ImageSource::Thumbnail { media_id: page.id });
        } else if is_remote_image(&page) {
            document_previews.push(ImageSource::Remote {
                url: page.file_path.trim().to_string(),
            });
        }
    }

    Ok(GalleryBundle {
        media: media
            .into_iter()
            .map(|item| GalleryMedia {
                source: item
                    .thumbnail_key
                    .is_some()
                    .then_some(ImageSource::Thumbnail { media_id: item.id }),
                event_ids: event_ids.get(&item.id).cloned().unwrap_or_default(),
                document_previews: previews.remove(&item.id).unwrap_or_default(),
                media_id: item.id,
            })
            .collect(),
        vignettes: cuttable
            .into_iter()
            .map(|vignette_id| GalleryVignette {
                vignette_id,
                source: ImageSource::Crop { vignette_id },
                crop: None,
            })
            .chain(
                remote_crops
                    .into_iter()
                    .map(|(vignette_id, url, crop)| GalleryVignette {
                        vignette_id,
                        source: ImageSource::Remote { url },
                        crop,
                    }),
            )
            .collect(),
    })
}

/// Whether a page is a picture the browser can fetch for itself.
///
/// Only an image: a remote PDF or video has no still to draw, and an `<img>`
/// pointed at one renders the broken-image glyph rather than nothing.
fn is_remote_image(page: &oxidgene_core::types::Media) -> bool {
    oxidgene_core::types::is_remote_url(&page.file_path)
        && page
            .mime_type
            .trim()
            .to_ascii_lowercase()
            .starts_with("image/")
}
