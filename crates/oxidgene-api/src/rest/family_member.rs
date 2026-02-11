//! REST handlers for FamilySpouse and FamilyChild membership operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{FamilyChildRepo, FamilySpouseRepo};
use uuid::Uuid;

use super::dto::{AddChildRequest, AddSpouseRequest};
use super::error::ApiError;
use super::state::AppState;

// ── Spouses ──────────────────────────────────────────────────────────

/// POST /api/v1/trees/:tree_id/families/:family_id/spouses
pub async fn add_spouse(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddSpouseRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let spouse = FamilySpouseRepo::create(
        &state.db,
        id,
        family_id,
        body.person_id,
        body.role,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(spouse).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/spouses/:spouse_id
pub async fn remove_spouse(
    State(state): State<AppState>,
    Path((_tree_id, _family_id, spouse_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    FamilySpouseRepo::delete(&state.db, spouse_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Children ─────────────────────────────────────────────────────────

/// POST /api/v1/trees/:tree_id/families/:family_id/children
pub async fn add_child(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddChildRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let child = FamilyChildRepo::create(
        &state.db,
        id,
        family_id,
        body.person_id,
        body.child_type,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(child).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/children/:child_id
pub async fn remove_child(
    State(state): State<AppState>,
    Path((_tree_id, _family_id, child_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    FamilyChildRepo::delete(&state.db, child_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
