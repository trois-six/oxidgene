//! REST handlers for Place CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{PaginationParams, PlaceRepo};
use uuid::Uuid;

use crate::profile::invalidation;

use super::dto::{CreatePlaceRequest, PlaceListQuery, UpdatePlaceRequest};
use super::error::ApiError;
use super::state::{AppState, TreeResource, begin_tx, commit_tx, require_tree_resource};

/// GET /api/v1/trees/:tree_id/places
pub async fn list_places(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PlaceListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = PlaceRepo::list(&state.db, tree_id, query.search.as_deref(), &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/places
pub async fn create_place(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreatePlaceRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "name must not be empty".to_string(),
        )));
    }
    let id = Uuid::now_v7();
    let place = PlaceRepo::create(
        &state.db,
        id,
        tree_id,
        body.name,
        body.latitude,
        body.longitude,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(place).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/places/:place_id
pub async fn get_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Place, place_id)
        .await
        .map_err(ApiError)?;
    let place = PlaceRepo::get(&state.db, place_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(place).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/places/:place_id
pub async fn update_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdatePlaceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Place, place_id)
        .await
        .map_err(ApiError)?;
    let affected = invalidation::affected_persons_for_place(&txn, place_id)
        .await
        .map_err(ApiError)?;
    let place = PlaceRepo::update(&txn, place_id, body.name, body.latitude, body.longitude)
        .await
        .map_err(ApiError::from)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(Json(serde_json::to_value(place).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/places/:place_id
pub async fn delete_place(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Place, place_id)
        .await
        .map_err(ApiError)?;
    let affected = invalidation::affected_persons_for_place(&txn, place_id)
        .await
        .map_err(ApiError)?;
    PlaceRepo::delete(&txn, place_id)
        .await
        .map_err(ApiError::from)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
