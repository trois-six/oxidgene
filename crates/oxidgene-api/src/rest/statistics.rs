//! REST handler for a tree's statistics (`docs/ui-statistics.md`).

use axum::Json;
use axum::extract::{Path, State};
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::statistics::{self, TreeStatistics};

/// GET /api/v1/trees/:tree_id/statistics
pub async fn statistics(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeStatistics>, ApiError> {
    statistics::load(&state.db, &state.profiles, tree_id)
        .await
        .map(Json)
        .map_err(ApiError)
}
