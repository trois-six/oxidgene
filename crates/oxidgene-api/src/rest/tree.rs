//! REST handlers for Tree CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Tree};
use oxidgene_db::repo::{PaginationParams, TreeRepo};
use uuid::Uuid;

use super::dto::{DuplicateTreeRequest, PaginationQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::service::tree::{self, NewTree, TreeListItem, TreePatch};

/// GET /api/v1/trees
pub async fn list_trees(
    State(state): State<AppState>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<TreeListItem>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(tree::list_trees(&state.reader, &params).await?))
}

/// POST /api/v1/trees
pub async fn create_tree(
    State(state): State<AppState>,
    Json(body): Json<NewTree>,
) -> Result<(StatusCode, Json<Tree>), ApiError> {
    let tree = tree::create_tree(&state.db, body).await?;
    Ok((StatusCode::CREATED, Json(tree)))
}

/// GET /api/v1/trees/:tree_id
pub async fn get_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Tree>, ApiError> {
    Ok(Json(TreeRepo::get(&state.reader, tree_id).await?))
}

/// PUT /api/v1/trees/:tree_id
pub async fn update_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<TreePatch>,
) -> Result<Json<Tree>, ApiError> {
    Ok(Json(tree::update_tree(&state.db, tree_id, body).await?))
}

/// POST /api/v1/trees/:tree_id/duplicate
///
/// Duplicate a tree by exporting its GEDCOM and importing it into a new tree.
pub async fn duplicate_tree(
    State(state): State<AppState>,
    Path(source_tree_id): Path<Uuid>,
    Json(body): Json<DuplicateTreeRequest>,
) -> Result<(StatusCode, Json<Tree>), ApiError> {
    let tree = tree::duplicate_tree(&state.db, &state.profiles, source_tree_id, body.name).await?;
    Ok((StatusCode::CREATED, Json(tree)))
}

/// DELETE /api/v1/trees/:tree_id
///
/// Flags the tree as deleted and returns straight away; the rows it owns are
/// removed by the background purge worker.
pub async fn delete_tree(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    tree::delete_tree(&state.db, &state.purge, tree_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
