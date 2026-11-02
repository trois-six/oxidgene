//! The pictures cut out of a page for its vignettes, stored once cut
//! (`docs/architecture.md` §4.2).
//!
//! Cutting a vignette decodes its whole page: tens of milliseconds of CPU for
//! a photograph, on every view of every card drawing it. The cut picture is
//! a function of the page's bytes, the rectangle and the size it is sent at
//! alone, so it is stored the first time it is cut and read back afterwards.
//!
//! It is derived data, never a source of truth. Its key names the vignette
//! and fingerprints everything the picture depends on — the page, the
//! page's content digest, the rectangle, the size and the version of the
//! cut — so a read computes the key from the rows as they are now and never
//! finds a picture of what they were: a moved rectangle or a replaced file
//! names another key. What can no longer be read is deleted: by the writes
//! that make it so, right after their commit ([`forget`]), and by the
//! maintenance sweep ([`sweep`]) for whatever a crash or a race left.
//!
//! Nothing here decides who may see a crop. Its readers, the vignette image
//! routes, check the viewer's access to the vignette and its page first,
//! exactly as they did when every view cut the page again. The same inputs
//! less the size make the crops' [`version`], which their addresses carry
//! (`docs/architecture.md` §4.3).

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::LazyLock;

use axum::body::Bytes;
use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::types::{Media, Vignette};
use oxidgene_db::repo::{MediaRepo, TreeRepo, VignetteRepo};
use sea_orm::ConnectionTrait;
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

use super::store::{MediaStore, crop_key, sha256_hex};
use super::thumbnail::Rendition;
use crate::error_contract::error_kind;

/// The version of the cut. Changing how a crop is cut, sized or encoded
/// bumps it: every crop cut the old way then has a key no read computes,
/// and the sweep deletes it.
const CUT_VERSION: u32 = 1;

/// How many first views of one key may cut at once in this process: one.
/// The others wait for it and read what it stored. Keys are spread over
/// this many locks, so two different crops rarely wait for each other.
const TURN_STRIPES: usize = 64;

/// One lock per stripe of keys; see [`TURN_STRIPES`].
static TURNS: LazyLock<Vec<Mutex<()>>> =
    LazyLock::new(|| (0..TURN_STRIPES).map(|_| Mutex::new(())).collect());

/// How many crops this process cuts at once, whatever the keys: each
/// decodes up to 128 MiB of pixels on the blocking pool.
static CUTS: LazyLock<Semaphore> = LazyLock::new(|| {
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    Semaphore::new(cores.clamp(1, 4))
});

/// The name a crop key gives `rendition`.
fn size_name(rendition: Rendition) -> &'static str {
    match rendition {
        Rendition::Card => "card",
        Rendition::Thumbnail => "thumbnail",
    }
}

/// The fingerprint of the crop of `vignette`, a region of page `medium`, at
/// `rendition`, as both rows are now: the SHA-256 of everything the picture
/// depends on, which reveals none of it. `None` for a page whose bytes have
/// no digest, which is a page we do not hold.
#[must_use]
pub fn fingerprint(medium: &Media, vignette: &Vignette, rendition: Rendition) -> Option<String> {
    let identity = identity(
        medium.id,
        medium.sha256.as_deref()?,
        (vignette.x, vignette.y, vignette.width, vignette.height),
    );
    Some(sha256_hex(
        format!("{identity}\n{}", size_name(rendition)).as_bytes(),
    ))
}

/// Everything a crop depends on but its size: the version of the cut, the
/// page's id and content digest, and the rectangle.
fn identity(page_id: Uuid, digest: &str, (x, y, width, height): (i32, i32, i32, i32)) -> String {
    format!("oxidgene-crop/{CUT_VERSION}\n{page_id}\n{digest}\n{x},{y},{width},{height}")
}

