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
use oxidgene_core::types::{Connection, Media, last_path_segment};
use oxidgene_db::repo::{MediaRepo, PaginationParams};
use uuid::Uuid;

use crate::media::MAX_UPLOAD_BYTES;
use crate::service::media::{
    self as media_service, MediaUpdate, NewPage, NewUpload, UploadTarget, archive_pages,
    download_record, stored_key,
};
use crate::service::media_library::{self, MediaListItem};

use super::dto::{
    CreateDocumentRequest, DeleteMediaQuery, GalleryBundleRequest, MediaDeletionStatusQuery,
    MediaListQuery, MediaTagRequest, ReorderPagesRequest,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::scope::{TreeResource, require_tree_resource};

/// POST /api/v1/trees/:tree_id/image-data
///
/// Resolve held picture sources to inline `data:` URLs, in request order, for
/// a client that cannot serve them from an origin of its own. A slot is null
/// when there is nothing to inline — a remote source, or a picture we no
/// longer hold.
pub async fn image_data(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<crate::rest::dto::ImageDataRequest>,
) -> Result<Json<Vec<Option<String>>>, ApiError> {
    let urls = crate::service::image_bytes::load_image_data_urls(
        &state.db,
        &state.media,
        tree_id,
        &body.sources,
    )
    .await?;
    Ok(Json(urls))
}

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
    .await?;
    Ok(Json(bundle))
}

/// GET /api/v1/trees/:tree_id/media
///
/// The tree's documents, narrowed by the query's filters, each with its
/// usage count. `tag` may be repeated: a document must carry every one.
pub async fn list_media(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<MediaListQuery>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Json<Connection<MediaListItem>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after.clone(),
    };
    let filters = query.filters(crate::rest::dto::tag_values(pairs));
    let connection = media_library::list(&state.db, tree_id, filters, &params).await?;
    Ok(Json(connection))
}

/// GET /api/v1/trees/:tree_id/media/facets
///
/// The tags, file kinds and categories the tree's documents carry, each
/// with its document count. With `tag` (repeatable), the tags are counted
/// among the documents carrying every tag given.
pub async fn list_media_facets(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<Json<media_library::MediaFacets>, ApiError> {
    let facets =
        media_library::facets(&state.db, tree_id, crate::rest::dto::tag_values(pairs)).await?;
    Ok(Json(facets))
}

/// POST /api/v1/trees/:tree_id/media
///
/// Add a page that names a file without holding its bytes — a URL an archive
/// serves, or a path a GEDCOM mentioned. The bytes-carrying counterpart is
/// `POST /media/upload`; both add a page to an existing document.
pub async fn create_media(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewPage>,
) -> Result<(StatusCode, Json<Media>), ApiError> {
    let page = media_service::create_page(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(page)))
}

/// GET /api/v1/trees/:tree_id/media/:media_id
pub async fn get_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Media>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id).await?;
    Ok(Json(MediaRepo::get(&state.db, media_id).await?))
}

/// PUT /api/v1/trees/:tree_id/media/:media_id
pub async fn update_media(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MediaUpdate>,
) -> Result<Json<Media>, ApiError> {
    let media =
        media_service::update_media(&state.db, &state.profiles, tree_id, media_id, body).await?;
    Ok(Json(media))
}

