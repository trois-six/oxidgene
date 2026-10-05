//! Media writes — documents, their pages, metadata, tags and definitive
//! deletion — and the checks shared by the downloads: the same steps whether
//! REST or GraphQL asked.
//!
//! Every write runs in one transaction with its audit entry, so a failing
//! history record never leaves an unaudited write behind. A write that
//! changes what a person's card draws — the picture representing them, how
//! many media they have — rewrites their projection in that transaction too.
//! Stored bytes leave the store only once the transaction holding their rows'
//! deletion has committed: a key deleted before the commit would be a file
//! gone from a page the database still lists.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::{AuditAction, AuditEntity};
use oxidgene_core::types::{Media, normalize_mime};
use oxidgene_core::{Calendar, DateQualifier};
use oxidgene_db::repo::{MediaLinkRepo, MediaPatch, MediaRepo, MediaTagRepo, PersonRepo};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Deserialize;
use uuid::Uuid;

use crate::media::{self, MediaStore};
use crate::profile::ProfileService;
use crate::service::event_date;
use crate::service::history::Change;
use crate::service::media_library::normalize_tag;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A page that names a file without holding its bytes — a URL an archive
/// serves, or a path a GEDCOM mentioned.
#[derive(Debug, Deserialize)]
pub struct NewPage {
    /// The document this becomes a page of.
    ///
    /// Required: bytes and URLs live on pages, and a page always belongs to a
    /// document. The document is created first.
    pub document_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    /// For a page held as an `http(s)` URL, the address of a small picture of
    /// it its server also serves, which gallery tiles draw instead of the full
    /// picture.
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    /// The picture's pixel size, when the client already knows it — an
    /// archive's image service states it. Sent together or not at all.
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,
}

/// Uploaded bytes, and where they go.
pub struct NewUpload {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub target: UploadTarget,
}

/// Where uploaded bytes go.
#[derive(Debug, Clone, Copy)]
pub enum UploadTarget {
    /// A new page of this document.
    NewPage { document_id: Uuid },
    /// This existing page, which had no bytes — the flow for filling in a
    /// GEDCOM-imported page that names a photograph nobody had.
    Attach { media_id: Uuid },
}

/// The metadata a media update changes: `None` keeps a field, and
/// `Some(None)` clears an optional one.
///
/// A media carries the same descriptive fields a fact does — a date with its
/// qualifier and calendar, a place, a description — because "a photograph
/// taken around 1890 at Nantes" is the same kind of statement as an event.
/// There is deliberately no source field: a media *is* a source document.
/// `date_sort` is absent on purpose: it is derived from `calendar` and
/// `date_value`, exactly as for an event.
#[derive(Debug, Default, Deserialize)]
pub struct MediaUpdate {
    #[serde(default, deserialize_with = "double_option")]
    pub title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value2: Option<Option<String>>,
    pub date_qualifier: Option<DateQualifier>,
    pub calendar: Option<Calendar>,
    #[serde(default, deserialize_with = "double_option")]
    pub place_id: Option<Option<Uuid>>,
    /// Where the file is. For a remote media this is the URL, and editing it
    /// is how a broken link gets fixed. Refused for a media whose bytes we
    /// hold — there `file_path` is the GEDCOM value an export writes back.
    pub file_path: Option<String>,
    /// Only meaningful alongside a `file_path` we cannot sniff. Left out, the
    /// type is guessed from the URL's extension.
    pub mime_type: Option<String>,
    /// The picture's pixel size, sent together or not at all, and only for a
    /// page we do not hold: the client that displayed it is the only witness
    /// to its size. For our own bytes the size is decoded from them.
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// A remote page's thumbnail address, an `http(s)` URL; `null` clears
    /// it. Repointing the page at another address without sending one clears
    /// it too, since it pictured the old address.
    #[serde(default, deserialize_with = "double_option")]
    pub thumbnail_url: Option<Option<String>>,
    /// Whether this is shown when the tree is published. Recorded now,
    /// enforced when authentication lands.
    pub privacy: Option<oxidgene_core::enums::Privacy>,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    pub source_media_type: Option<oxidgene_core::enums::SourceMediaType>,
    /// What kind of record it is; clearing it is meaningful.
    #[serde(default, deserialize_with = "double_option")]
    pub document_category: Option<Option<oxidgene_core::enums::DocumentCategory>>,
}

