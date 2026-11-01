//! REST handlers of the instance administrator's user management
//! (docs/authorization.md §10, docs/authentication.md §6.4–§6.6). Web build
//! only (`auth` feature).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::PaginationParams;
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::account::{self, IdentityRebind, ManagedUser, UserPatch};

/// GET /api/v1/admin/users
pub async fn list_users(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<ManagedUser>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let users = account::list_users(&state.reader, viewer.principal(), &params).await?;
    Ok(Json(users.map(|user| ManagedUser::from(&user))))
}

/// GET /api/v1/admin/users/{user_id}
pub async fn get_user(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ManagedUser>, ApiError> {
    let user = account::managed_user(&state.reader, viewer.principal(), user_id).await?;
    Ok(Json(ManagedUser::from(&user)))
}

/// PUT /api/v1/admin/users/{user_id}
pub async fn update_user(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
    Json(body): Json<UserPatch>,
) -> Result<Json<ManagedUser>, ApiError> {
    let user =
        account::update_user(&state.db, &state.auth, viewer.principal(), user_id, body).await?;
    Ok(Json(ManagedUser::from(&user)))
}

/// PUT /api/v1/admin/users/{user_id}/identity
pub async fn rebind_identity(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
    Json(body): Json<IdentityRebind>,
) -> Result<Json<ManagedUser>, ApiError> {
    let user =
        account::rebind_identity(&state.db, &state.auth, viewer.principal(), user_id, body).await?;
    Ok(Json(ManagedUser::from(&user)))
}

/// DELETE /api/v1/admin/users/{user_id}
pub async fn erase_user(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    account::erase_user(
        &state.db,
        &state.auth,
        &state.media,
        viewer.principal(),
        user_id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
