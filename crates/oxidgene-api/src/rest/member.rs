//! REST handlers of a tree's members and its ownership
//! (docs/authorization.md §5, §10). Web build only (`auth` feature).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::PaginationParams;
use serde::Deserialize;
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::membership::{self, MemberPatch, MemberView};

/// The member a tree is handed to.
#[derive(Debug, Deserialize)]
pub struct OwnershipTransfer {
    pub user_id: Uuid,
}

/// GET /api/v1/trees/{tree_id}/members
pub async fn list_members(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<MemberView>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        membership::list_members(&viewer, &state.reader, tree_id, &params).await?,
    ))
}

/// PUT /api/v1/trees/{tree_id}/members/{user_id}
pub async fn update_member(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<MemberPatch>,
) -> Result<Json<MemberView>, ApiError> {
    Ok(Json(
        membership::update_member(
            &viewer,
            &state.db,
            state.profiles.cache(),
            state.auth.mailing(),
            tree_id,
            user_id,
            body,
        )
        .await?,
    ))
}

/// DELETE /api/v1/trees/{tree_id}/members/{user_id}
pub async fn remove_member(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    membership::remove_member(
        &viewer,
        &state.db,
        state.profiles.cache(),
        state.auth.mailing(),
        tree_id,
        user_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/trees/{tree_id}/ownership
pub async fn transfer_ownership(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<OwnershipTransfer>,
) -> Result<Json<MemberView>, ApiError> {
    Ok(Json(
        membership::transfer_ownership(
            &viewer,
            &state.db,
            state.profiles.cache(),
            state.auth.mailing(),
            tree_id,
            body.user_id,
        )
        .await?,
    ))
}
