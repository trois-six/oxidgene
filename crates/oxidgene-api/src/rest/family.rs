//! REST handlers for Family CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_cache::invalidation;
use oxidgene_db::repo::{FamilyRepo, PaginationParams};
use uuid::Uuid;

use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/families
pub async fn list_families(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = FamilyRepo::list(&state.db, tree_id, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/families
pub async fn create_family(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let family = FamilyRepo::create(&state.db, id, tree_id)
        .await
        .map_err(ApiError::from)?;
    // No cache impact — empty family.
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(family).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/families/:family_id
pub async fn get_family(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let family = FamilyRepo::get(&state.db, family_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(family).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/families/:family_id
pub async fn update_family(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let family = FamilyRepo::update(&state.db, family_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(family).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id
pub async fn delete_family(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    // Compute affected BEFORE delete.
    let affected = invalidation::affected_persons_for_family(&state.db, family_id)
        .await
        .map_err(ApiError)?;
    FamilyRepo::delete(&state.db, family_id)
        .await
        .map_err(ApiError::from)?;
    if !affected.is_empty() {
        state
            .cache
            .invalidate_for_mutation(tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    Ok(StatusCode::NO_CONTENT)
}