/// Add a page naming a file we do not hold to a document of `tree_id`.
pub async fn create_page(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewPage,
) -> Result<Media, OxidGeneError> {
    if new.file_name.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "file_name must not be empty".to_string(),
        ));
    }
    // The caller's MIME type is a claim, not evidence: normalised, so every
    // row has a type worth believing and no reader has to second-guess one.
    let mime_type = normalize_mime(
        Some(&new.mime_type),
        if new.file_path.is_empty() {
            &new.file_name
        } else {
            &new.file_path
        },
    );
    let thumbnail_url = thumbnail_address(&new.file_path, new.thumbnail_url)?;
    let dimensions = page_dimensions(new.width, new.height)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, new.document_id).await?;
    // `create` counts the page into its document's `page_count`.
    let mut page = MediaRepo::create(
        &txn,
        id,
        tree_id,
        Some(new.document_id),
        new.file_name,
        mime_type,
        new.file_path,
        new.file_size,
        new.title,
        new.description,
    )
    .await?;
    if thumbnail_url.is_some() || dimensions.is_some() {
        page = MediaRepo::update(
            &txn,
            id,
            MediaPatch {
                thumbnail_url: thumbnail_url.map(Some),
                dimensions,
                ..MediaPatch::default()
            },
        )
        .await?;
    }
    refresh_showing(&txn, profiles, tree_id, new.document_id).await?;
    Change::create(tree_id, AuditEntity::MediaPage, id)
        .media(new.document_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(page)
}

/// Store uploaded bytes in `tree_id`, as a new page or into an existing one.
///
/// The bytes are stored before the transaction: they are content-addressed,
/// so a failed write leaves at worst an unreferenced object, never a row
/// pointing at nothing. Returns the page, and whether it was created.
pub async fn upload(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    store: &dyn MediaStore,
    tree_id: Uuid,
    upload: NewUpload,
) -> Result<(Media, bool), OxidGeneError> {
    let ingested = media::ingest(store, tree_id, &upload.file_name, upload.bytes).await?;
    let uploaded = ingested.into_upload(upload.title, upload.description);
    let txn = begin_tx(db).await?;
    let (page, change) = match upload.target {
        UploadTarget::Attach { media_id } => {
            require_tree_resource(&txn, tree_id, TreeResource::Media, media_id).await?;
            let page = MediaRepo::attach_file(&txn, media_id, uploaded).await?;
            let change = Change::update(tree_id, AuditEntity::Media, page.id);
            (page, change)
        }
        UploadTarget::NewPage { document_id } => {
            require_tree_resource(&txn, tree_id, TreeResource::Media, document_id).await?;
            // Born attached: a page belongs to its document from the moment
            // it lands, so an upload never sits in the tree as a scan
            // belonging to nothing.
            let page = MediaRepo::create_uploaded(
                &txn,
                Uuid::now_v7(),
                tree_id,
                Some(document_id),
                uploaded,
            )
            .await?;
            let change = Change::create(tree_id, AuditEntity::MediaPage, page.id);
            (page, change)
        }
    };
    let document_id = page.parent_media_id.unwrap_or(page.id);
    refresh_showing(&txn, profiles, tree_id, document_id).await?;
    change.media(document_id).record_unversioned(&txn).await?;
    commit_tx(txn).await?;
    Ok((page, matches!(upload.target, UploadTarget::NewPage { .. })))
}

