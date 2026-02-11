//! REST handlers for PersonName CRUD operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::PersonNameRepo;
use uuid::Uuid;

use super::dto::{CreatePersonNameRequest, UpdatePersonNameRequest};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn list_person_names(
    State(state): State<AppState>,
    Path((_tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let names = PersonNameRepo::list_by_person(&state.db, person_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(names).unwrap()))
}

/// POST /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn create_person_name(
    State(state): State<AppState>,
    Path((_tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreatePersonNameRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let name = PersonNameRepo::create(
        &state.db,
        id,
        person_id,
        body.name_type,
        body.given_names,
        body.surname,
        body.prefix,
        body.suffix,
        body.nickname,
        body.is_primary,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(name).unwrap()),
    ))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn update_person_name(
    State(state): State<AppState>,
    Path((_tree_id, _person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<UpdatePersonNameRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let name = PersonNameRepo::update(
        &state.db,
        name_id,
        body.name_type,
        body.given_names,
        body.surname,
        body.prefix,
        body.suffix,
        body.nickname,
        body.is_primary,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(name).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn delete_person_name(
    State(state): State<AppState>,
    Path((_tree_id, _person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    PersonNameRepo::delete(&state.db, name_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
