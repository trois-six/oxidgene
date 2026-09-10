//! REST handlers for Media CRUD, upload and file serving.

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::TryStreamExt;
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::Media;
use oxidgene_db::repo::{
    MediaPatch, MediaRepo, MediaTagRepo, PaginationParams, TreeRepo, UploadedMedia,
};
use uuid::Uuid;

use crate::media::{self, MAX_UPLOAD_BYTES};
use crate::service::event_date;

use super::dto::{
    CreateDocumentRequest, CreateMediaRequest, DeleteMediaQuery, GalleryBundleRequest,
    MediaDeletionStatusQuery, MediaTagRequest, PaginationQuery, ReorderPagesRequest,
    UpdateMediaRequest,
};
use super::error::ApiError;
use super::state::{AppState, TreeResource, begin_tx, commit_tx, require_tree_resource};

/// POST /api/v1/trees/:tree_id/gallery-bundle
pub async fn gallery_bundle(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<GalleryBundleRequest>,
) -> Result<Json<crate::service::gallery::GalleryBundle>, ApiError> {
    let bundle = crate::service::gallery::load_gallery_bundle(
        &state.db,
        tree_id,
        &body.media_ids,
        &body.vignette_ids,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(bundle))
}

/// GET /api/v1/trees/:tree_id/media
pub async fn list_media(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = MediaRepo::list(&state.db, tree_id, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/media
///
/// Add a page that names a file without holding its bytes — a URL an archive
/// serves, or a path a GEDCOM mentioned. The bytes-carrying counterpart is
/// `POST /media/upload`; both add a page to an existing document.
pub async fn create_media(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateMediaRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if body.file_name.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "file_name must not be empty".to_string(),
        )));
    }
    let id = Uuid::now_v7();
    // The last write path that could still store `application/octet-stream`:
    // this one takes the MIME type from the caller. Upload sniffs the bytes,
    // GEDCOM import reads the `FORM` or the file name, and repointing at a URL
    // guesses from its extension — normalising here means every row in the
    // table has a MIME type worth believing, so no reader has to second-guess
    // one.
    let mime_type = oxidgene_core::types::normalize_mime(
        Some(&body.mime_type),
        if body.file_path.is_empty() {
            &body.file_name
        } else {
            &body.file_path
        },
    );
    require_tree_resource(&state.db, tree_id, TreeResource::Media, body.document_id)
        .await
        .map_err(ApiError)?;
    let media = MediaRepo::create(
        &state.db,
        id,
        tree_id,
        Some(body.document_id),
        body.file_name,
        mime_type,
        body.file_path,
        body.file_size,
        body.title,
        body.description,
    )
    .await
    .map_err(ApiError::from)?;
    MediaRepo::refresh_page_count(&state.db, body.document_id)
        .await
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(media).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/media/:media_id
pub async fn get_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(media).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/media/:media_id
pub async fn update_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateMediaRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let stored = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    let patch = media_patch(&stored, body)?;
    let media = MediaRepo::update(&state.db, media_id, patch)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(media).unwrap()))
}

