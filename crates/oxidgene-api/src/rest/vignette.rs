//! REST handlers for vignettes — named rectangles cut out of a stored media
//! file, and the cropped images they stand for.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::{Media, Vignette};
use oxidgene_db::repo::{MediaRepo, VignetteInput, VignettePatch, VignetteRepo};
use uuid::Uuid;

use super::dto::{CreateVignetteRequest, UpdateVignetteRequest, VignetteListQuery};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn list_media_vignettes(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    let vignettes = VignetteRepo::list_for_media(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(vignettes))
}

/// GET /api/v1/trees/:tree_id/vignettes?person_id=…&event_id=…
///
/// Exactly one filter is required: an unfiltered list of every crop in a tree
/// is not a view anything needs, and paginating one would be busywork.
pub async fn list_vignettes(
    State(state): State<AppState>,
    Path(_tree_id): Path<Uuid>,
    Query(query): Query<VignetteListQuery>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    let vignettes = match (query.person_id, query.event_id) {
        (Some(person_id), None) => VignetteRepo::list_for_person(&state.db, person_id).await,
        (None, Some(event_id)) => VignetteRepo::list_for_event(&state.db, event_id).await,
        _ => {
            return Err(ApiError(OxidGeneError::Validation(
                "exactly one of person_id or event_id is required".into(),
            )));
        }
    }
    .map_err(ApiError::from)?;
    Ok(Json(vignettes))
}

/// POST /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn create_vignette(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateVignetteRequest>,
) -> Result<(StatusCode, Json<Vignette>), ApiError> {
    let media = MediaRepo::get(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    let page = body.page.unwrap_or(0);
    check_rect(&media, page, body.x, body.y, body.width, body.height)?;

    let vignette = VignetteRepo::create(
        &state.db,
        Uuid::now_v7(),
        VignetteInput {
            media_id,
            page,
            x: body.x,
            y: body.y,
            width: body.width,
            height: body.height,
            title: body.title,
            person_id: body.person_id,
            event_id: body.event_id,
        },
    )
    .await
    .map_err(ApiError::from)?;
    Ok((StatusCode::CREATED, Json(vignette)))
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn get_vignette(
    State(state): State<AppState>,
    Path((_tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vignette>, ApiError> {
    let vignette = VignetteRepo::get(&state.db, vignette_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(vignette))
}

/// PUT /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn update_vignette(
    State(state): State<AppState>,
    Path((_tree_id, vignette_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateVignetteRequest>,
) -> Result<Json<Vignette>, ApiError> {
    let existing = VignetteRepo::get(&state.db, vignette_id)
        .await
        .map_err(ApiError::from)?;

    // A rectangle is four numbers that only mean anything together, so the
    // patch takes all four or none — moving one edge in isolation would let a
    // client build a crop the media cannot contain.
    let rect = match (body.x, body.y, body.width, body.height) {
        (None, None, None, None) => None,
        (Some(x), Some(y), Some(width), Some(height)) => Some((x, y, width, height)),
        _ => {
            return Err(ApiError(OxidGeneError::Validation(
                "x, y, width and height must be sent together".into(),
            )));
        }
    };

    if rect.is_some() || body.page.is_some() {
        let media = MediaRepo::get(&state.db, existing.media_id)
            .await
            .map_err(ApiError::from)?;
        let page = body.page.unwrap_or(existing.page);
        let (x, y, width, height) =
            rect.unwrap_or((existing.x, existing.y, existing.width, existing.height));
        check_rect(&media, page, x, y, width, height)?;
    }

    let vignette = VignetteRepo::update(
        &state.db,
        vignette_id,
        VignettePatch {
            page: body.page,
            rect,
            title: body.title,
            person_id: body.person_id,
            event_id: body.event_id,
        },
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(vignette))
}

/// DELETE /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn delete_vignette(
    State(state): State<AppState>,
    Path((_tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    VignetteRepo::delete(&state.db, vignette_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id/image
///
/// The cropped region, as its own JPEG.
///
/// Cropping on read rather than storing a second file is what makes a vignette
/// cheap enough to create freely: eight entries on one register page cost eight
/// rows, not eight copies of a 40 MB scan.
pub async fn vignette_image(
    State(state): State<AppState>,
    Path((_tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    let vignette = VignetteRepo::get(&state.db, vignette_id)
        .await
        .map_err(ApiError::from)?;
    let media = MediaRepo::get(&state.db, vignette.media_id)
        .await
        .map_err(ApiError::from)?;

    let Some(key) = media.storage_key.as_deref() else {
        return Err(ApiError(OxidGeneError::NotFound {
            entity: "Media file",
            id: media.id,
        }));
    };
    if !crate::media::thumbnail::can_thumbnail(&media.mime_type) {
        return Err(ApiError(OxidGeneError::Validation(format!(
            "cannot crop a {} — only raster images can be cropped",
            media.mime_type
        ))));
    }

    let bytes = state.media.get(key).await.map_err(ApiError::from)?;
    let rect = (vignette.x, vignette.y, vignette.width, vignette.height);
    let cropped = tokio::task::spawn_blocking(move || crop(&bytes, rect))
        .await
        .map_err(|e| ApiError(OxidGeneError::Internal(format!("crop panicked: {e}"))))??;

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, super::media::header_value("image/jpeg"));
    headers.insert(
        CONTENT_LENGTH,
        super::media::header_value(&cropped.len().to_string()),
    );
    // Re-derived from the source on every miss, and the rectangle can move, so
    // this caches for a short while rather than being treated as immutable.
    headers.insert(
        CACHE_CONTROL,
        super::media::header_value("private, max-age=300"),
    );
    Ok((headers, Body::from(cropped)).into_response())
}

/// Thin wrapper over [`crate::media::validate_crop`] that speaks `ApiError`.
fn check_rect(
    media: &Media,
    page: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<(), ApiError> {
    crate::media::validate_crop(media, page, x, y, width, height).map_err(ApiError::from)
}

/// Decode, crop and re-encode as JPEG. CPU-bound; callers use `spawn_blocking`.
fn crop(bytes: &[u8], (x, y, width, height): (i32, i32, i32, i32)) -> Result<Vec<u8>, ApiError> {
    use image::ImageFormat;

    let image = image::load_from_memory(bytes)
        .map_err(|e| ApiError(OxidGeneError::Internal(format!("could not decode: {e}"))))?;
    let cropped = image::DynamicImage::crop_imm(
        &image,
        x.max(0) as u32,
        y.max(0) as u32,
        width.max(1) as u32,
        height.max(1) as u32,
    );

    let mut out = std::io::Cursor::new(Vec::new());
    cropped
        .into_rgb8()
        .write_to(&mut out, ImageFormat::Jpeg)
        .map_err(|e| ApiError(OxidGeneError::Internal(format!("could not encode: {e}"))))?;
    Ok(out.into_inner())
}
