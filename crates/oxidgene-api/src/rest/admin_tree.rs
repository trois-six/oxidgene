//! REST handlers of the administrator's view of the instance's trees
//! (docs/authorization.md §2, §10): metadata only, and an owner for an
//! ownerless tree. Web build only (`auth` feature).

use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_core::types::Connection;
use oxidgene_db::repo::PaginationParams;
use serde::Deserialize;
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::membership::{self, AdminTree, MemberView};

/// The user an ownerless tree is given to.
#[derive(Debug, Deserialize)]
pub struct OwnerAssignment {
    pub user_id: Uuid,
}

/// GET /api/v1/admin/trees
pub async fn list_trees(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<AdminTree>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        membership::admin_trees(&viewer, &state.reader, &params).await?,
    ))
}

/// PUT /api/v1/admin/trees/{tree_id}/owner
pub async fn assign_owner(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<OwnerAssignment>,
) -> Result<Json<MemberView>, ApiError> {
    Ok(Json(
        membership::assign_owner(&viewer, &state.db, tree_id, body.user_id).await?,
    ))
}
