//! REST handlers for Citation CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{CitationFilter, CitationRepo, PaginationParams};
use uuid::Uuid;

use super::dto::{CitationListQuery, CreateCitationRequest, UpdateCitationRequest};
use super::error::ApiError;
use super::state::AppState;
use crate::service::citation::{self, CitationPatch, NewCitation};
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/citations
pub async fn list_citations(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<CitationListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(source_id) = query.source_id {
        require_tree_resource(&state.db, tree_id, TreeResource::Source, source_id)
            .await
            .map_err(ApiError)?;
    }
    let filter = CitationFilter {
        person_id: query.person_id,
        event_id: query.event_id,
        family_id: query.family_id,
        source_id: query.source_id,
    };
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let citations = CitationRepo::list(&state.db, tree_id, &filter, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(citations).unwrap()))
}

/// POST /api/v1/trees/:tree_id/citations
pub async fn create_citation(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateCitationRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let new = NewCitation {
        source_id: body.source_id,
        person_id: body.person_id,
        event_id: body.event_id,
        family_id: body.family_id,
        page: body.page,
        confidence: body.confidence,
        text: body.text,
    };
    let citation = citation::create_citation(&state.db, &state.profiles, tree_id, new)
        .await
        .map_err(ApiError)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(citation).unwrap()),
    ))
}

/// PUT /api/v1/trees/:tree_id/citations/:citation_id
pub async fn update_citation(
    State(state): State<AppState>,
    Path((tree_id, citation_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateCitationRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let patch = CitationPatch {
        source_id: body.source_id,
        page: body.page,
        confidence: body.confidence,
        text: body.text,
    };
    let citation =
        citation::update_citation(&state.db, &state.profiles, tree_id, citation_id, patch)
            .await
            .map_err(ApiError)?;
    Ok(Json(serde_json::to_value(citation).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/citations/:citation_id
pub async fn delete_citation(
    State(state): State<AppState>,
    Path((tree_id, citation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    citation::delete_citation(&state.db, &state.profiles, tree_id, citation_id)
        .await
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
