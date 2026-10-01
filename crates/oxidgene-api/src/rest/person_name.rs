//! REST handlers for PersonName CRUD operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_core::types::PersonName;
use uuid::Uuid;

use super::dto::RelationLabelsRequest;
use super::error::ApiError;
use super::state::AppState;
use crate::service::person_name::{self, NewPersonName, PersonNamePatch};
use crate::service::relation_labels::{RelationLabels, load_relation_labels};

/// GET /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn list_person_names(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<PersonName>>, ApiError> {
    let names = person_name::list_person_names(&state.reader, tree_id, person_id).await?;
    Ok(Json(names))
}

/// POST /api/v1/trees/:tree_id/relation-labels
pub async fn relation_labels(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<RelationLabelsRequest>,
) -> Result<Json<RelationLabels>, ApiError> {
    let labels =
        load_relation_labels(&state.reader, tree_id, &body.person_ids, &body.family_ids).await?;
    Ok(Json(labels))
}

/// POST /api/v1/trees/:tree_id/persons/:person_id/names
pub async fn create_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewPersonName>,
) -> Result<(StatusCode, Json<PersonName>), ApiError> {
    let name =
        person_name::create_person_name(&state.db, &state.profiles, tree_id, person_id, body)
            .await?;
    Ok((StatusCode::CREATED, Json(name)))
}

/// PUT /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn update_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<PersonNamePatch>,
) -> Result<Json<PersonName>, ApiError> {
    let name = person_name::update_person_name(
        &state.db,
        &state.profiles,
        tree_id,
        person_id,
        name_id,
        body,
    )
    .await?;
    Ok(Json(name))
}

/// DELETE /api/v1/trees/:tree_id/persons/:person_id/names/:name_id
pub async fn delete_person_name(
    State(state): State<AppState>,
    Path((tree_id, person_id, name_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    person_name::delete_person_name(&state.db, &state.profiles, tree_id, person_id, name_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
