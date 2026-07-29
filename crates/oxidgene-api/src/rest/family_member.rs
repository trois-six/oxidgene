//! REST handlers for FamilySpouse and FamilyChild membership operations.

use crate::profile::invalidation;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{FamilyChildRepo, FamilySpouseRepo};
use uuid::Uuid;

use super::dto::{AddChildRequest, AddSpouseRequest};
use super::error::ApiError;
use super::state::{AppState, begin_tx, commit_tx};

// ── Spouses ──────────────────────────────────────────────────────────

/// GET /api/v1/trees/:tree_id/families/:family_id/spouses
pub async fn list_spouses(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let spouses = FamilySpouseRepo::list_by_family(&state.db, family_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(spouses).unwrap()))
}

/// POST /api/v1/trees/:tree_id/families/:family_id/spouses
pub async fn add_spouse(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddSpouseRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    let spouse = FamilySpouseRepo::create(
        &txn,
        id,
        family_id,
        body.person_id,
        body.role,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    let affected =
        invalidation::affected_persons_for_family_spouse_change(&txn, family_id, body.person_id)
            .await
            .map_err(ApiError)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(spouse).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/spouses/:spouse_id
pub async fn remove_spouse(
    State(state): State<AppState>,
    Path((tree_id, family_id, spouse_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    // Look up which person this spouse link refers to BEFORE deletion.
    let spouses = FamilySpouseRepo::list_by_families(&txn, &[family_id])
        .await
        .map_err(ApiError::from)?;
    let person_id = spouses
        .iter()
        .find(|s| s.id == spouse_id)
        .map(|s| s.person_id);
    // Compute affected BEFORE delete.
    let affected = if let Some(pid) = person_id {
        invalidation::affected_persons_for_family_spouse_change(&txn, family_id, pid)
            .await
            .map_err(ApiError)?
    } else {
        vec![]
    };
    FamilySpouseRepo::delete(&txn, spouse_id)
        .await
        .map_err(ApiError::from)?;
    if !affected.is_empty() {
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Children ─────────────────────────────────────────────────────────

/// GET /api/v1/trees/:tree_id/families/:family_id/children
pub async fn list_children(
    State(state): State<AppState>,
    Path((_tree_id, family_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let children = FamilyChildRepo::list_by_family(&state.db, family_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(children).unwrap()))
}

/// POST /api/v1/trees/:tree_id/families/:family_id/children
pub async fn add_child(
    State(state): State<AppState>,
    Path((tree_id, family_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddChildRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    let child = FamilyChildRepo::create(
        &txn,
        id,
        family_id,
        body.person_id,
        body.child_type,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    let affected =
        invalidation::affected_persons_for_family_child_change(&txn, family_id, body.person_id)
            .await
            .map_err(ApiError)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(child).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/families/:family_id/children/:child_id
pub async fn remove_child(
    State(state): State<AppState>,
    Path((tree_id, family_id, child_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    // Look up which person this child link refers to BEFORE deletion.
    let children = FamilyChildRepo::list_by_families(&txn, &[family_id])
        .await
        .map_err(ApiError::from)?;
    let person_id = children
        .iter()
        .find(|c| c.id == child_id)
        .map(|c| c.person_id);
    let affected = if let Some(pid) = person_id {
        invalidation::affected_persons_for_family_child_change(&txn, family_id, pid)
            .await
            .map_err(ApiError)?
    } else {
        vec![]
    };
    FamilyChildRepo::delete(&txn, child_id)
        .await
        .map_err(ApiError::from)?;
    if !affected.is_empty() {
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
