//! REST handlers for Citation CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Citation, Connection};
use oxidgene_db::repo::{CitationFilter, PaginationParams};
use uuid::Uuid;

use super::dto::CitationListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::citation::{self, CitationPatch, NewCitation};

/// GET /api/v1/trees/:tree_id/citations
pub async fn list_citations(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<CitationListQuery>,
) -> Result<Json<Connection<Citation>>, ApiError> {
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
    Ok(Json(
        citation::list_citations(&state.reader, tree_id, &filter, &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/citations
pub async fn create_citation(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewCitation>,
) -> Result<(StatusCode, Json<Citation>), ApiError> {
    let citation = citation::create_citation(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(citation)))
}

/// PUT /api/v1/trees/:tree_id/citations/:citation_id
pub async fn update_citation(
    State(state): State<AppState>,
    Path((tree_id, citation_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CitationPatch>,
) -> Result<Json<Citation>, ApiError> {
    let citation =
        citation::update_citation(&state.db, &state.profiles, tree_id, citation_id, body).await?;
    Ok(Json(citation))
}

/// DELETE /api/v1/trees/:tree_id/citations/:citation_id
pub async fn delete_citation(
    State(state): State<AppState>,
    Path((tree_id, citation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    citation::delete_citation(&state.db, &state.profiles, tree_id, citation_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
