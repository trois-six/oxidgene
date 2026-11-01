//! REST handlers for the exclusions from sharing (docs/privacy.md §6),
//! mounted by the web build only.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::authorize::Viewer;
use super::error::ApiError;
use super::state::AppState;
use crate::service::exclusion::{self, ExclusionView, NewExclusion};

/// GET /api/v1/trees/:tree_id/exclusions
pub async fn list_exclusions(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<ExclusionView>>, ApiError> {
    Ok(Json(exclusion::list_exclusions(&state.db, tree_id).await?))
}

/// POST /api/v1/trees/:tree_id/exclusions
pub async fn create_exclusion(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewExclusion>,
) -> Result<(StatusCode, Json<ExclusionView>), ApiError> {
    let created = exclusion::create_exclusion(&viewer, &state.db, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// DELETE /api/v1/trees/:tree_id/exclusions/:exclusion_id
pub async fn delete_exclusion(
    State(state): State<AppState>,
    viewer: Viewer,
    Path((tree_id, exclusion_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    exclusion::delete_exclusion(&viewer, &state.db, tree_id, exclusion_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
