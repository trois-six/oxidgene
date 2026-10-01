//! REST handlers for FamilySpouse and FamilyChild membership operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_core::types::{FamilyChild, FamilySpouse};
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::family::{self, NewChild, NewSpouse};

// ── Spouses ──────────────────────────────────────────────────────────

/// GET /api/v1/trees/:tree_id/families/:family_id/spouses
pub async fn list_spouses(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<FamilySpouse>>, ApiError> {
    Ok(Json(
        family::list_spouses(&state.db, tree_id, family_id).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/families/:family_id/spouses
pub async fn add_spouse(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewSpouse>,
) -> Result<(StatusCode, Json<FamilySpouse>), ApiError> {
    let spouse = family::add_spouse(&state.db, &state.profiles, tree_id, family_id, body).await?;
    Ok((StatusCode::CREATED, Json(spouse)))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/spouses/:spouse_id
pub async fn remove_spouse(
    State(state): State<AppState>,
    Path((tree_id, family_id, spouse_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    family::remove_spouse(&state.db, &state.profiles, tree_id, family_id, spouse_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Children ─────────────────────────────────────────────────────────

/// GET /api/v1/trees/:tree_id/families/:family_id/children
pub async fn list_children(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<FamilyChild>>, ApiError> {
    Ok(Json(
        family::list_children(&state.db, tree_id, family_id).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/families/:family_id/children
pub async fn add_child(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewChild>,
) -> Result<(StatusCode, Json<FamilyChild>), ApiError> {
    let child = family::add_child(&state.db, &state.profiles, tree_id, family_id, body).await?;
    Ok((StatusCode::CREATED, Json(child)))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/children/:child_id
pub async fn remove_child(
    State(state): State<AppState>,
    Path((tree_id, family_id, child_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    family::remove_child(&state.db, &state.profiles, tree_id, family_id, child_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