/// Create an empty multi-page document in `tree_id`.
pub async fn create_document(
    db: &DatabaseConnection,
    tree_id: Uuid,
    title: Option<String>,
) -> Result<Media, OxidGeneError> {
    let txn = begin_tx(db).await?;
    let document =
        MediaRepo::create_document(&txn, Uuid::now_v7(), tree_id, title, chrono::Utc::now())
            .await?;
    Change::create(tree_id, AuditEntity::Media, document.id)
        .media(document.id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(document)
}

/// Update the metadata of media `id` of `tree_id`.
pub async fn update_media(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    update: MediaUpdate,
) -> Result<Media, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, id).await?;
    if let Some(Some(place_id)) = update.place_id {
        require_tree_resource(&txn, tree_id, TreeResource::Place, place_id).await?;
    }
    let stored = MediaRepo::get(&txn, id).await?;
    let patch = media_patch(&stored, update)?;
    let updated = MediaRepo::update(&txn, id, patch).await?;
    // The title, path and type are what a card draws from.
    refresh_showing(&txn, profiles, tree_id, id).await?;
    Change::update(tree_id, AuditEntity::Media, id)
        .media(id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(updated)
}

/// Turn an update into a repository patch, deriving what the client may not
/// set and refusing what it may not change.
fn media_patch(stored: &Media, update: MediaUpdate) -> Result<MediaPatch, OxidGeneError> {
    let (file_path, mime_type) = file_change(stored, update.file_path, update.mime_type)?;
    let dimensions = dimensions(stored, update.width, update.height)?;
    let thumbnail_url = thumbnail_change(stored, file_path.as_deref(), update.thumbnail_url)?;
    // The calendar and the value are only meaningful together, so a patch that
    // moves one re-reads the other from the stored row before converting.
    let date_sort = Some(event_date::derive_patch(
        stored.calendar,
        stored.date_value.as_deref(),
        update.calendar,
        update.date_value.as_ref().map(|v| v.as_deref()),
    ));
    Ok(MediaPatch {
        title: update.title,
        description: update.description,
        date_value: update.date_value,
        date_value2: update.date_value2,
        date_qualifier: update.date_qualifier,
        calendar: update.calendar,
        place_id: update.place_id,
        file_path,
        mime_type,
        dimensions,
        thumbnail_url,
        privacy: update.privacy,
        source_media_type: update.source_media_type,
        document_category: update.document_category,
        date_sort,
    })
}

/// The path and type an update moves a media to.
///
/// A media is one of three things, and only one of them owns its path:
///
/// - stored: we hold the bytes. `file_path` is the GEDCOM value an export
///   writes back; repointing it would make the export describe a file we are
///   not serving, and relabelling the type would let a crafted image be
///   served as a page.
/// - remote: `file_path` is an http(s) URL, the bytes are somebody else's,
///   and editing it is how a dead link gets fixed.
/// - unheld: a GEDCOM record naming a local file nobody uploaded. Editing
///   the path is how it gets pointed at a URL instead.
fn file_change(
    stored: &Media,
    file_path: Option<String>,
    mime_type: Option<String>,
) -> Result<(Option<String>, Option<String>), OxidGeneError> {
    let held = stored.storage_key.is_some();
    let mime_type = mime_type
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let Some(requested) = file_path else {
        if held && mime_type.is_some() {
            return Err(OxidGeneError::Validation(
                "the type of a file stored here is read from its bytes".into(),
            ));
        }
        return Ok((None, mime_type));
    };
    if held {
        return Err(OxidGeneError::Validation(
            "cannot repoint a media whose file is stored here; upload a replacement instead".into(),
        ));
    }
    let requested = requested.trim().to_string();
    if requested.is_empty() {
        return Err(OxidGeneError::Validation(
            "file_path must not be empty".into(),
        ));
    }
    // No sniffing is possible for a URL — fetching it is exactly what a
    // remote media exists to avoid — so the extension is the only evidence.
    let mime_type = mime_type.or_else(|| media::guess_mime(&requested).map(str::to_string));
    Ok((Some(requested), mime_type))
}

