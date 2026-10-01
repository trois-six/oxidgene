//! REST handlers for MediaLink create/delete operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use oxidgene_core::types::MediaLink;
use oxidgene_db::repo::{MediaLinkRepo, MediaLinkTarget};
use uuid::Uuid;

use super::dto::{MediaLinkListQuery, MediaLinkListRow, MediaWithLink};
use super::error::ApiError;
use super::state::AppState;
use crate::service::media_link::{self, NewMediaLink};
use crate::service::scope::{TreeResource, require_tree_resource};

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
) -> Result<Response, ApiError> {
    if let (Some(entity_type), Some(entity_id)) = (&query.entity_type, query.entity_id) {
        let target = MediaLinkTarget::parse(entity_type).ok_or_else(|| {
            ApiError(OxidGeneError::Validation(format!(
                "unknown entity_type `{entity_type}`; expected person, family, event or source"
            )))
        })?;
        let resource = match target {
            MediaLinkTarget::Person => TreeResource::Person,
            MediaLinkTarget::Family => TreeResource::Family,
            MediaLinkTarget::Event => TreeResource::Event,
            MediaLinkTarget::Source => TreeResource::Source,
        };
        require_tree_resource(&state.db, tree_id, resource, entity_id).await?;
        let rows = MediaLinkRepo::list_with_media(&state.db, target, entity_id).await?;
        let response: Vec<MediaWithLink> = rows
            .into_iter()
            .map(|(link, media)| MediaWithLink {
                link_id: link.id,
                sort_order: link.sort_order,
                media,
            })
            .collect();
        return Ok(Json(response).into_response());
    }
    if query.entity_type.is_some() || query.entity_id.is_some() {
        return Err(ApiError(OxidGeneError::Validation(
            "entity_type and entity_id must be given together".into(),
        )));
    }

    // The other direction: what one file is attached to. Answering this is
    // what lets a media's own panel say which events it documents, without
    // the caller having to walk every event's gallery to find out.
    if let Some(media_id) = query.media_id {
        require_tree_resource(&state.db, tree_id, TreeResource::Media, media_id).await?;
        let links = MediaLinkRepo::list_by_media(&state.db, media_id).await?;
        return Ok(Json(links).into_response());
    }

    let db_rows = MediaLinkRepo::list_for_tree(&state.db, tree_id).await?;
    let response: Vec<MediaLinkListRow> = db_rows
        .into_iter()
        .map(|r| MediaLinkListRow {
            link_id: r.link_id,
            entity_id: r.entity_id,
            entity_type: r.entity_type,
            media_id: r.media_id,
            file_path: r.file_path,
            file_name: r.file_name,
            mime_type: r.mime_type,
            has_thumbnail: r.has_thumbnail,
        })
        .collect();
    Ok(Json(response).into_response())
}

/// POST /api/v1/trees/:tree_id/media-links
pub async fn create_media_link(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewMediaLink>,
) -> Result<(StatusCode, Json<MediaLink>), ApiError> {
    let link = media_link::create_media_link(&state.db, &state.profiles, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(link)))
}

/// DELETE /api/v1/trees/:tree_id/media-links/:link_id
pub async fn delete_media_link(
    State(state): State<AppState>,
    Path((tree_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    media_link::delete_media_link(&state.db, &state.profiles, tree_id, link_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
