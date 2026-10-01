//! REST handler for a tree's statistics (`docs/ui-statistics.md`).

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::statistics::growth::TreeGrowth;
use crate::service::statistics::{self, TreeStatistics};

#[derive(Debug, Deserialize)]
pub struct StatisticsQuery {
    /// Let ages and averages use dates about, calculated or estimated.
    #[serde(default)]
    pub approximate: bool,
    /// The language places' countries, regions and subdivisions are named
    /// in; English when omitted.
    pub lang: Option<String>,
}

/// GET /api/v1/trees/:tree_id/statistics?approximate=false&lang=en
pub async fn statistics(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StatisticsQuery>,
) -> Result<Json<TreeStatistics>, ApiError> {
    let lang = statistics::language(query.lang.as_deref())?;
    Ok(Json(
        statistics::load(&state.db, &state.profiles, tree_id, query.approximate, lang).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/statistics/growth
pub async fn growth(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeGrowth>, ApiError> {
    Ok(Json(statistics::growth::load(&state.db, tree_id).await?))
}
