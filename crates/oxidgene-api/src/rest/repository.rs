//! REST handlers for repositories and a source's links to them.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use oxidgene_core::types::{Connection, Repository, SourceRepository};
use oxidgene_db::repo::PaginationParams;
use uuid::Uuid;

use super::dto::{DeleteRepositoryQuery, HeldSource, PaginationQuery};
use super::error::ApiError;
use super::state::AppState;
use crate::service::repository::{
    self, NewRepository, NewSourceRepository, RepositoryPatch, SourceRepositoryPatch,
};

/// GET /api/v1/trees/:tree_id/repositories
pub async fn list_repositories(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<Repository>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        repository::list_repositories(&state.reader, tree_id, &params).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/repositories
pub async fn create_repository(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<NewRepository>,
) -> Result<(StatusCode, Json<Repository>), ApiError> {
    let created = repository::create_repository(&state.db, tree_id, body).await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// GET /api/v1/trees/:tree_id/repositories/:repository_id
pub async fn get_repository(
    State(state): State<AppState>,
    Path((tree_id, repository_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Repository>, ApiError> {
    Ok(Json(
        repository::get_repository(&state.reader, tree_id, repository_id).await?,
    ))
}

/// PUT /api/v1/trees/:tree_id/repositories/:repository_id
pub async fn update_repository(
    State(state): State<AppState>,
    Path((tree_id, repository_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<RepositoryPatch>,
) -> Result<Json<Repository>, ApiError> {
    Ok(Json(
        repository::update_repository(&state.db, tree_id, repository_id, body).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/repositories/:repository_id
///
/// `only_if_unused` turns the delete into a cleanup: a repository still
/// holding a source, or still the subject of a note, is kept, which the
/// 200/204 split reports back.
pub async fn delete_repository(
    State(state): State<AppState>,
    Path((tree_id, repository_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<DeleteRepositoryQuery>,
) -> Result<StatusCode, ApiError> {
    let deleted =
        repository::delete_repository(&state.db, tree_id, repository_id, query.only_if_unused)
            .await?;
    Ok(if deleted {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::OK
    })
}

/// GET /api/v1/trees/:tree_id/repositories/:repository_id/sources
///
/// The live sources the repository holds, one per call number, each with
/// its source.
pub async fn list_repository_sources(
    State(state): State<AppState>,
    Path((tree_id, repository_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<HeldSource>>, ApiError> {
    let held = repository::held_sources(&state.reader, tree_id, repository_id).await?;
    Ok(Json(
        held.into_iter()
            .map(|(link, source)| HeldSource { link, source })
            .collect(),
    ))
}

/// GET /api/v1/trees/:tree_id/sources/:source_id/repositories
pub async fn list_source_repositories(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<SourceRepository>>, ApiError> {
    Ok(Json(
        repository::list_source_repositories(&state.reader, tree_id, source_id).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/sources/:source_id/repositories
pub async fn add_source_repository(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<NewSourceRepository>,
) -> Result<(StatusCode, Json<SourceRepository>), ApiError> {
    let link = repository::add_source_repository(&state.db, tree_id, source_id, body).await?;
    Ok((StatusCode::CREATED, Json(link)))
}

/// PUT /api/v1/trees/:tree_id/sources/:source_id/repositories/:link_id
pub async fn update_source_repository(
    State(state): State<AppState>,
    Path((tree_id, source_id, link_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(body): Json<SourceRepositoryPatch>,
) -> Result<Json<SourceRepository>, ApiError> {
    Ok(Json(
        repository::update_source_repository(&state.db, tree_id, source_id, link_id, body).await?,
    ))
}

/// DELETE /api/v1/trees/:tree_id/sources/:source_id/repositories/:link_id
pub async fn remove_source_repository(
    State(state): State<AppState>,
    Path((tree_id, source_id, link_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    repository::remove_source_repository(&state.db, tree_id, source_id, link_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