/// The thumbnail address an update gives a page.
///
/// Only a page held as an `http(s)` URL has one, and it must be an `http(s)`
/// URL itself: it is drawn by the reader's browser, never fetched here. A
/// blank address clears it, and so does repointing the page at another
/// address without naming a new thumbnail, since the old one pictured the
/// old address.
fn thumbnail_change(
    stored: &Media,
    file_path: Option<&str>,
    requested: Option<Option<String>>,
) -> Result<Option<Option<String>>, OxidGeneError> {
    let requested = requested.map(|url| {
        url.map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
    });
    match requested {
        Some(Some(url)) => {
            if stored.is_document() || stored.storage_key.is_some() {
                return Err(OxidGeneError::Validation(
                    "only a page held as a URL has a thumbnail address".into(),
                ));
            }
            let path = file_path.unwrap_or(&stored.file_path);
            thumbnail_address(path, Some(url)).map(Some)
        }
        Some(None) => Ok(Some(None)),
        None if file_path.is_some() && stored.thumbnail_url.is_some() => Ok(Some(None)),
        None => Ok(None),
    }
}

/// A new page's thumbnail address, checked: an `http(s)` URL, for a page
/// whose own path is one. A blank address is none.
fn thumbnail_address(
    file_path: &str,
    thumbnail_url: Option<String>,
) -> Result<Option<String>, OxidGeneError> {
    let Some(url) = thumbnail_url
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
    else {
        return Ok(None);
    };
    if !oxidgene_core::types::is_remote_url(file_path) {
        return Err(OxidGeneError::Validation(
            "only a page held as a URL has a thumbnail address".into(),
        ));
    }
    if !oxidgene_core::types::is_remote_url(&url) {
        return Err(OxidGeneError::Validation(
            "thumbnail_url must be an http or https URL".into(),
        ));
    }
    Ok(Some(url))
}

/// A new page's pixel size: both sides, positive, or none.
fn page_dimensions(
    width: Option<i32>,
    height: Option<i32>,
) -> Result<Option<(i32, i32)>, OxidGeneError> {
    match (width, height) {
        (None, None) => Ok(None),
        (Some(width), Some(height)) if width > 0 && height > 0 => Ok(Some((width, height))),
        (Some(_), Some(_)) => Err(OxidGeneError::Validation(
            "image dimensions must be positive".into(),
        )),
        _ => Err(OxidGeneError::Validation(
            "width and height are sent together or not at all".into(),
        )),
    }
}

/// The pixel size an update gives a media. Half a size is not a size, so
/// the pair is required rather than half-applied.
fn dimensions(
    stored: &Media,
    width: Option<i32>,
    height: Option<i32>,
) -> Result<Option<(i32, i32)>, OxidGeneError> {
    match (width, height) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) if stored.storage_key.is_some() => Err(OxidGeneError::Validation(
            "the dimensions of a file stored here are read from its bytes".into(),
        )),
        (Some(width), Some(height)) if width <= 0 || height <= 0 => Err(OxidGeneError::Validation(
            "image dimensions must be positive".into(),
        )),
        (Some(width), Some(height)) => Ok(Some((width, height))),
        _ => Err(OxidGeneError::Validation(
            "width and height are sent together or not at all".into(),
        )),
    }
}

/// Add a tag to media `id` of `tree_id` — to its document when it is a page,
/// since tags describe documents — without replacing the others. Returns the
/// tagged document.
pub async fn add_tag(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    tag: &str,
) -> Result<Media, OxidGeneError> {
    let (tag, normalized) = normalize_tag(tag)
        .ok_or_else(|| OxidGeneError::Validation("tag must not be empty".into()))?;
    let txn = begin_tx(db).await?;
    let document_id = tagged_document(&txn, tree_id, id).await?;
    MediaTagRepo::create(&txn, document_id, tag, normalized).await?;
    Change::new(tree_id, AuditAction::Create, AuditEntity::MediaTag, None)
        .media(document_id)
        .record_unversioned(&txn)
        .await?;
    let document = MediaRepo::get(&txn, document_id).await?;
    commit_tx(txn).await?;
    Ok(document)
}

