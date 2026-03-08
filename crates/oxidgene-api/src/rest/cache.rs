//! REST handlers for cache operations.
//!
//! These endpoints provide access to the server-side cache layer:
//! - Cached person data (denormalised, ready-to-render)
//! - Full tree cache rebuild (used after GEDCOM import)
//! - Cached search (server-side, accent-folded)
//! - Cache invalidation

use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::Value;
use uuid::Uuid;

use super::dto::{
    CacheInvalidateResponse, CacheRebuildResponse, CacheSearchQuery, PedigreeExpandQuery,
    PedigreeQuery,
};
use super::error::ApiError;
use super::state::AppState;

/// `GET /api/v1/trees/{tree_id}/cache/persons/{person_id}`
///
/// Returns the cached (denormalised) person profile. Falls back to building
/// from DB if not yet cached.
pub async fn get_cached_person(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let cached = state
        .cache
        .get_or_build_person(tree_id, person_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(cached).unwrap()))
}

/// `GET /api/v1/trees/{tree_id}/cache/persons`
///
/// Returns all cached persons for a tree. If the tree cache is empty, triggers
/// a full rebuild first.
pub async fn get_cached_persons(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    // Try to get from cache first
    let persons = state
        .cache
        .store()
        .get_all_persons(tree_id)
        .await
        .map_err(ApiError)?;

    if persons.is_empty() {
        // Cache is cold — build it
        state
            .cache
            .rebuild_tree_full(tree_id)
            .await
            .map_err(ApiError)?;
        let persons = state
            .cache
            .store()
            .get_all_persons(tree_id)
            .await
            .map_err(ApiError)?;
        return Ok(Json(serde_json::to_value(persons).unwrap()));
    }

    Ok(Json(serde_json::to_value(persons).unwrap()))
}

/// `POST /api/v1/trees/{tree_id}/cache/rebuild`
///
/// Triggers a full cache rebuild for the tree (all persons + search index).
pub async fn rebuild_tree_cache(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<CacheRebuildResponse>, ApiError> {
    let count = state
        .cache
        .rebuild_tree_full(tree_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(CacheRebuildResponse {
        rebuilt: true,
        persons_count: count,
    }))
}

/// `POST /api/v1/trees/{tree_id}/cache/rebuild/{person_id}`
///
/// Rebuilds the cache for a single person (and their affected set).
pub async fn rebuild_person_cache(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<CacheRebuildResponse>, ApiError> {
    state
        .cache
        .rebuild_person(tree_id, person_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(CacheRebuildResponse {
        rebuilt: true,
        persons_count: 1,
    }))
}

/// `GET /api/v1/trees/{tree_id}/cache/search?q=...&limit=...&offset=...`
///
/// Server-side search across all persons in a tree. Uses the cached search
/// index with accent-folding and normalised matching.
pub async fn search_cached(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(params): Query<CacheSearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = params.limit.unwrap_or(25).min(100);
    let offset = params.offset.unwrap_or(0);

    let results = state
        .cache
        .search(tree_id, &params.q, limit, offset)
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(results).unwrap()))
}

/// `POST /api/v1/trees/{tree_id}/cache/invalidate`
///
/// Drops all caches for a tree. Useful for debugging or after bulk operations.
pub async fn invalidate_tree_cache(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<CacheInvalidateResponse>, ApiError> {
    state
        .cache
        .invalidate_tree(tree_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(CacheInvalidateResponse { invalidated: true }))
}

/// `GET /api/v1/trees/{tree_id}/cache/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N`
///
/// Returns a windowed pedigree for the given root person. If no pedigree
/// cache exists yet, it is built lazily from the PersonAncestry closure table
/// and the PersonCache.
pub async fn get_cached_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeQuery>,
) -> Result<Json<Value>, ApiError> {
    let pedigree = state
        .cache
        .get_or_build_pedigree(
            tree_id,
            root_person_id,
            params.ancestor_depth,
            params.descendant_depth,
        )
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(pedigree).unwrap()))
}

/// `PATCH /api/v1/trees/{tree_id}/cache/pedigree/{root_person_id}/expand?direction=...&from_depth=...&to_depth=...`
///
/// Expands an existing pedigree cache in one direction. Returns only the new
/// nodes and edges as a [`PedigreeDelta`]. The client merges the delta into
/// its current view.
pub async fn expand_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeExpandQuery>,
) -> Result<Json<Value>, ApiError> {
    use oxidgene_cache::types::PedigreeDirection;

    let direction = match params.direction.as_str() {
        "ancestors" => PedigreeDirection::Ancestors,
        "descendants" => PedigreeDirection::Descendants,
        _ => {
            return Err(ApiError(oxidgene_core::error::OxidGeneError::Validation(
                format!(
                    "Invalid direction '{}': must be 'ancestors' or 'descendants'",
                    params.direction
                ),
            )));
        }
    };

    if params.to_depth <= params.from_depth {
        return Err(ApiError(oxidgene_core::error::OxidGeneError::Validation(
            format!(
                "to_depth ({}) must be greater than from_depth ({})",
                params.to_depth, params.from_depth
            ),
        )));
    }

    let additional_levels = params.to_depth - params.from_depth;

    let delta = state
        .cache
        .expand_pedigree(tree_id, root_person_id, direction, additional_levels)
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(delta).unwrap()))
}
