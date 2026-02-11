//! REST handlers for Event CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{EventFilter, EventRepo, PaginationParams};
use uuid::Uuid;

use super::dto::{CreateEventRequest, EventListQuery, UpdateEventRequest};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/events
pub async fn list_events(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<EventListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let filter = EventFilter {
        event_type: query.event_type,
        person_id: query.person_id,
        family_id: query.family_id,
    };
    let connection = EventRepo::list(&state.db, tree_id, &filter, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(connection).unwrap()))
}

/// POST /api/v1/trees/:tree_id/events
pub async fn create_event(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateEventRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let event = EventRepo::create(
        &state.db,
        id,
        tree_id,
        body.event_type,
        body.date_value,
        body.date_sort,
        body.place_id,
        body.person_id,
        body.family_id,
        body.description,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(event).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/events/:event_id
pub async fn get_event(
    State(state): State<AppState>,
    Path((_tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let event = EventRepo::get(&state.db, event_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(event).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/events/:event_id
pub async fn update_event(
    State(state): State<AppState>,
    Path((_tree_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let event = EventRepo::update(
        &state.db,
        event_id,
        body.event_type,
        body.date_value,
        body.date_sort,
        body.place_id,
        body.description,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(event).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/events/:event_id
pub async fn delete_event(
    State(state): State<AppState>,
    Path((_tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    EventRepo::delete(&state.db, event_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
