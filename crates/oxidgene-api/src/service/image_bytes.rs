//! Reading held pictures back as inline data, for clients that cannot serve
//! them from an origin of their own.
//!
//! A picture normally travels as an [`ImageSource`] and its bytes over their
//! own request — see `service::gallery`. A client whose shell can answer for a
//! path (the desktop) resolves those itself and never comes here.
//!
//! The web build has no such shell. Until authentication ships it may not put a
//! backend address in its markup either (`docs/cross-cutting.md`
//! §7.1), so it fetches the bytes and hands them to the engine as `data:` URLs.
//! Doing that one picture at a time is a request per portrait on a pedigree,
//! which is what this exists to avoid: it answers for a whole screen at once.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use futures_util::{StreamExt as _, stream};
use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::types::{ImageSource, Media, Vignette};
use oxidgene_db::repo::{MediaRepo, VignetteRepo};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::authz::records::Records;
use crate::media::MediaStore;
use crate::media::thumbnail::Rendition;

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
#[tracing::instrument(name = "images.load", skip_all, fields(image.count = sources.len()))]
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

    let reads = plan_reads(db, tree_id, sources).await?;
    let mut resolved = stream::iter(reads.into_iter().enumerate())
        .map(|(index, read)| {
            let store = Arc::clone(store);
            async move {
                let (data, written) = match read {
                    Some(read) => read.load(&store).await,
                    None => (None, None),
                };
                (index, data, written)
            }
        })
        .buffer_unordered(BLOB_READ_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
    // The crops this request cut and stored, confirmed together: a fixed
    // number of statements however many there are.
    let written: Vec<crate::media::crops::Written> = resolved
        .iter_mut()
        .filter_map(|(_, _, written)| written.take())
        .collect();
    crate::media::crops::confirm(db, &**store, &written).await;
    resolved.sort_by_key(|(index, _, _)| *index);
    Ok(resolved.into_iter().map(|(_, data, _)| data).collect())
}

/// [`load_image_data_urls`] of what the viewer of `records` may see: the
/// slot of a hidden picture is `None`, as that of one we no longer hold.
pub async fn load_visible_image_data_urls(
    db: &DatabaseConnection,
    records: &Records,
    store: &Arc<dyn MediaStore>,
    tree_id: Uuid,
    sources: &[ImageSource],
) -> Result<Vec<Option<String>>, OxidGeneError> {
    if records.is_unrestricted() {
        return load_image_data_urls(db, store, tree_id, sources).await;
    }
    let shown: Vec<ImageSource> = sources
        .iter()
        .filter(|source| !records.hides_image(source))
        .cloned()
        .collect();
    let mut loaded = load_image_data_urls(db, store, tree_id, &shown)
        .await?
        .into_iter();
    Ok(sources
        .iter()
        .map(|source| {
            if records.hides_image(source) {
                None
            } else {
                loaded.next().flatten()
            }
        })
        .collect())
}

/// What producing one picture takes: a stored blob, or a vignette's crop,
/// stored or cut on first view.
enum BlobRead {
    /// A stored picture: a thumbnail or a card rendition.
    Stored { key: String },
    /// A held, drawable medium (its storage key known), the vignette of it,
    /// and the size to send it at.
    Crop {
        medium: Box<Media>,
        vignette: Box<Vignette>,
        rendition: Rendition,
    },
}

impl BlobRead {
    /// The picture as a `data:` URL, and the crop it stored, if it cut one.
    async fn load(
        self,
        store: &Arc<dyn MediaStore>,
    ) -> (Option<String>, Option<crate::media::crops::Written>) {
        match self {
            Self::Stored { key } => {
                let Ok(bytes) = store.get(&key).await else {
                    return (None, None);
                };
                // `ingest` only ever writes `jpg` or `png`, and the key says
                // which.
                let mime_type = if key.ends_with(".png") {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                (Some(data_url(mime_type, &bytes)), None)
            }
            Self::Crop {
                medium,
                vignette,
                rendition,
            } => match crate::media::crops::read_or_cut(&**store, &medium, &vignette, rendition)
                .await
            {
                Ok((cropped, written)) => (Some(data_url("image/jpeg", &cropped)), written),
                Err(_) => (None, None),
            },
        }
    }
}

/// The blob each source needs, in request order, read from the database in
/// two batches — the vignettes, then every medium named — rather than one or
/// two queries per picture. `None` where nothing can be drawn: a remote
/// source (we never proxy somebody else's bandwidth; the client fetches it
/// itself), a record that is gone or of another tree, a medium without the
/// file a picture needs.
async fn plan_reads(
    db: &DatabaseConnection,
    tree_id: Uuid,
    sources: &[ImageSource],
) -> Result<Vec<Option<BlobRead>>, OxidGeneError> {
    let vignette_ids = sorted_unique(sources.iter().filter_map(|source| match source {
        ImageSource::Crop { vignette_id } | ImageSource::CardCrop { vignette_id } => {
            Some(*vignette_id)
        }
        _ => None,
    }));
    let vignettes: HashMap<Uuid, Vignette> = VignetteRepo::get_many(db, &vignette_ids)
        .await?
        .into_iter()
        .map(|vignette| (vignette.id, vignette))
        .collect();
    let media_ids = sorted_unique(
        sources
            .iter()
            .filter_map(|source| match source {
                ImageSource::Thumbnail { media_id } | ImageSource::Card { media_id } => {
                    Some(*media_id)
                }
                _ => None,
            })
            .chain(vignettes.values().map(|vignette| vignette.media_id)),
    );
    let media: HashMap<Uuid, Media> = MediaRepo::get_many(db, &media_ids)
        .await?
        .into_iter()
        .filter(|media| media.tree_id == tree_id)
        .map(|media| (media.id, media))
        .collect();

    Ok(sources
        .iter()
        .map(|source| match source {
            ImageSource::Remote { .. } => None,
            ImageSource::Thumbnail { media_id } => {
                let key = media.get(media_id)?.thumbnail_key.clone()?;
                Some(BlobRead::Stored { key })
            }
            ImageSource::Card { media_id } => {
                let medium = media.get(media_id)?;
                let key = card_or_thumbnail(medium)?.to_owned();
                Some(BlobRead::Stored { key })
            }
            ImageSource::Crop { vignette_id } | ImageSource::CardCrop { vignette_id } => {
                let rendition = match source {
                    ImageSource::CardCrop { .. } => Rendition::Card,
                    _ => Rendition::Thumbnail,
                };
                let vignette = vignettes.get(vignette_id)?;
                let medium = media.get(&vignette.media_id)?;
                if !crate::media::thumbnail::can_crop(&medium.mime_type)
                    || medium.storage_key.is_none()
                {
                    return None;
                }
                Some(BlobRead::Crop {
                    medium: Box::new(medium.clone()),
                    vignette: Box::new(vignette.clone()),
                    rendition,
                })
            }
        })
        .collect())
}

/// The key a card-sized picture of `medium` is read from: its card
/// rendition, or its thumbnail where it has none — a picture stored before
/// card renditions existed, or one whose card could not be stored.
pub fn card_or_thumbnail(medium: &Media) -> Option<&str> {
    medium
        .card_key
        .as_deref()
        .or(medium.thumbnail_key.as_deref())
}

fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    let _span = tracing::info_span!("image.encode_base64", image.bytes = bytes.len()).entered();
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime_type};base64,{encoded}")
}
