//! REST handlers for Event CRUD operations.

use crate::profile::invalidation;
use crate::service::event_date;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{EventFilter, EventRepo, EventWitnessRepo, PaginationParams};
use uuid::Uuid;

use super::dto::{AddEventWitnessRequest, CreateEventRequest, EventListQuery, UpdateEventRequest};
use super::error::ApiError;
use super::state::{AppState, begin_tx, commit_tx};

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
    // Derived here, never taken from the request — see `service::event_date`.
    let date_sort = event_date::derive(body.calendar, body.date_value.as_deref());
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    let event = EventRepo::create(
        &txn,
        id,
        tree_id,
        body.event_type,
        body.date_value,
        date_sort,
        body.place_id,
        body.person_id,
        body.family_id,
        body.description,
        body.date_qualifier,
        body.date_value2,
        body.calendar,
        body.cause,
    )
    .await
    .map_err(ApiError::from)?;
    // Invalidate: person event or family event.
    if let Some(pid) = body.person_id {
        let affected = invalidation::affected_persons(&txn, pid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    } else if let Some(fid) = body.family_id {
        let affected = invalidation::affected_persons_for_family(&txn, fid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
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
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateEventRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    // Derived from the patched state, reading whichever half the patch leaves
    // alone off the stored event — see `service::event_date`.
    let stored = EventRepo::get(&txn, event_id)
        .await
        .map_err(ApiError::from)?;
    let date_sort = Some(event_date::derive_patch(
        stored.calendar,
        stored.date_value.as_deref(),
        body.calendar,
        body.date_value.as_ref().map(Option::as_deref),
    ));
    let event = EventRepo::update(
        &txn,
        event_id,
        body.event_type,
        body.date_value,
        date_sort,
        body.place_id,
        body.description,
        body.date_qualifier,
        body.date_value2,
        body.calendar,
        body.cause,
    )
    .await
    .map_err(ApiError::from)?;
    // Invalidate based on event ownership.
    if let Some(pid) = event.person_id {
        let affected = invalidation::affected_persons(&txn, pid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    } else if let Some(fid) = event.family_id {
        let affected = invalidation::affected_persons_for_family(&txn, fid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(Json(serde_json::to_value(event).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/events/:event_id
pub async fn delete_event(
    State(state): State<AppState>,
    Path((tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    let event = EventRepo::get(&txn, event_id)
        .await
        .map_err(ApiError::from)?;
    EventRepo::delete(&txn, event_id)
        .await
        .map_err(ApiError::from)?;
    if let Some(pid) = event.person_id {
        let affected = invalidation::affected_persons(&txn, pid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    } else if let Some(fid) = event.family_id {
        let affected = invalidation::affected_persons_for_family(&txn, fid)
            .await
            .map_err(ApiError)?;
        state
            .profiles
            .invalidate_for_mutation(&txn, tree_id, &affected)
            .await
            .map_err(ApiError)?;
    }
    commit_tx(txn).await.map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/trees/:tree_id/events/:event_id/witnesses
pub async fn list_witnesses(
    State(state): State<AppState>,
    Path((_tree_id, event_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let witnesses = EventWitnessRepo::list_by_event(&state.db, event_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(witnesses).unwrap()))
}

/// POST /api/v1/trees/:tree_id/events/:event_id/witnesses
pub async fn add_witness(
    State(state): State<AppState>,
    Path((_tree_id, event_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AddEventWitnessRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let witness = EventWitnessRepo::create(
        &state.db,
        id,
        event_id,
        body.person_id,
        body.relation,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(witness).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/events/:event_id/witnesses/:witness_id
pub async fn remove_witness(
    State(state): State<AppState>,
    Path((_tree_id, _event_id, witness_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    EventWitnessRepo::delete(&state.db, witness_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
