//! REST handlers for the read models of a tree's Tools page
//! (`docs/ui-tools.md`), each computed on request by its service.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use crate::service::ancestry::{self, AncestryCompleteness};
use crate::service::anomalies::{self, TreeAnomalies};
use crate::service::duplicates::{self, PotentialDuplicates};
use crate::service::statistics::PlaceUsage;

#[derive(Debug, Deserialize)]
pub struct AncestryQuery {
    /// Generations to cover, the root's included; 8 when omitted.
    pub generations: Option<i64>,
}

/// GET /api/v1/trees/:tree_id/anomalies
pub async fn tree_anomalies(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<TreeAnomalies>, ApiError> {
    anomalies::load(&state.db, &state.profiles, tree_id)
        .await
        .map(Json)
        .map_err(ApiError)
}

/// GET /api/v1/trees/:tree_id/duplicates
pub async fn potential_duplicates(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<PotentialDuplicates>, ApiError> {
    duplicates::load_potential_duplicates(&state.db, &state.profiles, tree_id)
        .await
        .map(Json)
        .map_err(ApiError)
}

/// GET /api/v1/trees/:tree_id/unlocated-places
pub async fn unlocated_places(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<PlaceUsage>>, ApiError> {
    anomalies::load_unlocated_places(&state.db, tree_id)
        .await
        .map(Json)
        .map_err(ApiError)
}

/// GET /api/v1/trees/:tree_id/ancestry-completeness?generations=8
pub async fn ancestry_completeness(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<AncestryQuery>,
) -> Result<Json<AncestryCompleteness>, ApiError> {
    let generations = ancestry::generations(query.generations).map_err(ApiError)?;
    ancestry::load(&state.db, &state.profiles, tree_id, generations)
        .await
        .map(Json)
        .map_err(ApiError)
}
