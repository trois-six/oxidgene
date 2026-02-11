//! REST handlers for Source CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{PaginationParams, SourceRepo};
use uuid::Uuid;

use super::dto::{CreateSourceRequest, PaginationQuery, UpdateSourceRequest};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/sources
pub async fn list_sources(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = SourceRepo::list(&state.db, tree_id, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/sources
pub async fn create_source(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateSourceRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if body.title.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "title must not be empty".to_string(),
        )));
    }
    let id = Uuid::now_v7();
    let source = SourceRepo::create(
        &state.db,
        id,
        tree_id,
        body.title,
        body.author,
        body.publisher,
        body.abbreviation,
        body.repository_name,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(source).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/sources/:source_id
pub async fn get_source(
    State(state): State<AppState>,
    Path((_tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let source = SourceRepo::get(&state.db, source_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(source).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/sources/:source_id
pub async fn update_source(
    State(state): State<AppState>,
    Path((_tree_id, source_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateSourceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let source = SourceRepo::update(
        &state.db,
        source_id,
        body.title,
        body.author,
        body.publisher,
        body.abbreviation,
        body.repository_name,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(source).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/sources/:source_id
pub async fn delete_source(
    State(state): State<AppState>,
    Path((_tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    SourceRepo::delete(&state.db, source_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
