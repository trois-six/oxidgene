//! REST handlers for Note CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_db::repo::NoteRepo;
use uuid::Uuid;

use super::dto::{CreateNoteRequest, NoteListQuery, UpdateNoteRequest};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/notes
pub async fn list_notes(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let notes = NoteRepo::list_by_entity(
        &state.db,
        tree_id,
        query.person_id,
        query.event_id,
        query.family_id,
        query.source_id,
    )
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
    if body.text.trim().is_empty() {
        return Err(ApiError(oxidgene_core::OxidGeneError::Validation(
            "text must not be empty".to_string(),
        )));
    }
    let id = Uuid::now_v7();
    let note = NoteRepo::create(
        &state.db,
        id,
        tree_id,
        body.text,
        body.person_id,
        body.event_id,
        body.family_id,
        body.source_id,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(note).unwrap()),
    ))
}

/// GET /api/v1/trees/:tree_id/notes/:note_id
pub async fn get_note(
    State(state): State<AppState>,
    Path((_tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let note = NoteRepo::get(&state.db, note_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(note).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/notes/:note_id
pub async fn update_note(
    State(state): State<AppState>,
    Path((_tree_id, note_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateNoteRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let note = NoteRepo::update(&state.db, note_id, body.text)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::to_value(note).unwrap()))
}

/// DELETE /api/v1/trees/:tree_id/notes/:note_id
pub async fn delete_note(
    State(state): State<AppState>,
    Path((_tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    NoteRepo::delete(&state.db, note_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
