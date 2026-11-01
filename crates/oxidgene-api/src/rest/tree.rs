//! REST handlers for Tree CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Tree};
use oxidgene_db::repo::PaginationParams;
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::{DuplicateTreeRequest, PaginationQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::service::tree::{self, NewTree, TreeListItem, TreePatch, TreeView};

/// GET /api/v1/trees
pub async fn list_trees(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<TreeListItem>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        tree::list_trees(&viewer, &state.reader, &state.profiles, &params).await?,
    ))
}

/// POST /api/v1/trees
pub async fn create_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Json(body): Json<NewTree>,
) -> Result<(StatusCode, Json<Tree>), ApiError> {
    let tree = tree::create_tree(&viewer, &state.db, body).await?;
    Ok((StatusCode::CREATED, Json(tree)))
}

/// GET /api/v1/trees/:tree_id
pub async fn get_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeView>, ApiError> {
    Ok(Json(
        tree::get_tree(&viewer, &state.reader, &state.profiles, tree_id).await?,
    ))
}

/// PUT /api/v1/trees/:tree_id
pub async fn update_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<TreePatch>,
) -> Result<Json<Tree>, ApiError> {
    Ok(Json(
        tree::update_tree(&viewer, &state.db, tree_id, body).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/duplicate
///
/// Duplicate a tree by exporting its GEDCOM and importing it into a new tree.
pub async fn duplicate_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(source_tree_id): Path<Uuid>,
    Json(body): Json<DuplicateTreeRequest>,
) -> Result<(StatusCode, Json<Tree>), ApiError> {
    let tree = tree::duplicate_tree(
        &viewer,
        &state.db,
        &state.profiles,
        source_tree_id,
        body.name,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(tree)))
}

/// DELETE /api/v1/trees/:tree_id
///
/// Flags the tree as deleted and returns straight away; the rows it owns are
/// removed by the background purge worker.
pub async fn delete_tree(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    tree::delete_tree(&viewer, &state.db, &state.purge, tree_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/recent-persons?tree_ids=ID,ID&limit=N
///
/// The persons modified most recently in each of several trees, in one
/// request: the home page's cards. A tree that is missing or deleted, or
/// that the viewer may not read, is left out.
pub async fn recent_persons(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<super::dto::RecentPersonsQuery>,
) -> Result<Json<Vec<crate::service::history::TreeRecentPersons>>, ApiError> {
    let tree_ids = query
        .tree_ids
        .split(',')
        .filter(|id| !id.trim().is_empty())
        .map(|id| Uuid::parse_str(id.trim()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            ApiError(oxidgene_core::OxidGeneError::Validation(
                "tree_ids must be comma-separated UUIDs".into(),
            ))
        })?;
    let tree_ids = crate::authz::permitted_trees(
        &state.reader,
        &viewer,
        &tree_ids,
        oxidgene_core::access::Permission::Read,
    )
    .await?;
    Ok(Json(
        crate::service::history::recently_modified_persons_of_trees(
            &state.reader,
            &state.profiles,
            &viewer,
            &tree_ids,
            query
                .limit
                .unwrap_or(crate::service::history::RECENT_PERSONS_DEFAULT_LIMIT),
        )
        .await?,
    ))
}