/// Turn an update request into a repo patch, deriving what the client may not
/// set and rejecting what it may not change.
pub(crate) fn media_patch(
    stored: &Media,
    body: UpdateMediaRequest,
) -> Result<MediaPatch, ApiError> {
    // A media is one of three things, and only one of them owns its path.
    //
    //  - stored:  we hold the bytes (`storage_key` set). `file_path` is the
    //             GEDCOM value an export writes back; repointing it would make
    //             the export describe a file we are not serving.
    //  - remote:  `file_path` is an http(s) URL, the bytes are somebody else's,
    //             and editing it is how a dead link gets fixed.
    //  - unheld:  a GEDCOM record naming a local file nobody uploaded. Editing
    //             the path is how it gets pointed at a URL instead.
    let mut file_path = None;
    let mut mime_type = None;
    if let Some(requested) = body.file_path {
        let requested = requested.trim().to_string();
        if stored.storage_key.is_some() {
            return Err(ApiError(OxidGeneError::Validation(
                "cannot repoint a media whose file is stored here; upload a replacement instead"
                    .into(),
            )));
        }
        if requested.is_empty() {
            return Err(ApiError(OxidGeneError::Validation(
                "file_path must not be empty".into(),
            )));
        }
        // No sniffing is possible for a URL — fetching it is exactly what a
        // remote media exists to avoid — so the extension is the only evidence
        // there is. It decides whether the profile embeds the media or offers
        // it as a download, so a wrong guess costs a click.
        mime_type = body
            .mime_type
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .or_else(|| media::guess_mime(&requested).map(str::to_string));
        file_path = Some(requested);
    } else if let Some(requested) = body.mime_type {
        let requested = requested.trim().to_string();
        if !requested.is_empty() {
            mime_type = Some(requested);
        }
    }

    // Half a size is not a size: a width without a height cannot scale
    // anything, so the pair is required rather than half-applied.
    let dimensions = match (body.width, body.height) {
        (None, None) => None,
        (Some(width), Some(height)) => {
            if stored.storage_key.is_some() {
                return Err(ApiError(OxidGeneError::Validation(
                    "the dimensions of a file stored here are read from its bytes".into(),
                )));
            }
            if width <= 0 || height <= 0 {
                return Err(ApiError(OxidGeneError::Validation(
                    "image dimensions must be positive".into(),
                )));
            }
            Some((width, height))
        }
        _ => {
            return Err(ApiError(OxidGeneError::Validation(
                "width and height are sent together or not at all".into(),
            )));
        }
    };

    // The calendar and the value are only meaningful together, so a patch that
    // moves one re-reads the other from the stored row before converting.
    let date_sort = Some(event_date::derive_patch(
        stored.calendar,
        stored.date_value.as_deref(),
        body.calendar,
        body.date_value.as_ref().map(|v| v.as_deref()),
    ));

    Ok(MediaPatch {
        title: body.title,
        description: body.description,
        date_value: body.date_value,
        date_value2: body.date_value2,
        date_qualifier: body.date_qualifier,
        calendar: body.calendar,
        place_id: body.place_id,
        file_path,
        mime_type,
        dimensions,
        privacy: body.privacy,
        source_media_type: body.source_media_type,
        document_category: body.document_category,
        date_sort,
    })
}

/// Normalize a label once, so both REST and GraphQL use the same identity.
pub(crate) fn normalize_tag(tag: String) -> Option<(String, String)> {
    let tag = tag.trim().to_string();
    (!tag.is_empty()).then(|| (tag.clone(), tag.to_lowercase()))
}