/// Remove a tag from media `id` of `tree_id` (from its document when it is a
/// page), leaving the others.
pub async fn remove_tag(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    tag: &str,
) -> Result<(), OxidGeneError> {
    let (_, normalized) = normalize_tag(tag)
        .ok_or_else(|| OxidGeneError::Validation("tag must not be empty".into()))?;
    let txn = begin_tx(db).await?;
    let document_id = tagged_document(&txn, tree_id, id).await?;
    MediaTagRepo::delete(&txn, document_id, &normalized).await?;
    Change::new(tree_id, AuditAction::Delete, AuditEntity::MediaTag, None)
        .media(document_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await
}

/// The document whose tags a tag write on media `id` of `tree_id` changes.
async fn tagged_document(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    id: Uuid,
) -> Result<Uuid, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Media, id).await?;
    let media = MediaRepo::get(db, id).await?;
    Ok(media.parent_media_id.unwrap_or(media.id))
}

/// Set the page order of document `document_id` of `tree_id`. The list must
/// name exactly its pages, once each.
pub async fn reorder_pages(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    document_id: Uuid,
    page_ids: &[Uuid],
) -> Result<Vec<Media>, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, document_id).await?;
    for page_id in page_ids {
        require_tree_resource(&txn, tree_id, TreeResource::Media, *page_id).await?;
    }
    let pages = MediaRepo::reorder_pages(&txn, document_id, page_ids).await?;
    // The first page is what a card draws for the document.
    refresh_showing(&txn, profiles, tree_id, document_id).await?;
    Change::new(tree_id, AuditAction::Update, AuditEntity::MediaPage, None)
        .media(document_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(pages)
}

/// Remove page `page_id` from document `document_id` of `tree_id`,
/// permanently, with its bytes, transcript, links, crops and portraits.
/// Removing the last page leaves the document standing and empty.
pub async fn delete_page(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    store: &dyn MediaStore,
    tree_id: Uuid,
    document_id: Uuid,
    page_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, document_id).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, page_id).await?;
    // Read before the page and its links go.
    let affected = persons_showing(&txn, document_id).await?;
    let purge = MediaRepo::delete_page(&txn, document_id, page_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::delete(tree_id, AuditEntity::MediaPage, page_id)
        .media(document_id)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    for key in purge.storage_keys {
        store.delete(&key).await?;
    }
    Ok(())
}

/// Delete media `id` of `tree_id` permanently, with its pages, related rows
/// and the stored bytes nothing else shares; whether it was deleted.
///
/// Given a gallery link, the deletion is a conditional cleanup: the link is
/// allowed, but any other link, crop or portrait keeps the media, and `false`
/// says so.
pub async fn delete_media(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    store: &dyn MediaStore,
    tree_id: Uuid,
    id: Uuid,
    allowed_link_id: Option<Uuid>,
) -> Result<bool, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, id).await?;
    if let Some(link_id) = allowed_link_id {
        require_tree_resource(&txn, tree_id, TreeResource::MediaLink, link_id).await?;
    }
    // Read before the purge removes what they come from.
    let label = MediaRepo::get(&txn, id).await?.display_label();
    let affected = persons_showing(&txn, id).await?;
    let purge = match allowed_link_id {
        Some(link_id) => MediaRepo::purge_if_unreferenced_elsewhere(&txn, id, link_id).await?,
        None => Some(MediaRepo::purge(&txn, id).await?),
    };
    let Some(purge) = purge else {
        return Ok(false);
    };
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::delete(tree_id, AuditEntity::Media, id)
        .media(id)
        .label(label)
        .record_unversioned(&txn)
        .await?;
    commit_tx(txn).await?;
    for key in purge.storage_keys {
        store.delete(&key).await?;
    }
    Ok(true)
}

