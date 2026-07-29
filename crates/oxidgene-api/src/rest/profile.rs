//! REST handlers for the denormalized person projections and pedigrees.
//!
//! - Person projections (ready-to-render, read straight from `person_denorm`)
//! - Full tree rebuild (used after a GEDCOM import)
//! - Projection teardown
//! - Pedigree assembly and expansion
//!
//! Search moved to the normal search path (`GET /persons/search?q=...`)
//! in Sprint E.6 — it is backed by the `person_search_fts` DB table.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde_json::Value;
use uuid::Uuid;

use super::dto::{PedigreeExpandQuery, PedigreeQuery, ProfileDropResponse, ProfileRebuildResponse};
use super::error::ApiError;
use super::state::{AppState, begin_tx, commit_tx};

/// `GET /api/v1/trees/{tree_id}/profiles/{person_id}`
///
/// Returns the denormalized person profile, building it on demand if it has
/// not been materialized yet.
pub async fn get_person_profile(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiError> {
    let profile = state
        .profiles
        .get_or_build_person(&state.db, tree_id, person_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(profile).unwrap()))
}

/// `GET /api/v1/trees/{tree_id}/profiles`
///
/// Returns every person projection of a tree, materializing the tree first if
/// it has never been built.
pub async fn get_person_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let persons = state
        .profiles
        .get_all_persons(&state.db, tree_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(persons).unwrap()))
}

/// `POST /api/v1/trees/{tree_id}/profiles/rebuild`
///
/// Rebuilds every projection of the tree, plus its search rows.
pub async fn rebuild_tree_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<ProfileRebuildResponse>, ApiError> {
    let count = state
        .profiles
        .rebuild_tree_full(&state.db, tree_id)
        .await
        .map_err(ApiError)?;

    Ok(Json(ProfileRebuildResponse {
        rebuilt: true,
        persons_count: count,
    }))
}

/// `POST /api/v1/trees/{tree_id}/profiles/rebuild/{person_id}`
///
/// Rebuilds a single person's projection and search row.
pub async fn rebuild_person_profile(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ProfileRebuildResponse>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    state
        .profiles
        .rebuild_person(&txn, tree_id, person_id)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;

    Ok(Json(ProfileRebuildResponse {
        rebuilt: true,
        persons_count: 1,
    }))
}

/// `POST /api/v1/trees/{tree_id}/profiles/drop`
///
/// Drops every projection and search row of a tree. They are rebuilt lazily on
/// the next read. Useful for debugging or after a bulk operation.
pub async fn drop_tree_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<ProfileDropResponse>, ApiError> {
    let txn = begin_tx(&state.db).await.map_err(ApiError)?;
    state
        .profiles
        .invalidate_tree(&txn, tree_id)
        .await
        .map_err(ApiError)?;
    commit_tx(txn).await.map_err(ApiError)?;

    Ok(Json(ProfileDropResponse { dropped: true }))
}

/// `GET /api/v1/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N`
///
/// Returns a windowed pedigree for the given root person, assembled from the
/// `person_ancestry` closure table joined against the stored projections.
pub async fn get_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeQuery>,
) -> Result<Json<Value>, ApiError> {
    let pedigree = state
        .profiles
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

/// `PATCH /api/v1/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=…&from_depth=…&to_depth=…&other_depth=…`
///
/// Returns only the nodes and edges a pedigree gains when expanded in one
/// direction, so the client can merge a delta rather than re-render.
/// `other_depth` is the depth already loaded in the opposite direction.
pub async fn expand_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeExpandQuery>,
) -> Result<Json<Value>, ApiError> {
    use oxidgene_core::projection::PedigreeDirection;

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

    let delta = state
        .profiles
        .expand_pedigree(
            tree_id,
            root_person_id,
            direction,
            params.from_depth,
            params.to_depth,
            params.other_depth,
        )
        .await
        .map_err(ApiError)?;

    Ok(Json(serde_json::to_value(delta).unwrap()))
}
