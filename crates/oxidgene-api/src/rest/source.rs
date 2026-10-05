//! REST handlers for Source CRUD operations.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_archives::ArchiveTarget;
use oxidgene_core::types::{Connection, Source};
use oxidgene_db::repo::{PaginationParams, SourceRepo};
use uuid::Uuid;

use super::dto::{ArchiveTargetBody, DeleteSourceQuery, SourceListQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::service::archive;
use crate::service::scope::{TreeResource, require_tree_resource};
use crate::service::source::{self, NewSource, SourcePatch};

/// GET /api/v1/trees/:tree_id/sources
pub async fn list_sources(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<SourceListQuery>,
) -> Result<Json<Connection<Source>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        SourceRepo::list_titled(&state.reader, tree_id, query.title.as_deref(), &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/sources
pub async fn create_source(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewSource>,
) -> Result<(StatusCode, Json<Source>), ApiError> {
    let source = source::create_source(&state.db, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(source)))
}

/// GET /api/v1/trees/:tree_id/sources/:source_id
pub async fn get_source(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Source>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Source, source_id).await?;
    Ok(Json(SourceRepo::get(&state.reader, source_id).await?))
}

/// PUT /api/v1/trees/:tree_id/sources/:source_id
pub async fn update_source(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SourcePatch>,
) -> Result<Json<Source>, ApiError> {
    Ok(Json(
        source::update_source(&state.db, tree_id, source_id, body).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/sources/:source_id
///
/// `only_if_unused` turns the delete into a cleanup: a source still cited
/// anywhere is kept, which the 200/204 split reports back.
pub async fn delete_source(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteSourceQuery>,
) -> Result<StatusCode, ApiError> {
    let deleted =
        source::delete_source(&state.db, tree_id, source_id, query.only_if_unused).await?;
    Ok(if deleted {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::OK
    })
}

/// POST /api/v1/trees/:tree_id/sources/:source_id/archive-target
///
/// A `POST` although it writes nothing: it may send requests to an archive
/// portal, one per reader's click, and its answer is not to be cached by
/// the HTTP layer.
pub async fn archive_target(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ArchiveTargetBody>,
) -> Result<Json<ArchiveTarget>, ApiError> {
    Ok(Json(
        archive::archive_target(
            &state.reader,
            &state.archives,
            tree_id,
            source_id,
            body.citation_id,
        )
        .await?,
    ))
}
