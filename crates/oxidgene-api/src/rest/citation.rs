//! REST handlers for Citation CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{CitationRepo, SourceRepo};
use uuid::Uuid;

use super::dto::{CitationListQuery, CreateCitationRequest, UpdateCitationRequest};
use super::error::ApiError;
use super::state::{AppState, TreeResource, begin_tx, commit_tx, require_tree_resource};

/// GET /api/v1/trees/:tree_id/citations
pub async fn list_citations(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<CitationListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let source_ids = if let Some(source_id) = query.source_id {
        require_tree_resource(&state.db, tree_id, TreeResource::Source, source_id)
            .await
            .map_err(ApiError)?;
        vec![source_id]
    } else {
        SourceRepo::list_all(&state.db, tree_id)
            .await
            .map_err(ApiError::from)?
            .into_iter()
            .map(|source| source.id)
            .collect()
    };
    let citations = CitationRepo::list_by_sources(&state.db, &source_ids)
        .await
        .map_err(ApiError::from)?
        .into_iter()
        .filter(|citation| {
            query
                .person_id
                .is_none_or(|pid| citation.person_id == Some(pid))
                && query
                    .event_id
                    .is_none_or(|eid| citation.event_id == Some(eid))
                && query
                    .family_id
                    .is_none_or(|fid| citation.family_id == Some(fid))
        })
        .collect::<Vec<_>>();
    Ok(Json(serde_json::to_value(citations).unwrap()))
}

/// POST /api/v1/trees/:tree_id/citations
pub async fn create_citation(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateCitationRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Source, body.source_id)
        .await
        .map_err(ApiError)?;
    for (resource, id) in [
        (TreeResource::Person, body.person_id),
        (TreeResource::Event, body.event_id),
        (TreeResource::Family, body.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id)
                .await
                .map_err(ApiError)?;
        }
    }
    let id = Uuid::now_v7();
    let citation = CitationRepo::create(
        &txn,
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
    if let Some(person_id) = citation.person_id {
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
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
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Citation, citation_id)
        .await
        .map_err(ApiError)?;
    let previous = CitationRepo::get(&txn, citation_id)
        .await
        .map_err(ApiError::from)?;
    if let Some(source_id) = body.source_id {
        require_tree_resource(&txn, tree_id, TreeResource::Source, source_id)
            .await
            .map_err(ApiError)?;
    }
    let citation = CitationRepo::update(
        &txn,
        citation_id,
        body.source_id,
        body.page,
        body.confidence,
        body.text,
    )
    .await
    .map_err(ApiError::from)?;
    if let Some(person_id) = previous.person_id {
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(Json(serde_json::to_value(citation).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/citations/:citation_id
pub async fn delete_citation(
    State(state): State<AppState>,
    Path((tree_id, citation_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    require_tree_resource(&txn, tree_id, TreeResource::Citation, citation_id)
        .await
        .map_err(ApiError)?;
    let citation = CitationRepo::get(&txn, citation_id)
        .await
        .map_err(ApiError::from)?;
    CitationRepo::delete(&txn, citation_id)
        .await
        .map_err(ApiError::from)?;
    if let Some(person_id) = citation.person_id {
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &[person_id])
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
