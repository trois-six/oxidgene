//! REST handlers for PersonName CRUD operations.

use crate::profile::invalidation;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo};
use uuid::Uuid;

use super::dto::{CreatePersonNameRequest, UpdatePersonNameRequest};
use super::error::ApiError;
use super::state::{AppState, TreeResource, begin_tx, commit_tx, require_tree_resource};

/// GET /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn list_person_names(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Person, person_id)
        .await
        .map_err(ApiError)?;
    let names = PersonNameRepo::list_by_person(&state.db, person_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(names).unwrap()))
}

/// POST /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn create_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreatePersonNameRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, person_id)
        .await
        .map_err(ApiError)?;
    let name = PersonNameRepo::create(
        &txn,
        id,
        person_id,
        body.name_type,
        PersonNamePieces {
            given_names: body.given_names,
            surname: body.surname,
            surname_prefix: body.surname_prefix,
            prefix: body.prefix,
            suffix: body.suffix,
            nickname: body.nickname,
        },
        body.is_primary,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    // Name changes affect display_name references across relatives.
    let affected = invalidation::affected_persons(&txn, person_id)
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
        Json(serde_json::to_value(name).unwrap()),
    ))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn update_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<UpdatePersonNameRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, person_id)
        .await
        .map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::PersonName, name_id)
        .await
        .map_err(ApiError)?;
    let name = PersonNameRepo::update(
        &txn,
        name_id,
        body.name_type,
        PersonNamePiecesPatch {
            given_names: body.given_names,
            surname: body.surname,
            surname_prefix: body.surname_prefix,
            prefix: body.prefix,
            suffix: body.suffix,
            nickname: body.nickname,
        },
        body.is_primary,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    let affected = invalidation::affected_persons(&txn, person_id)
        .await
        .map_err(ApiError)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(Json(serde_json::to_value(name).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn delete_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, person_id)
        .await
        .map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::PersonName, name_id)
        .await
        .map_err(ApiError)?;
    PersonNameRepo::delete(&txn, name_id)
        .await
        .map_err(ApiError::from)?;
    let affected = invalidation::affected_persons(&txn, person_id)
        .await
        .map_err(ApiError)?;
    state
        .profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
