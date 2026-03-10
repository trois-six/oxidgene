//! REST handlers for Tree CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{PaginationParams, TreeRepo};
use uuid::Uuid;

use super::dto::{CreateTreeRequest, DuplicateTreeRequest, PaginationQuery, UpdateTreeRequest};
use super::error::ApiError;
use super::state::AppState;
use crate::service::gedcom;

/// GET /api/v1/trees
pub async fn list_trees(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let connection = TreeRepo::list(&state.db, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees
pub async fn create_tree(
    State(state): State<AppState>,
    Json(body): Json<CreateTreeRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "name must not be empty".to_string(),
        )));
    }
    let id = Uuid::now_v7();
    let tree = TreeRepo::create(&state.db, id, body.name, body.description)
        .await
        .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(tree).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id
pub async fn get_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tree = TreeRepo::get(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(tree).unwrap()))
}

/// PUT /api/v1/trees/:tree_id
pub async fn update_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<UpdateTreeRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tree = TreeRepo::update(
        &state.db,
        tree_id,
        body.name,
        body.description,
        body.sosa_root_person_id,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(tree).unwrap()))
}

/// POST /api/v1/trees/:tree_id/duplicate
///
/// Duplicate a tree by exporting its GEDCOM and importing it into a new tree.
pub async fn duplicate_tree(
    State(state): State<AppState>,
    Path(source_tree_id): Path<Uuid>,
    Json(body): Json<DuplicateTreeRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "name must not be empty".to_string(),
        )));
    }

    // Export GEDCOM from source tree
    let export = gedcom::load_and_export(&state.db, source_tree_id)
        .await
        .map_err(ApiError::from)?;

    // Create the new tree
    let new_id = Uuid::now_v7();
    let new_tree = TreeRepo::create(&state.db, new_id, body.name, None)
        .await
        .map_err(ApiError::from)?;

    // Import GEDCOM into the new tree
    gedcom::import_and_persist(&state.db, new_id, &export.gedcom)
        .await
        .map_err(ApiError::from)?;

    // Rebuild cache for the new tree
    state
        .cache
        .rebuild_tree_full(new_id)
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(new_tree).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id
pub async fn delete_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    TreeRepo::delete(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;

    // Invalidate all caches for the deleted tree
    state
        .cache
        .invalidate_tree(tree_id)
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}