/// The version of the crops of a vignette with `rect` on page `page_id`
/// whose content digest is `digest`, at both sizes: the first digits of the
/// SHA-256 of what they depend on, which names none of it
/// (`docs/architecture.md` §4.3). A moved rectangle, a replaced page or a
/// new way of cutting is a new version.
#[must_use]
pub fn version(page_id: Uuid, digest: &str, rect: (i32, i32, i32, i32)) -> String {
    oxidgene_core::types::version_of(&sha256_hex(identity(page_id, digest, rect).as_bytes()))
}

/// [`version`] of `vignette`, a region of page `medium`, as both rows are
/// now; `None` for a page we do not hold.
#[must_use]
pub fn vignette_version(medium: &Media, vignette: &Vignette) -> Option<String> {
    Some(version(
        medium.id,
        medium.sha256.as_deref()?,
        (vignette.x, vignette.y, vignette.width, vignette.height),
    ))
}

/// The key of the crop of `vignette`, a region of page `medium`, at
/// `rendition`, as both rows are now; `None` as for [`fingerprint`].
#[must_use]
pub fn current_key(medium: &Media, vignette: &Vignette, rendition: Rendition) -> Option<String> {
    Some(crop_key(
        medium.tree_id,
        vignette.id,
        &fingerprint(medium, vignette, rendition)?,
        size_name(rendition),
    ))
}

/// A crop a request has just stored, to [`confirm`] once its reads are done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    tree_id: Uuid,
    vignette_id: Uuid,
    rendition: Rendition,
    key: String,
}

/// The crop of `vignette` out of `medium`, its held raster page, at
/// `rendition`: read back from the store when it was cut before, cut now and
/// stored otherwise — and then also returned as [`Written`], which the
/// caller hands to [`confirm`] with the others it stored.
///
/// The caller has checked that the viewer may see the vignette, and that
/// `medium` is its page, live and of its tree, held and raster.
pub async fn read_or_cut(
    store: &dyn MediaStore,
    medium: &Media,
    vignette: &Vignette,
    rendition: Rendition,
) -> Result<(Vec<u8>, Option<Written>), OxidGeneError> {
    let Some(storage_key) = medium.storage_key.as_deref() else {
        return Err(OxidGeneError::NotFound {
            entity: "Media file",
            id: medium.id,
        });
    };
    let rect = (vignette.x, vignette.y, vignette.width, vignette.height);
    let Some(key) = current_key(medium, vignette, rendition) else {
        let cut = super::cut_vignette(store, medium, storage_key, rect, rendition).await?;
        return Ok((cut, None));
    };
    if let Ok(stored) = store.get(&key).await {
        return Ok((stored, None));
    }
    let _turn = turn(&key).lock().await;
    // Whoever held the turn before may have stored it meanwhile.
    if let Ok(stored) = store.get(&key).await {
        return Ok((stored, None));
    }
    let cut = {
        let _permit = CUTS
            .acquire()
            .await
            .map_err(|_| OxidGeneError::Internal("crop cutting stopped".into()))?;
        super::cut_vignette(store, medium, storage_key, rect, rendition).await?
    };
    if let Err(error) = store.put_crop(&key, Bytes::copy_from_slice(&cut)).await {
        tracing::warn!(
            error = "crop_storage",
            error.kind = error_kind(&error),
            "could not store a cut vignette"
        );
        return Ok((cut, None));
    }
    let written = Written {
        tree_id: medium.tree_id,
        vignette_id: vignette.id,
        rendition,
        key,
    };
    Ok((cut, Some(written)))
}

/// The lock serializing the first views of `key`.
fn turn(key: &str) -> &'static Mutex<()> {
    let mut hasher = DefaultHasher::new();
    key.hash(&mut hasher);
    let stripe = usize::try_from(hasher.finish() % TURN_STRIPES as u64).unwrap_or_default();
    &TURNS[stripe]
}

/// Take back each crop of `written` that is no longer the current crop of
/// its vignette in a live tree: the vignette, its page or the tree may have
/// changed or gone between the read that named its key and its write, and
/// the deletion that followed the change then ran before the write.
///
/// Three statements whatever the number of crops, none when there are none.
/// A read that fails takes every crop back: a crop deleted for nothing is
/// only cut again.
pub async fn confirm(db: &impl ConnectionTrait, store: &dyn MediaStore, written: &[Written]) {
    if written.is_empty() {
        return;
    }
    let current = current_of(db, written).await.unwrap_or_default();
    for crop in written.iter().filter(|crop| !current.contains(&crop.key)) {
        if let Err(error) = store.delete(&crop.key).await {
            crop_deletion_failed(&error);
        }
    }
}

