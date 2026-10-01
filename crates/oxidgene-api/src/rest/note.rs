//! REST handlers for Note CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::{NoteFilter, NoteRepo, PaginationParams};
use uuid::Uuid;

use super::dto::{CreateNoteRequest, NoteListQuery, UpdateNoteRequest};
use super::error::ApiError;
use super::state::AppState;
use crate::service::note::{self, NewNote};
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/notes
pub async fn list_notes(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let filter = NoteFilter {
        person_id: query.person_id,
        event_id: query.event_id,
        family_id: query.family_id,
        source_id: query.source_id,
        media_id: query.media_id,
    };
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let notes = NoteRepo::list(&state.db, tree_id, &filter, &params)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(notes).unwrap()))
}

/// POST /api/v1/trees/:tree_id/notes
pub async fn create_note(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<CreateNoteRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let new = NewNote {
        text: body.text,
        person_id: body.person_id,
        event_id: body.event_id,
        family_id: body.family_id,
        source_id: body.source_id,
        media_id: body.media_id,
    };
    let note = note::create_note(&state.db, &state.profiles, tree_id, new)
        .await
        .map_err(ApiError)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(note).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/notes/:note_id
pub async fn get_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_tree_resource(&state.db, tree_id, TreeResource::Note, note_id)
        .await
        .map_err(ApiError)?;
    let note = NoteRepo::get(&state.db, note_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(note).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/notes/:note_id
pub async fn update_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateNoteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let note = note::update_note(&state.db, &state.profiles, tree_id, note_id, body.text)
        .await
        .map_err(ApiError)?;
    Ok(Json(serde_json::to_value(note).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/notes/:note_id
pub async fn delete_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    note::delete_note(&state.db, &state.profiles, tree_id, note_id)
        .await
        .map_err(ApiError)?;
    Ok(StatusCode::NO_CONTENT)
}
