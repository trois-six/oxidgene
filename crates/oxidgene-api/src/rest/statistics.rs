//! REST handler for a tree's statistics (`docs/ui-statistics.md`).

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use super::authorize::Viewer;
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
    /// Only the persons carrying this tag; the whole tree when omitted.
    pub tag: Option<Uuid>,
}

/// GET /api/v1/trees/:tree_id/statistics?approximate=false&lang=en&tag=<tag_id>
pub async fn statistics(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<StatisticsQuery>,
) -> Result<Json<TreeStatistics>, ApiError> {
    let lang = statistics::language(query.lang.as_deref())?;
    let redaction = viewer.redaction(&state, tree_id).await?;
    Ok(Json(
        statistics::load(
            &state.reader,
            &state.profiles,
            &redaction,
            tree_id,
            query.approximate,
            lang,
            query.tag,
        )
        .await?,
    ))
}

/// GET /api/v1/trees/:tree_id/statistics/growth
pub async fn growth(
    State(state): State<AppState>,
    viewer: Viewer,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeGrowth>, ApiError> {
    let redaction = viewer.redaction(&state, tree_id).await?;
    Ok(Json(
        statistics::growth::load(&state.reader, &redaction, tree_id).await?,
    ))
}
