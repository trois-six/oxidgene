//! REST handlers for Citation CRUD operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::CitationRepo;
use uuid::Uuid;

use super::dto::{CreateCitationRequest, UpdateCitationRequest};
use super::error::ApiError;
use super::state::AppState;

/// POST /api/v1/trees/:tree_id/citations
pub async fn create_citation(
    State(state): State<AppState>,
    Path(_tree_id): Path<Uuid>,
    Json(body): Json<CreateCitationRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let citation = CitationRepo::create(
        &state.db,
        id,
        body.source_id,
        body.person_id,
        body.event_id,
        body.family_id,
        body.page,
        body.confidence,
        body.text,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(citation).unwrap()),
    ))
}

/// PUT /api/v1/trees/:tree_id/citations/:citation_id
pub async fn update_citation(
    State(state): State<AppState>,
    Path((_tree_id, citation_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateCitationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let citation = CitationRepo::update(
        &state.db,
        citation_id,
        body.page,
        body.confidence,
        body.text,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(citation).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/citations/:citation_id
pub async fn delete_citation(
    State(state): State<AppState>,
    Path((_tree_id, citation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    CitationRepo::delete(&state.db, citation_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