/// POST /api/v1/trees/:tree_id/media/:media_id/tags
pub async fn add_tag(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MediaTagRequest>,
) -> Result<Json<Media>, ApiError> {
    Ok(Json(
        media_service::add_tag(&state.db, tree_id, media_id, &body.tag).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/media/:media_id/tags
pub async fn remove_tag(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MediaTagRequest>,
) -> Result<StatusCode, ApiError> {
    media_service::remove_tag(&state.db, tree_id, media_id, &body.tag).await?;
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
) -> Result<(StatusCode, Json<Media>), ApiError> {
    let document = media_service::create_document(&state.db, tree_id, body.title).await?;
    Ok((StatusCode::CREATED, Json(document)))
}

/// GET /api/v1/trees/:tree_id/media/:media_id/pages
pub async fn list_pages(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Media>>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id).await?;
    Ok(Json(MediaRepo::list_pages(&state.db, media_id).await?))
}

/// PUT /api/v1/trees/:tree_id/media/:media_id/pages
///
/// Set the page order. The body lists exactly this document's pages, once
/// each — a partial list is refused rather than guessed at.
pub async fn reorder_pages(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReorderPagesRequest>,
) -> Result<Json<Vec<Media>>, ApiError> {
    let pages = media_service::reorder_pages(
        &state.db,
        &state.profiles,
        tree_id,
        media_id,
        &body.page_ids,
    )
    .await?;
    Ok(Json(pages))
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
    media_service::delete_page(
        &state.db,
        &state.profiles,
        &*state.media,
        tree_id,
        media_id,
        page_id,
    )
    .await?;
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
    let allowed_link_id = match (query.only_if_unreferenced_elsewhere, query.allowed_link_id) {
        (false, _) => None,
        (true, Some(link_id)) => Some(link_id),
        (true, None) => {
            return Err(ApiError(OxidGeneError::Validation(
                "allowed_link_id is required for conditional media deletion".into(),
            )));
        }
    };
    let deleted = media_service::delete_media(
        &state.db,
        &state.profiles,
        &*state.media,
        tree_id,
        media_id,
        allowed_link_id,
    )
    .await?;
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
    let can_delete =
        media_service::can_delete_media(&state.db, tree_id, media_id, query.allowed_link_id)
            .await?;
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
) -> Result<(StatusCode, Json<Media>), ApiError> {
    // Before the body is read: a waiting upload holds a connection, not the
    // file. The slot is held until the bytes are stored.
    let _intake = crate::service::intake::slot().await?;
    let form = read_upload_form(multipart).await?;
    let Some((file_name, bytes)) = form.file else {
        return Err(ApiError(OxidGeneError::Validation(
            "multipart form has no `file` part".into(),
        )));
    };
    let target = match (form.media_id, form.document_id) {
        (Some(media_id), _) => UploadTarget::Attach { media_id },
        (None, Some(document_id)) => UploadTarget::NewPage { document_id },
        (None, None) => {
            return Err(ApiError(OxidGeneError::Validation(
                "multipart form has no `document_id` part".into(),
            )));
        }
    };
    let (page, created) = media_service::upload(
        &state.db,
        &state.profiles,
        &*state.media,
        tree_id,
        NewUpload {
            file_name,
            bytes,
            title: form.title,
            description: form.description,
            target,
        },
    )
    .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(page)))
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
    let key = stored_key(&media)?;
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
/// A page held only as a URL contributes a `.url` Internet Shortcut rather
/// than bytes: we never fetch a remote file, and dropping the page would make
/// the archive disagree with the document about how many pages it has.
///
/// Missing page files fail the request before response headers are sent.
///
/// Entries use Stored compression to avoid recompressing media files.
pub async fn download_archive(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    // Bound temporary archives through delivery, not just while packaging them.
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let permit = SLOTS
        .acquire()
        .await
        .map_err(|_| ApiError(OxidGeneError::Internal("archive worker unavailable".into())))?;
    let (document, pages) = archive_pages(&state.db, tree_id, media_id).await?;
    let runtime = tokio::runtime::Handle::current();
    let (_alive, mut cancelled) = tokio::sync::oneshot::channel::<()>();
    let archive_span = tracing::info_span!("media.archive", page.count = pages.len());
    let (file, permit) =
        crate::service::blocking::spawn_in(archive_span, move || -> Result<_, OxidGeneError> {
            let file = write_page_archive(&pages, &*state.media, &runtime, &mut cancelled)?;
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
            use tokio::io::AsyncReadExt;
            let mut buffer = vec![0; 64 * 1024];
            let read = file.read(&mut buffer).await?;
            buffer.truncate(read);
            Ok::<_, std::io::Error>((read != 0).then_some((buffer, (file, permit))))
        });
    Ok((headers, Body::from_stream(stream)).into_response())
}

/// Writes `pages` into an anonymous temporary ZIP, rewound; blocking.
///
/// The file is removed on error, disconnect or EOF. It is finished before any
/// header is sent, so a failed page cannot become a partial ZIP.
fn write_page_archive(
    pages: &[Media],
    media: &dyn crate::media::MediaStore,
    runtime: &tokio::runtime::Handle,
    cancelled: &mut tokio::sync::oneshot::Receiver<()>,
) -> Result<std::fs::File, OxidGeneError> {
    use std::io::{Seek, Write};
    let mut writer = zip::ZipWriter::new(tempfile::tempfile()?);
    let digits = pages.len().to_string().len().max(3);
    let entry_failed = |_| OxidGeneError::Internal("archive entry creation failed".into());
    for (index, page) in pages.iter().enumerate() {
        let position = index + 1;
        let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        if oxidgene_core::types::is_remote_url(&page.file_path) {
            let name = format!("{position:0digits$}_{}.url", zip_safe(&page.file_name));
            writer.start_file(name, options).map_err(entry_failed)?;
            writer.write_all(internet_shortcut(&page.file_path).as_bytes())?;
            continue;
        }
        let key = stored_key(page)?;
        let mut stream = runtime.block_on(media.get_stream(key))?;
        let name = format!("{position:0digits$}_{}", zip_safe(&page.file_name));
        writer.start_file(name, options).map_err(entry_failed)?;
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
    Ok(file)
}

/// The entry a remote page contributes to an archive: an Internet Shortcut.
///
/// We never fetch a remote file, so there are no bytes to pack — and an
/// archive that silently dropped the page would tell the reader a two-page
/// document has one. `.url` is the format that answers this: a two-line INI
/// file, opened by a double-click on Windows and plain readable text
/// everywhere else, so the address survives the round trip through a ZIP.
///
/// CRLF because the format is a Windows one. Control characters are stripped:
/// the address came from a GEDCOM or a paste box, and a newline in it would
/// otherwise write a second key into the file.
fn internet_shortcut(url: &str) -> String {
    let url: String = url.trim().chars().filter(|c| !c.is_control()).collect();
    format!("[InternetShortcut]\r\nURL={url}\r\n")
}

/// A file name that cannot escape the archive's own directory.
///
/// A page's `file_name` came from whatever produced it — an upload, a GEDCOM,
/// a Geneanet deposit — so it is not ours to trust. Anything that would make
/// an unzipper write outside the folder it is unpacking into is flattened.
fn zip_safe(file_name: &str) -> String {
    let base = last_path_segment(file_name).trim_matches(['.', ' ']);
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
    require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id).await?;
    let media = MediaRepo::get(&state.db, media_id).await?;
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

    let bytes = state.media.get(key).await?;

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
    confine(&mut headers, mime_type);

    Ok((headers, Body::from(bytes)).into_response())
}

/// Keep a stored file from acting as a page of the application.
///
/// Its MIME type is whatever an upload or an import declared, so an HTML or
/// SVG file served inline on the API's origin — the frontend's own origin
/// behind a same-origin gateway — would run its scripts with the reader's
/// access to every tree. `nosniff` stops a browser from promoting a file past
/// its declared type, and the sandbox gives whatever renders an opaque origin
/// with no scripts. PDFs are spared the sandbox, which browsers' built-in
/// viewers refuse to render into; they execute nothing of the page's.
pub(crate) fn confine(headers: &mut HeaderMap, mime_type: &str) {
    headers.insert("x-content-type-options", header_value("nosniff"));
    if !mime_type.eq_ignore_ascii_case("application/pdf") {
        headers.insert("content-security-policy", header_value("sandbox"));
    }
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
    let key = stored_key(&media)?;
    let stream = state.media.get_stream(key).await?;
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header_value(&media.mime_type));
    headers.insert(CACHE_CONTROL, header_value("private, no-store"));
    headers.insert(
        CONTENT_DISPOSITION,
        header_value(&attachment_disposition(&media.file_name)),
    );
    confine(&mut headers, &media.mime_type);
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
