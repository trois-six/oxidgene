//! REST handlers for MediaLink create/delete operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::OxidGeneError;
use oxidgene_db::repo::{MediaLinkRepo, MediaLinkTarget};
use uuid::Uuid;

use super::dto::{CreateMediaLinkRequest, MediaLinkListQuery, MediaLinkListRow, MediaWithLink};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/media-links
///
/// Unfiltered, this is the tree-wide list the pedigree canvas uses to find
/// each person's photo. With `entity_type` + `entity_id` it is one entity's
/// gallery instead, and each row carries the media itself — the grid needs the
/// MIME type and the thumbnail's existence to draw a tile, and asking for
/// those separately would be a request per tile.
pub async fn list_media_links(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<MediaLinkListQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let (Some(entity_type), Some(entity_id)) = (&query.entity_type, query.entity_id) {
        let target = MediaLinkTarget::parse(entity_type).ok_or_else(|| {
            ApiError(OxidGeneError::Validation(format!(
                "unknown entity_type `{entity_type}`; expected person, family, event or source"
            )))
        })?;
        let rows = MediaLinkRepo::list_with_media(&state.db, target, entity_id)
            .await
            .map_err(ApiError::from)?;
        let response: Vec<MediaWithLink> = rows
            .into_iter()
            .map(|(link, media)| MediaWithLink {
                link_id: link.id,
                sort_order: link.sort_order,
                is_profile: link.is_profile,
                media,
            })
            .collect();
        return Ok(Json(serde_json::to_value(response).unwrap()));
    }
    if query.entity_type.is_some() || query.entity_id.is_some() {
        return Err(ApiError(OxidGeneError::Validation(
            "entity_type and entity_id must be given together".into(),
        )));
    }

    let db_rows = MediaLinkRepo::list_for_tree(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    let response: Vec<MediaLinkListRow> = db_rows
        .into_iter()
        .map(|r| MediaLinkListRow {
            entity_id: r.entity_id,
            entity_type: r.entity_type,
            media_id: r.media_id,
            file_path: r.file_path,
            file_name: r.file_name,
        })
        .collect();
    Ok(Json(serde_json::to_value(response).unwrap()))
}

/// PUT /api/v1/trees/:tree_id/media-links/:link_id/profile
///
/// Make this link the person's profile image, or clear it with
/// `{"is_profile": false}`. Setting one clears the person's others in the same
/// statement, so the tree never shows two stars.
pub async fn set_profile_media_link(
    State(state): State<AppState>,
    Path((tree_id, link_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<super::dto::SetProfileMediaLinkRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let link = if body.is_profile {
        MediaLinkRepo::set_profile(&state.db, link_id).await
    } else {
        MediaLinkRepo::clear_profile(&state.db, link_id).await
    }
    .map_err(ApiError::from)?;

    // The profile photo is embedded in `person_denorm`, so the projection has
    // to be rebuilt or the tree keeps drawing the old portrait.
    if let Some(person_id) = link.person_id {
        state
            .profiles
            .rebuild_person(&state.db, tree_id, person_id)
            .await
            .map_err(ApiError::from)?;
    }
    Ok(Json(serde_json::to_value(link).unwrap()))
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