/// Whether the gallery link `allowed_link_id` is media `id`'s sole reference
/// in `tree_id`, so that deleting the media from that gallery loses nothing
/// else.
pub async fn can_delete_media(
    db: &DatabaseConnection,
    tree_id: Uuid,
    id: Uuid,
    allowed_link_id: Uuid,
) -> Result<bool, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Media, id).await?;
    require_tree_resource(db, tree_id, TreeResource::MediaLink, allowed_link_id).await?;
    MediaRepo::can_purge_if_unreferenced_elsewhere(db, id, allowed_link_id).await
}

/// Rewrite the projections of the persons showing media `media_id`.
pub(crate) async fn refresh_showing(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<(), OxidGeneError> {
    let affected = persons_showing(db, media_id).await?;
    profiles
        .invalidate_for_mutation(db, tree_id, &affected)
        .await
}

/// The persons whose projection draws from media `media_id`: those linked to
/// it, and those whose chosen portrait is it or a crop of it. A document and
/// its pages count as one media — a card draws a document's first page.
pub(crate) async fn persons_showing(
    db: &impl ConnectionTrait,
    media_id: Uuid,
) -> Result<Vec<Uuid>, OxidGeneError> {
    let media = MediaRepo::get(db, media_id).await?;
    let document_id = media.parent_media_id.unwrap_or(media.id);
    let mut media_ids: Vec<Uuid> = MediaRepo::list_pages(db, document_id)
        .await?
        .into_iter()
        .map(|page| page.id)
        .collect();
    media_ids.push(document_id);
    let mut persons: Vec<Uuid> = MediaLinkRepo::list_by_medias(db, &media_ids)
        .await?
        .into_iter()
        .filter_map(|link| link.person_id)
        .collect();
    persons.extend(PersonRepo::portrayed_by(db, &media_ids).await?);
    persons.sort_unstable();
    persons.dedup();
    Ok(persons)
}

/// Download reads must not outlive a soft-deleted tree or parent document,
/// even while the asynchronous purge has not removed the page rows yet.
pub async fn download_record(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<Media, OxidGeneError> {
    crate::service::scope::require_live_tree(db, tree_id).await?;
    require_tree_resource(db, tree_id, TreeResource::Media, media_id).await?;
    let media = MediaRepo::get(db, media_id).await?;
    if let Some(parent) = media.parent_media_id {
        require_tree_resource(db, tree_id, TreeResource::Media, parent).await?;
    }
    Ok(media)
}

/// A complete document and its pages, checked for a download: every page
/// must be in the tree, and every page we hold must have its bytes.
pub async fn archive_pages(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<(Media, Vec<Media>), OxidGeneError> {
    let document = download_record(db, tree_id, media_id).await?;
    let pages = MediaRepo::list_pages(db, media_id).await?;
    let missing = OxidGeneError::NotFound {
        entity: "Media file",
        id: media_id,
    };
    if pages.is_empty() || pages.iter().any(|page| page.tree_id != tree_id) {
        return Err(missing);
    }
    for page in &pages {
        // A page we never received has no bytes to pack, but it does have
        // the one thing we ever held about it: it travels as a shortcut.
        if !oxidgene_core::types::is_remote_url(&page.file_path) {
            stored_key(page)?;
        }
    }
    Ok((document, pages))
}

/// The storage key of a media whose bytes we hold, or a `NotFound`.
///
/// A row with no key is a file we know the name of and not the content —
/// every GEDCOM import produces those — so "not found" is accurate: there are
/// no bytes to serve. A key outside the media's tree is never served.
pub fn stored_key(media: &Media) -> Result<&str, OxidGeneError> {
    media
        .storage_key
        .as_deref()
        .filter(|key| key.starts_with(&format!("{}/", media.tree_id)))
        .ok_or(OxidGeneError::NotFound {
            entity: "Media file",
            id: media.id,
        })
}
