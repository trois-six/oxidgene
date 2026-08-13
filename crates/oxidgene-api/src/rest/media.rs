//! REST handlers for Media CRUD, upload and file serving.

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::Media;
use oxidgene_db::repo::{MediaRepo, PaginationParams, UploadedMedia};
use uuid::Uuid;

use crate::media::{self, MAX_UPLOAD_BYTES};

use super::dto::{CreateMediaRequest, PaginationQuery, UpdateMediaRequest};
use super::error::ApiError;
use super::state::AppState;

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
    let media = MediaRepo::create(
        &state.db,
        id,
        tree_id,
        body.file_name,
        body.mime_type,
        body.file_path,
        body.file_size,
        body.title,
        body.description,
    )
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
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(media).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/media/:media_id
pub async fn update_media(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateMediaRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let media = MediaRepo::update(&state.db, media_id, body.title, body.description)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(media).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/media/:media_id
///
/// Soft-deletes the record and leaves the bytes in the store. Content
/// addressing means another record may be sharing them, and the tree purge
/// removes the whole directory anyway.
pub async fn delete_media(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    MediaRepo::delete(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/trees/:tree_id/media/upload
///
/// Multipart form. The `file` part carries the bytes; optional `title` and
/// `description` parts carry metadata. Sending a `media_id` part attaches the
/// file to an existing record instead of creating one — the flow for filling
/// in a GEDCOM-imported stub that names a photo nobody had.
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
    };

    let (status, media) = match form.media_id {
        Some(media_id) => (
            StatusCode::OK,
            MediaRepo::attach_file(&state.db, media_id, upload)
                .await
                .map_err(ApiError::from)?,
        ),
        None => (
            StatusCode::CREATED,
            MediaRepo::create_uploaded(&state.db, Uuid::now_v7(), tree_id, upload)
                .await
                .map_err(ApiError::from)?,
        ),
    };

    Ok((status, Json(serde_json::to_value(media).unwrap())))
}

/// GET /api/v1/trees/:tree_id/media/:media_id/file
///
/// The stored bytes, served inline.
pub async fn download_media(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
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

/// GET /api/v1/trees/:tree_id/media/:media_id/thumbnail
///
/// The generated thumbnail. `404` when the format could not be rasterised —
/// a PDF, or an image whose thumbnail generation failed at upload — so a
/// gallery can fall back to an icon on the status code alone.
pub async fn download_thumbnail(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
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
            "title" | "description" | "media_id" => {
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
                    _ => {
                        form.media_id = Some(Uuid::parse_str(&text).map_err(|_| {
                            ApiError(OxidGeneError::Validation(
                                "`media_id` is not a UUID".to_string(),
                            ))
                        })?)
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
fn stored_key<'a>(media: &Media, key: Option<&'a str>) -> Result<&'a str, ApiError> {
    key.ok_or(ApiError(OxidGeneError::NotFound {
        entity: "Media file",
        id: media.id,
    }))
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
}
