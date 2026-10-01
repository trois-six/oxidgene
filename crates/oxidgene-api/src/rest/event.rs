//! REST handlers for Event CRUD operations and event witnesses.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Event, EventWitness};
use oxidgene_db::repo::{EventFilter, EventRepo, PaginationParams};
use uuid::Uuid;

use super::dto::EventListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::event::{self, EventPatch, NewEvent, NewWitness};
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/events
pub async fn list_events(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<EventListQuery>,
) -> Result<Json<Connection<Event>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let filter = EventFilter {
        event_type: query.event_type,
        person_id: query.person_id,
        family_id: query.family_id,
    };
    Ok(Json(
        event::list_events(&state.reader, tree_id, &filter, &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/events
pub async fn create_event(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewEvent>,
) -> Result<(StatusCode, Json<Event>), ApiError> {
    let event = event::create_event(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(event)))
}

/// GET /api/v1/trees/:tree_id/events/:event_id
pub async fn get_event(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Event>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Event, event_id).await?;
    Ok(Json(EventRepo::get(&state.reader, event_id).await?))
}

/// PUT /api/v1/trees/:tree_id/events/:event_id
pub async fn update_event(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<EventPatch>,
) -> Result<Json<Event>, ApiError> {
    let event = event::update_event(&state.db, &state.profiles, tree_id, event_id, body).await?;
    Ok(Json(event))
}

/// DELETE /api/v1/trees/:tree_id/events/:event_id
pub async fn delete_event(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    event::delete_event(&state.db, &state.profiles, tree_id, event_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/events/:event_id/witnesses
pub async fn list_witnesses(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<EventWitness>>, ApiError> {
    Ok(Json(
        event::list_witnesses(&state.reader, tree_id, event_id).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/events/:event_id/witnesses
pub async fn add_witness(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewWitness>,
) -> Result<(StatusCode, Json<EventWitness>), ApiError> {
    let witness = event::add_witness(&state.db, tree_id, event_id, body).await?;
    Ok((StatusCode::CREATED, Json(witness)))
}

/// DELETE /api/v1/trees/:tree_id/events/:event_id/witnesses/:witness_id
pub async fn remove_witness(
    State(state): State<AppState>,
    Path((tree_id, event_id, witness_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    event::remove_witness(&state.db, tree_id, Some(event_id), witness_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
