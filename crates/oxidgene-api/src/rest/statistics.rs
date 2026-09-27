//! REST handler for a tree's statistics (`docs/ui-statistics.md`).

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::statistics::{self, DEFAULT_INTERVAL, TreeStatistics};

#[derive(Debug, Deserialize)]
pub struct StatisticsQuery {
    /// Width of the periods, in years.
    pub interval: Option<i32>,
}

/// GET /api/v1/trees/:tree_id/statistics?interval=25
pub async fn statistics(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StatisticsQuery>,
) -> Result<Json<TreeStatistics>, ApiError> {
    let interval = query.interval.unwrap_or(DEFAULT_INTERVAL);
    statistics::load(&state.db, &state.profiles, tree_id, interval)
        .await
        .map(Json)
        .map_err(ApiError)
}
