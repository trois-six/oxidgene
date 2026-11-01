//! REST handlers of the access log (docs/authorization.md §9.2). Web build
//! only (`auth` feature).

use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{AccessEvent, PaginationParams};
use uuid::Uuid;

use super::authorize::Viewer;
use super::dto::PaginationQuery;
use super::error::ApiError;
use super::state::AppState;
use crate::service::access_log;

fn params(query: PaginationQuery) -> PaginationParams {
    PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    }
}

/// GET /api/v1/trees/{tree_id}/access-log
pub async fn tree_events(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<AccessEvent>>, ApiError> {
    Ok(Json(
        access_log::tree_events(&viewer, &state.reader, tree_id, &params(query)).await?,
    ))
}

/// GET /api/v1/admin/access-log
pub async fn instance_events(
    State(state): State<AppState>,
    viewer: Viewer,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<AccessEvent>>, ApiError> {
    Ok(Json(
        access_log::instance_events(&viewer, &state.reader, &params(query)).await?,
    ))
}
