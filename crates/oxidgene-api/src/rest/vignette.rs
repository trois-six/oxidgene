//! REST handlers for vignettes — named rectangles cut out of a stored media
//! file, and the cropped images they stand for.

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::Vignette;
use oxidgene_db::repo::{MediaRepo, VignetteRepo};
use uuid::Uuid;

use super::dto::VignetteListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::scope::{TreeResource, require_tree_resource};
use crate::service::vignette::{self, NewVignette, VignetteUpdate};

/// GET /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn list_media_vignettes(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Media, media_id).await?;
    let vignettes = VignetteRepo::list_for_media(&state.reader, media_id).await?;
    Ok(Json(vignettes))
}

/// GET /api/v1/trees/:tree_id/vignettes?person_id=…&event_id=…
///
/// Exactly one filter is required: an unfiltered list of every crop in a tree
/// is not a view anything needs, and paginating one would be busywork.
pub async fn list_vignettes(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<VignetteListQuery>,
) -> Result<Json<Vec<Vignette>>, ApiError> {
    if let Some(person_id) = query.person_id {
        require_tree_resource(&state.reader, tree_id, TreeResource::Person, person_id).await?;
    }
    if let Some(event_id) = query.event_id {
        require_tree_resource(&state.reader, tree_id, TreeResource::Event, event_id).await?;
    }
    let vignettes = match (query.person_id, query.event_id) {
        (Some(person_id), None) => VignetteRepo::list_for_person(&state.reader, person_id).await,
        (None, Some(event_id)) => VignetteRepo::list_for_event(&state.reader, event_id).await,
        _ => {
            return Err(ApiError(OxidGeneError::Validation(
                "exactly one of person_id or event_id is required".into(),
            )));
        }
    }?;
    Ok(Json(vignettes))
}

/// POST /api/v1/trees/:tree_id/media/:media_id/vignettes
pub async fn create_vignette(
    State(state): State<AppState>,
    Path((tree_id, media_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewVignette>,
) -> Result<(StatusCode, Json<Vignette>), ApiError> {
    let vignette = vignette::create_vignette(&state.db, tree_id, media_id, body).await?;
    Ok((StatusCode::CREATED, Json(vignette)))
}

/// GET /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn get_vignette(
    State(state): State<AppState>,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vignette>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Vignette, vignette_id).await?;
    let vignette = VignetteRepo::get(&state.reader, vignette_id).await?;
    Ok(Json(vignette))
}

/// PUT /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn update_vignette(
    State(state): State<AppState>,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<VignetteUpdate>,
) -> Result<Json<Vignette>, ApiError> {
    Ok(Json(
        vignette::update_vignette(&state.db, tree_id, vignette_id, body).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/vignettes/:vignette_id
pub async fn delete_vignette(
    State(state): State<AppState>,
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    vignette::delete_vignette(&state.db, &state.profiles, tree_id, vignette_id).await?;
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
    Path((tree_id, vignette_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Vignette, vignette_id).await?;
    let vignette = VignetteRepo::get(&state.reader, vignette_id).await?;
    let media = MediaRepo::get(&state.reader, vignette.media_id).await?;

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

    let rect = (vignette.x, vignette.y, vignette.width, vignette.height);
    let cropped = crate::media::cut_vignette(&*state.media, &media, key, rect).await?;

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