/// The keys of `written` that are the current crops of their vignettes, in
/// live trees.
async fn current_of(
    db: &impl ConnectionTrait,
    written: &[Written],
) -> Result<HashSet<String>, OxidGeneError> {
    let trees =
        TreeRepo::liveness(db, &sorted_unique(written.iter().map(|crop| crop.tree_id))).await?;
    let vignettes: HashMap<Uuid, Vignette> = VignetteRepo::get_many(
        db,
        &sorted_unique(written.iter().map(|crop| crop.vignette_id)),
    )
    .await?
    .into_iter()
    .map(|vignette| (vignette.id, vignette))
    .collect();
    let media: HashMap<Uuid, Media> = MediaRepo::get_many(
        db,
        &sorted_unique(vignettes.values().map(|vignette| vignette.media_id)),
    )
    .await?
    .into_iter()
    .map(|medium| (medium.id, medium))
    .collect();
    Ok(written
        .iter()
        .filter(|crop| trees.get(&crop.tree_id) == Some(&true))
        .filter_map(|crop| {
            let vignette = vignettes.get(&crop.vignette_id)?;
            let medium = media.get(&vignette.media_id)?;
            let current = current_key(medium, vignette, crop.rendition)?;
            (medium.tree_id == crop.tree_id && current == crop.key).then_some(current)
        })
        .collect())
}

/// Delete the stored crops of vignettes `vignette_ids` of tree `tree_id`,
/// once the write that deleted them, moved their rectangle or replaced
/// their page's file has committed.
///
/// Best effort: the write stands whatever happens here, a failure is
/// logged, and the maintenance sweep deletes what is left.
pub async fn forget(store: &dyn MediaStore, tree_id: Uuid, vignette_ids: &[Uuid]) {
    for vignette_id in vignette_ids {
        if let Err(error) = store.delete_crops(tree_id, Some(*vignette_id)).await {
            crop_deletion_failed(&error);
        }
    }
}

/// Log a crop the store would not delete: the kind of the failure, never
/// which crop.
fn crop_deletion_failed(error: &OxidGeneError) {
    tracing::warn!(
        error = "crop_deletion",
        error.kind = error_kind(error),
        "could not delete a stored crop; the maintenance sweep retries"
    );
}

/// Delete every stored crop no read can reach any more: those of a
/// soft-deleted tree; those of a tree this database does not hold, once its
/// id was minted before `unknown_before` (a store shared with another
/// database, or a database recreated beside its store); and in a live tree,
/// those of a vignette that is gone, and those whose key is not its
/// current one — cut before its rectangle moved or its page's file was
/// replaced, or cut the old way.
///
/// What the writes delete right after their commit, this deletes after a
/// crash, a failed deletion or a race. Deleting a crop that is still
/// current is harmless: it is cut again on its next view.
pub async fn sweep(
    db: &impl ConnectionTrait,
    store: &dyn MediaStore,
    unknown_before: chrono::DateTime<chrono::Utc>,
) -> Result<(), OxidGeneError> {
    let trees = store.tree_ids().await?;
    let liveness = TreeRepo::liveness(db, &trees).await?;
    for tree_id in trees {
        match liveness.get(&tree_id) {
            Some(true) => sweep_tree(db, store, tree_id).await?,
            Some(false) => store.delete_crops(tree_id, None).await?,
            None if crate::service::background_job::created_before(tree_id, unknown_before) => {
                store.delete_crops(tree_id, None).await?;
            }
            None => {}
        }
    }
    Ok(())
}

