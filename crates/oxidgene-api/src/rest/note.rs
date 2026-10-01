//! REST handlers for Note CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Note};
use oxidgene_db::repo::{NoteFilter, NoteRepo, PaginationParams};
use uuid::Uuid;

use super::dto::NoteListQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::note::{self, NewNote, NotePatch};

/// GET /api/v1/trees/:tree_id/notes
pub async fn list_notes(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<NoteListQuery>,
) -> Result<Json<Connection<Note>>, ApiError> {
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
    Ok(Json(
        NoteRepo::list(&state.reader, tree_id, &filter, &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/notes
pub async fn create_note(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewNote>,
) -> Result<(StatusCode, Json<Note>), ApiError> {
    let note = note::create_note(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(note)))
}

/// GET /api/v1/trees/:tree_id/notes/:note_id
pub async fn get_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Note>, ApiError> {
    Ok(Json(note::get_note(&state.reader, tree_id, note_id).await?))
}

/// PUT /api/v1/trees/:tree_id/notes/:note_id
pub async fn update_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NotePatch>,
) -> Result<Json<Note>, ApiError> {
    let note = note::update_note(&state.db, &state.profiles, tree_id, note_id, body).await?;
    Ok(Json(note))
}

/// DELETE /api/v1/trees/:tree_id/notes/:note_id
pub async fn delete_note(
    State(state): State<AppState>,
    Path((tree_id, note_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    note::delete_note(&state.db, &state.profiles, tree_id, note_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
