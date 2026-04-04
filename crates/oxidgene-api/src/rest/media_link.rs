//! REST handlers for MediaLink create/delete operations.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use oxidgene_db::repo::MediaLinkRepo;
use uuid::Uuid;

use super::dto::{CreateMediaLinkRequest, MediaLinkListRow};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/media-links
pub async fn list_media_links(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<MediaLinkListRow>>, ApiError> {
    let db_rows = MediaLinkRepo::list_for_tree(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    let response = db_rows
        .into_iter()
        .map(|r| MediaLinkListRow {
            entity_id: r.entity_id,
            entity_type: r.entity_type,
            media_id: r.media_id,
            file_path: r.file_path,
            file_name: r.file_name,
        })
        .collect();
    Ok(Json(response))
}

/// POST /api/v1/trees/:tree_id/media-links
pub async fn create_media_link(
    State(state): State<AppState>,
    Path(_tree_id): Path<Uuid>,
    Json(body): Json<CreateMediaLinkRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), ApiError> {
    let id = Uuid::now_v7();
    let link = MediaLinkRepo::create(
        &state.db,
        id,
        body.media_id,
        body.person_id,
        body.event_id,
        body.source_id,
        body.family_id,
        body.sort_order,
    )
    .await
    .map_err(ApiError::from)?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(link).unwrap()),
    ))
}

/// DELETE /api/v1/trees/:tree_id/media-links/:link_id
pub async fn delete_media_link(
    State(state): State<AppState>,
    Path((_tree_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    MediaLinkRepo::delete(&state.db, link_id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