/// POST /api/v1/trees/:tree_id/media/:media_id/tags
pub async fn add_tag(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MediaTagRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    let (tag, normalized_tag) = normalize_tag(body.tag)
        .ok_or_else(|| ApiError(OxidGeneError::Validation("tag must not be empty".into())))?;
    let target_id = media.parent_media_id.unwrap_or(media.id);
    MediaTagRepo::create(&state.db, target_id, tag, normalized_tag)
        .await
        .map_err(ApiError::from)?;
    let media = MediaRepo::get(&state.db, target_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(media).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/media/:media_id/tags
pub async fn remove_tag(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MediaTagRequest>,
) -> Result<StatusCode, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    let (_, normalized_tag) = normalize_tag(body.tag)
        .ok_or_else(|| ApiError(OxidGeneError::Validation("tag must not be empty".into())))?;
    MediaTagRepo::delete(
        &state.db,
        media.parent_media_id.unwrap_or(media.id),
        &normalized_tag,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/trees/:tree_id/media/document
///
/// Create an empty multi-page document. Pages are added by uploading images
/// with a `document_id` part.
pub async fn create_document(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let media = MediaRepo::create_document(
        &state.db,
        Uuid::now_v7(),
        tree_id,
        body.title,
        chrono::Utc::now(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(media).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/media/:media_id/pages
pub async fn list_pages(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let pages = MediaRepo::list_pages(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(pages).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/media/:media_id/pages
///
/// Set the page order. The body lists exactly this document's pages, once
/// each — a partial list is refused rather than guessed at.
pub async fn reorder_pages(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReorderPagesRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    for page_id in &body.page_ids {
        require_tree_resource(&state.db, tree_id, TreeResource::Media, *page_id)
            .await
            .map_err(ApiError)?;
    }
    let pages = MediaRepo::reorder_pages(&state.db, media_id, &body.page_ids)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(pages).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/media/:media_id/pages/:page_id
///
/// Remove a page from its document, permanently, along with its bytes,
/// transcript, links, identifications and portrait references. Removing the
/// last page leaves the document standing and empty; deleting the document is
/// a separate act.
pub async fn delete_page(
    State(state): State<AppState>,
    Path((tree_id, media_id, page_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError::from)?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Media, page_id)
        .await
        .map_err(ApiError)?;
    let purge = MediaRepo::delete_page(&txn, media_id, page_id)
        .await
        .map_err(ApiError::from)?;
    commit_tx(txn).await.map_err(ApiError::from)?;
    // Only once the transaction holds: a key deleted before the commit is a
    // file gone from a page the database would still list.
    for key in purge.storage_keys {
        state.media.delete(&key).await.map_err(ApiError::from)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/trees/:tree_id/media/:media_id
///
/// Permanently deletes the record, its related data and unshared stored bytes.
///
/// `only_if_unreferenced_elsewhere` protects a gallery's context-menu cleanup:
/// the gallery link is allowed, but any other link, crop or portrait retains
/// the media. A `204` means deleted; `200` means it is still referenced.
pub async fn delete_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteMediaQuery>,
) -> Result<StatusCode, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let allowed_link_id = if query.only_if_unreferenced_elsewhere {
        let link_id = query.allowed_link_id.ok_or_else(|| {
            ApiError(OxidGeneError::Validation(
                "allowed_link_id is required for conditional media deletion".into(),
            ))
        })?;
        require_tree_resource(&state.db, tree_id, TreeResource::MediaLink, link_id)
            .await
            .map_err(ApiError)?;
        Some(link_id)
    } else {
        None
    };
    let deleted = crate::service::media::purge_media(
        &state.db,
        state.media.as_ref(),
        media_id,
        allowed_link_id,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(if deleted {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::OK
    })
}

/// GET /api/v1/trees/:tree_id/media/:media_id/deletion-status
///
/// Reports whether the current gallery link is the media's sole external
/// reference. The UI calls this before asking for definitive deletion.
pub async fn media_deletion_status(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<MediaDeletionStatusQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    require_tree_resource(
        &state.db,
        tree_id,
        TreeResource::MediaLink,
        query.allowed_link_id,
    )
    .await
    .map_err(ApiError)?;
    let can_delete =
        MediaRepo::can_purge_if_unreferenced_elsewhere(&state.db, media_id, query.allowed_link_id)
            .await
            .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "can_delete": can_delete })))
}

/// POST /api/v1/trees/:tree_id/media/upload
///
/// Multipart form. The `file` part carries the bytes and the `document_id`
/// part names the document the file becomes a page of; optional `title` and
/// `description` parts carry metadata. Sending a `media_id` part attaches the
/// bytes to an existing page instead of creating one — the flow for filling in
/// a GEDCOM-imported page that names a photo nobody had.
pub async fn upload_media(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let form = read_upload_form(multipart).await?;
    let Some((file_name, bytes)) = form.file else {
        return Err(ApiError(OxidGeneError::Validation(
            "multipart form has no `file` part".into(),
        )));
    };

    let ingested = media::ingest(&*state.media, tree_id, &file_name, bytes).await?;
    let upload = UploadedMedia {
        file_name: ingested.file_name,
        mime_type: ingested.mime_type,
        storage_key: ingested.storage_key,
        sha256: ingested.sha256,
        file_size: ingested.file_size,
        thumbnail_key: ingested.thumbnail_key,
        width: ingested.width,
        height: ingested.height,
        page_count: ingested.page_count,
        title: form.title,
        description: form.description,
        created_at: chrono::Utc::now(),
        metadata: Default::default(),
    };

    let (status, media) = match form.media_id {
        Some(media_id) => {
            require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
                .await
                .map_err(ApiError)?;
            (
                StatusCode::OK,
                MediaRepo::attach_file(&state.db, media_id, upload)
                    .await
                    .map_err(ApiError::from)?,
            )
        }
        None => {
            // A page belongs to its document from the moment it lands. Born
            // attached rather than created loose and adopted afterwards: an
            // upload that succeeded but was not attached would sit in the tree
            // as a scan belonging to nothing, which is not a shape any reader
            // knows how to show.
            let document_id = form.document_id.ok_or_else(|| {
                ApiError(OxidGeneError::Validation(
                    "multipart form has no `document_id` part".into(),
                ))
            })?;
            require_tree_resource(&state.db, tree_id, TreeResource::Media, document_id)
                .await
                .map_err(ApiError)?;
            (
                StatusCode::CREATED,
                MediaRepo::create_uploaded(
                    &state.db,
                    Uuid::now_v7(),
                    tree_id,
                    Some(document_id),
                    upload,
                )
                .await
                .map_err(ApiError::from)?,
            )
        }
    };
    if let Some(document_id) = media.parent_media_id {
        MediaRepo::refresh_page_count(&state.db, document_id)
            .await
            .map_err(ApiError::from)?;
    }

    Ok((status, Json(serde_json::to_value(media).unwrap())))
}

/// GET /api/v1/trees/:tree_id/media/:media_id/file
///
/// The stored bytes, served inline.
pub async fn download_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let media = download_record(&state.db, tree_id, media_id).await?;
    let key = stored_key(&media, media.storage_key.as_deref())?;
    serve(
        &state,
        key,
        &media.mime_type,
        media.sha256.as_deref(),
        Some(&media.file_name),
        &headers,
    )
    .await
}

/// GET /api/v1/trees/:tree_id/media/:media_id/archive
///
/// Every page of a document, in one ZIP.
///
/// Entry names include their position to preserve the document's reading order.
///
/// Missing page files fail the request before response headers are sent.
///
/// Entries use Stored compression to avoid recompressing media files.
pub async fn download_archive(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    use std::io::{Seek, Write};
    use tokio::io::AsyncReadExt;

    // Bound temporary archives through delivery, not just while packaging them.
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let permit = SLOTS
        .acquire()
        .await
        .map_err(|_| ApiError(OxidGeneError::Internal("archive worker unavailable".into())))?;
    let (document, pages) = archive_pages(&state.db, tree_id, media_id).await?;
    let runtime = tokio::runtime::Handle::current();
    let (_alive, mut cancelled) = tokio::sync::oneshot::channel::<()>();
    let (file, permit) = tokio::task::spawn_blocking(move || -> Result<_, OxidGeneError> {
        // An anonymous temporary file is removed on error, disconnect or EOF.
        // Finish before sending headers so a failed page cannot become a partial ZIP.
        let mut writer = zip::ZipWriter::new(tempfile::tempfile()?);
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        let digits = pages.len().to_string().len().max(3);
        for (index, page) in pages.iter().enumerate() {
            let key = stored_key(page, page.storage_key.as_deref())?;
            let mut stream = runtime.block_on(state.media.get_stream(key))?;
            writer
                .start_file(
                    format!("{:0digits$}_{}", index + 1, zip_safe(&page.file_name)),
                    options,
                )
                .map_err(|_| OxidGeneError::Internal("archive entry creation failed".into()))?;
            while let Some(chunk) = runtime.block_on(stream.try_next())? {
                if cancelled
                    .try_recv()
                    .is_err_and(|error| error == tokio::sync::oneshot::error::TryRecvError::Closed)
                {
                    return Err(OxidGeneError::Internal("archive request cancelled".into()));
                }
                writer.write_all(&chunk)?;
            }
        }
        let mut file = writer
            .finish()
            .map_err(|_| OxidGeneError::Internal("archive finalization failed".into()))?;
        file.rewind()?;
        Ok((file, permit))
    })
    .await
    .map_err(|_| ApiError(OxidGeneError::Internal("archive worker failed".into())))??;

    let name = format!("{}.zip", archive_stem(&document.file_name));
    let file = tokio::fs::File::from_std(file);
    let length = file.metadata().await.map_err(OxidGeneError::from)?.len();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value("application/zip"));
    headers.insert(CONTENT_LENGTH, header_value(&length.to_string()));
    headers.insert(CACHE_CONTROL, header_value("private, no-store"));
    headers.insert(
        CONTENT_DISPOSITION,
        header_value(&attachment_disposition(&name)),
    );
    headers.insert("x-content-type-options", header_value("nosniff"));
    let stream =
        futures_util::stream::try_unfold((file, permit), |(mut file, permit)| async move {
            let mut buffer = vec![0; 64 * 1024];
            let read = file.read(&mut buffer).await?;
            buffer.truncate(read);
            Ok::<_, std::io::Error>((read != 0).then_some((buffer, (file, permit))))
        });
    Ok((headers, Body::from_stream(stream)).into_response())
}

/// Download reads must not outlive a soft-deleted tree or parent document,
/// even while the asynchronous purge has not removed the page rows yet.
pub(crate) async fn download_record(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<Media, OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    require_tree_resource(db, tree_id, TreeResource::Media, media_id).await?;
    let media = MediaRepo::get(db, media_id).await?;
    if let Some(parent) = media.parent_media_id {
        require_tree_resource(db, tree_id, TreeResource::Media, parent).await?;
    }
    Ok(media)
}

/// Validate the complete document for both HTTP and GraphQL download requests.
pub(crate) async fn archive_pages(
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
    media_id: Uuid,
) -> Result<(Media, Vec<Media>), OxidGeneError> {
    let document = download_record(db, tree_id, media_id).await?;
    let pages = MediaRepo::list_pages(db, media_id).await?;
    if pages.is_empty() {
        return Err(OxidGeneError::NotFound {
            entity: "Media file",
            id: media_id,
        });
    }
    for page in &pages {
        if page.tree_id != tree_id {
            return Err(OxidGeneError::NotFound {
                entity: "Media file",
                id: media_id,
            });
        }
        stored_key(page, page.storage_key.as_deref())?;
    }
    Ok((document, pages))
}

/// A file name that cannot escape the archive's own directory.
///
/// A page's `file_name` came from whatever produced it — an upload, a GEDCOM,
/// a Geneanet deposit — so it is not ours to trust. Anything that would make
/// an unzipper write outside the folder it is unpacking into is flattened.
fn zip_safe(file_name: &str) -> String {
    let base = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .trim_matches(['.', ' ']);
    if base.is_empty() {
        "page".to_string()
    } else {
        base.chars()
            .map(|c| {
                if c.is_control() || matches!(c, ':' | '"' | '<' | '>' | '|' | '?' | '*') {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    }
}

/// The document's name without an extension, for naming the archive.
///
/// A document assembled in the UI is titled `Livret de famille`, but one
/// built by an import may be called `deposit_4713.jpg` — zipping that into
/// `deposit_4713.jpg.zip` reads as a mistake.
fn archive_stem(file_name: &str) -> String {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return "document".to_string();
    }
    match trimmed.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && ext.len() <= 4 && ext.chars().all(char::is_alphanumeric) =>
        {
            stem.to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// GET /api/v1/trees/:tree_id/media/:media_id/thumbnail
///
/// The generated thumbnail. `404` when the format could not be rasterised —
/// a PDF, or an image whose thumbnail generation failed at upload — so a
/// gallery can fall back to an icon on the status code alone.
pub async fn download_thumbnail(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id)
        .await
        .map_err(ApiError)?;
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    let Some(key) = media.thumbnail_key.as_deref() else {
        return Err(ApiError(OxidGeneError::NotFound {
            entity: "Thumbnail",
            id: media_id,
        }));
    };
    // The thumbnail's own extension tells us what it was encoded as; `ingest`
    // only ever writes `jpg` or `png`.
    let mime_type = if key.ends_with(".png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    // No ETag: the thumbnail's digest is not on the row, and its key already
    // changes whenever the bytes do.
    serve(&state, key, mime_type, None, None, &headers).await
}

// ── Shared helpers ──────────────────────────────────────────────────

/// The parts of an upload form we read.
#[derive(Debug, Default)]
struct UploadForm {
    file: Option<(String, Vec<u8>)>,
    title: Option<String>,
    description: Option<String>,
    media_id: Option<Uuid>,
    /// Append the uploaded file as the next page of this document.
    document_id: Option<Uuid>,
}

async fn read_upload_form(mut multipart: Multipart) -> Result<UploadForm, ApiError> {
    let mut form = UploadForm::default();
    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|e| ApiError(OxidGeneError::Validation(format!("malformed upload: {e}"))))?;
        let Some(field) = field else { break };

        let name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().map(str::to_string);
        match name.as_str() {
            "file" => {
                let bytes = field.bytes().await.map_err(|e| {
                    ApiError(OxidGeneError::Validation(format!(
                        "could not read uploaded file: {e}"
                    )))
                })?;
                form.file = Some((
                    file_name.unwrap_or_else(|| "upload".to_string()),
                    bytes.to_vec(),
                ));
            }
            "title" | "description" | "media_id" | "document_id" => {
                let text = field.text().await.map_err(|e| {
                    ApiError(OxidGeneError::Validation(format!(
                        "could not read `{name}` field: {e}"
                    )))
                })?;
                let text = text.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                match name.as_str() {
                    "title" => form.title = Some(text),
                    "description" => form.description = Some(text),
                    other => {
                        let id = Uuid::parse_str(&text).map_err(|_| {
                            ApiError(OxidGeneError::Validation(format!(
                                "`{other}` is not a UUID"
                            )))
                        })?;
                        if other == "document_id" {
                            form.document_id = Some(id);
                        } else {
                            form.media_id = Some(id);
                        }
                    }
                }
            }
            // Ignore anything else rather than failing: a browser form may
            // carry parts we have no use for.
            _ => {}
        }
    }
    Ok(form)
}

/// The storage key of a record that has one, or a `404`-shaped error.
///
/// A media row with no key is a file we know the name of and not the content —
/// every GEDCOM import produces those. Telling the client "not found" is
/// accurate: there are no bytes to serve.
pub(crate) fn stored_key<'a>(
    media: &Media,
    key: Option<&'a str>,
) -> Result<&'a str, OxidGeneError> {
    key.filter(|key| key.starts_with(&format!("{}/", media.tree_id)))
        .ok_or(OxidGeneError::NotFound {
            entity: "Media file",
            id: media.id,
        })
}

/// Serve stored bytes, honouring conditional requests.
async fn serve(
    state: &AppState,
    key: &str,
    mime_type: &str,
    sha256: Option<&str>,
    download_name: Option<&str>,
    request_headers: &HeaderMap,
) -> Result<Response, ApiError> {
    // The content hash makes a strong validator for free — no timestamps, no
    // guessing. A gallery that reloads gets 304s instead of megabytes.
    let etag = sha256.map(|digest| format!("\"{digest}\""));
    if let (Some(etag), Some(requested)) = (&etag, request_headers.get(IF_NONE_MATCH))
        && requested
            .to_str()
            .is_ok_and(|value| value.split(',').any(|candidate| candidate.trim() == etag))
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    let bytes = state.media.get(key).await.map_err(ApiError::from)?;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value(mime_type));
    headers.insert(CONTENT_LENGTH, header_value(&bytes.len().to_string()));
    // Long enough that a pedigree full of portraits is not re-fetched on every
    // navigation, short enough that attaching bytes to an existing record
    // becomes visible without anyone clearing a cache. `private` because a
    // family archive is not something a shared proxy should hold.
    headers.insert(CACHE_CONTROL, header_value("private, max-age=3600"));
    if let Some(etag) = etag {
        headers.insert(ETAG, header_value(&etag));
    }
    if let Some(name) = download_name {
        headers.insert(CONTENT_DISPOSITION, header_value(&disposition(name)));
    }

    Ok((headers, Body::from(bytes)).into_response())
}

/// A `Content-Disposition` value that survives a non-ASCII file name.
///
/// Header values are ASCII, and French archives are full of names like
/// `acte_naissance_thérèse.jpg`. RFC 6266 says to send both: a stripped
/// `filename` for anything old, and a percent-encoded `filename*` that every
/// current browser prefers.
fn disposition(file_name: &str) -> String {
    let file_name = zip_safe(file_name);
    let ascii: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .filter(|c| *c != '"')
        .collect();
    let mut encoded = String::with_capacity(file_name.len());
    for byte in file_name.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    format!("inline; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

fn attachment_disposition(file_name: &str) -> String {
    disposition(file_name).replacen("inline;", "attachment;", 1)
}

/// GET /api/v1/trees/:tree_id/media/:media_id/download
///
/// Stream one original as an attachment; previews keep using `/file` inline.
pub async fn download_attachment(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let media = download_record(&state.db, tree_id, media_id).await?;
    let key = stored_key(&media, media.storage_key.as_deref())?;
    let stream = state.media.get_stream(key).await?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value(&media.mime_type));
    headers.insert(CACHE_CONTROL, header_value("private, no-store"));
    headers.insert(
        CONTENT_DISPOSITION,
        header_value(&attachment_disposition(&media.file_name)),
    );
    headers.insert("x-content-type-options", header_value("nosniff"));
    Ok((headers, Body::from_stream(stream)).into_response())
}

pub(super) fn header_value(value: &str) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(value)
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("application/octet-stream"))
}

/// Body-size ceiling for the upload route, in bytes.
///
/// Slightly above [`MAX_UPLOAD_BYTES`] to leave room for multipart boundaries
/// and the metadata parts, so a file exactly at the limit is rejected by
/// `ingest` with a message about the file rather than by the body layer with a
/// bare `413`.
pub const UPLOAD_BODY_LIMIT: usize = MAX_UPLOAD_BYTES + 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_is_named_after_the_document_not_its_first_page() {
        assert_eq!(archive_stem("Livret de famille"), "Livret de famille");
        assert_eq!(archive_stem("deposit_4713.jpg"), "deposit_4713");
        // Not an extension: a title that happens to contain a full stop.
        assert_eq!(
            archive_stem("Acte n. 12 du registre"),
            "Acte n. 12 du registre"
        );
        assert_eq!(archive_stem("   "), "document");
    }

    #[test]
    fn a_page_name_cannot_write_outside_the_archive() {
        assert_eq!(zip_safe("scan.jpg"), "scan.jpg");
        assert_eq!(zip_safe("../../etc/passwd"), "passwd");
        assert_eq!(zip_safe("C:\\Windows\\system32\\x.dll"), "x.dll");
        assert_eq!(zip_safe(".."), "page");
        assert_eq!(zip_safe("scan.png:payload"), "scan.png_payload");
        assert_eq!(zip_safe("scan\r\n\0.png"), "scan___.png");
        assert_eq!(zip_safe(" ../ . "), "page");
    }

    #[test]
    fn an_accented_file_name_is_sent_in_both_forms() {
        let value = disposition("acte_thérèse.jpg");
        // One underscore per non-ASCII character, not per byte.
        assert!(value.contains(r#"filename="acte_th_r_se.jpg""#), "{value}");
        assert!(
            value.ends_with("filename*=UTF-8''acte_th%C3%A9r%C3%A8se.jpg"),
            "{value}"
        );
    }

    #[test]
    fn a_quote_cannot_close_the_filename_parameter_early() {
        let value = disposition(r#"a"; attachment; x=".jpg"#);
        assert_eq!(value.matches('"').count(), 2, "{value}");
    }

    #[test]
    fn an_attachment_name_cannot_inject_headers_or_paths() {
        let value = attachment_disposition("../../folder\\scan\r\n\0.pdf:payload");
        assert!(value.starts_with("attachment;"));
        assert!(
            value.contains("filename=\"scan___.pdf_payload\""),
            "{value}"
        );
        assert!(!value.contains(['\r', '\n', '\0', '/', '\\']), "{value}");
        assert!(axum::http::HeaderValue::from_str(&value).is_ok());
    }
}
