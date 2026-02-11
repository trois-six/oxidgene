//! REST handlers for Media CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{MediaRepo, PaginationParams};
use uuid::Uuid;

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
pub async fn delete_media(
    State(state): State<AppState>,
    Path((_tree_id, media_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    MediaRepo::delete(&state.db, media_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