/// [`sweep`] in live tree `tree_id`.
async fn sweep_tree(
    db: &impl ConnectionTrait,
    store: &dyn MediaStore,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    // Listed before the rows are read: a crop cut meanwhile is not listed,
    // and one whose vignette changes meanwhile is stale anyway.
    let stored = store.crop_objects(tree_id).await?;
    if stored.is_empty() {
        return Ok(());
    }
    let vignette_ids = sorted_unique(stored.iter().map(|crop| crop.vignette_id));
    let vignettes: HashMap<Uuid, Vignette> = VignetteRepo::get_many(db, &vignette_ids)
        .await?
        .into_iter()
        .map(|vignette| (vignette.id, vignette))
        .collect();
    let media_ids = sorted_unique(vignettes.values().map(|vignette| vignette.media_id));
    let media: HashMap<Uuid, Media> = MediaRepo::get_many(db, &media_ids)
        .await?
        .into_iter()
        .filter(|medium| medium.tree_id == tree_id)
        .map(|medium| (medium.id, medium))
        .collect();
    let mut gone = HashSet::new();
    for crop in stored {
        let current = vignettes
            .get(&crop.vignette_id)
            .and_then(|vignette| Some((media.get(&vignette.media_id)?, vignette)));
        let Some((medium, vignette)) = current else {
            if gone.insert(crop.vignette_id) {
                store.delete_crops(tree_id, Some(crop.vignette_id)).await?;
            }
            continue;
        };
        let wanted = [Rendition::Card, Rendition::Thumbnail]
            .into_iter()
            .any(|rendition| {
                current_key(medium, vignette, rendition).as_deref() == Some(crop.key.as_str())
            });
        if !wanted {
            store.delete(&crop.key).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn medium(tree_id: Uuid, sha256: &str) -> Media {
        Media {
            id: Uuid::now_v7(),
            tree_id,
            file_name: "group.jpg".into(),
            mime_type: "image/jpeg".into(),
            file_path: "group.jpg".into(),
            storage_key: None,
            sha256: Some(sha256.into()),
            thumbnail_key: None,
            card_key: None,
            thumbnail_url: None,
            width: Some(640),
            height: Some(480),
            page_count: 1,
            parent_media_id: Some(Uuid::now_v7()),
            page_index: 0,
            file_size: 1,
            title: None,
            description: None,
            date_value: None,
            date_sort: None,
            date_qualifier: Default::default(),
            date_value2: None,
            calendar: Default::default(),
            privacy: Default::default(),
            source_media_type: Default::default(),
            tags: vec![],
            document_category: None,
            place_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        }
    }

    fn vignette(media_id: Uuid, rect: (i32, i32, i32, i32)) -> Vignette {
        Vignette {
            id: Uuid::now_v7(),
            media_id,
            x: rect.0,
            y: rect.1,
            width: rect.2,
            height: rect.3,
            person_id: None,
            event_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn the_key_changes_with_everything_the_picture_depends_on() {
        let tree_id = Uuid::now_v7();
        let page = medium(tree_id, &"a".repeat(64));
        let region = vignette(page.id, (10, 20, 30, 40));
        let key = current_key(&page, &region, Rendition::Card).unwrap();
        assert!(key.starts_with(&format!("{tree_id}/crops/{}/", region.id)));
        assert!(key.ends_with("-card.jpg"));
        assert_eq!(
            crate::media::store::crop_key_owner(&key),
            Some((tree_id, region.id))
        );

        let mut moved = region.clone();
        moved.x += 1;
        let mut replaced = page.clone();
        replaced.sha256 = Some("b".repeat(64));
        let mut other_page = page.clone();
        other_page.id = Uuid::now_v7();
        for other in [
            current_key(&page, &region, Rendition::Thumbnail),
            current_key(&page, &moved, Rendition::Card),
            current_key(&replaced, &region, Rendition::Card),
            current_key(&other_page, &region, Rendition::Card),
        ] {
            assert_ne!(other.unwrap(), key);
        }
        // Stable: the same rows always name the same key.
        assert_eq!(current_key(&page, &region, Rendition::Card).unwrap(), key);

        let mut unheld = page;
        unheld.sha256 = None;
        assert_eq!(current_key(&unheld, &region, Rendition::Card), None);
    }
}
