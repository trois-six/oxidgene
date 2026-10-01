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
use oxidgene_core::projection::{Pedigree, PedigreeDelta, PersonProfile};
use uuid::Uuid;

use super::dto::{
    PaginationQuery, PedigreeExpandQuery, PedigreeQuery, PedigreesRequest, ProfileDropResponse,
    ProfileRebuildResponse,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::scope::{begin_tx, commit_tx};

/// `GET /api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle`
///
/// Returns only the family neighborhood and evidence rendered by the person
/// detail page, assembled in bounded batch queries.
pub async fn get_person_detail_bundle(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<crate::service::person_detail::PersonDetailBundle>, ApiError> {
    let bundle =
        crate::service::person_detail::load_person_detail_bundle(&state.reader, tree_id, person_id)
            .await?;
    Ok(Json(bundle))
}

/// `GET /api/v1/trees/{tree_id}/profiles/{person_id}`
///
/// Returns the denormalized person profile, building it on demand if it has
/// not been materialized yet.
pub async fn get_person_profile(
    State(state): State<AppState>,
    Path((tree_id, person_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<PersonProfile>, ApiError> {
    let profile = state
        .profiles
        .get_or_build_person(tree_id, person_id)
        .await?;

    Ok(Json(profile))
}

/// `GET /api/v1/trees/{tree_id}/profiles?first=N&after=CURSOR`
///
/// One page of the tree's person projections, by person id, materializing
/// the tree first if it has never been built.
pub async fn get_person_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<oxidgene_core::types::Connection<PersonProfile>>, ApiError> {
    let params = oxidgene_db::repo::PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(state.profiles.persons_page(tree_id, &params).await?))
}

/// `POST /api/v1/trees/{tree_id}/profiles/rebuild`
///
/// Rebuilds every projection of the tree, plus its search rows.
pub async fn rebuild_tree_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<ProfileRebuildResponse>, ApiError> {
    let count = state.profiles.rebuild_tree_full(&state.db, tree_id).await?;

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
    let txn = begin_tx(&state.db).await?;
    state
        .profiles
        .rebuild_person(&txn, tree_id, person_id)
        .await?;
    commit_tx(txn).await?;

    Ok(Json(ProfileRebuildResponse {
        rebuilt: true,
        persons_count: 1,
    }))
}

/// `DELETE /api/v1/trees/{tree_id}/profiles`
///
/// Drops every projection and search row of a tree. They are rebuilt lazily on
/// the next read. Useful for debugging or after a bulk operation.
pub async fn drop_tree_profiles(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<ProfileDropResponse>, ApiError> {
    let txn = begin_tx(&state.db).await?;
    state.profiles.invalidate_tree(&txn, tree_id).await?;
    commit_tx(txn).await?;

    Ok(Json(ProfileDropResponse { dropped: true }))
}

/// `GET /api/v1/trees/{tree_id}/pedigree/{root_person_id}?ancestor_depth=N&descendant_depth=N`
///
/// Returns a windowed pedigree for the given root person, assembled by walking
/// the family links and joining the reached persons against the stored
/// projections.
pub async fn get_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeQuery>,
) -> Result<Json<Pedigree>, ApiError> {
    let pedigree = crate::service::pedigrees::pedigree(
        &state.profiles,
        tree_id,
        root_person_id,
        params.ancestor_depth.into(),
        params.descendant_depth.into(),
    )
    .await?;

    Ok(Json(pedigree))
}

/// `POST /api/v1/trees/{tree_id}/pedigrees`
///
/// Assemble the pedigrees of several roots in one operation, for a screen that
/// draws one small pedigree per row. Request order is preserved and a root that
/// cannot be assembled is omitted rather than failing the batch.
pub async fn load_pedigrees(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<PedigreesRequest>,
) -> Result<Json<Vec<crate::service::pedigrees::PedigreeEntry>>, ApiError> {
    let entries = crate::service::pedigrees::load_pedigrees(
        &state.profiles,
        tree_id,
        &body.root_person_ids,
        body.ancestor_depth.into(),
        body.descendant_depth.into(),
    )
    .await?;
    Ok(Json(entries))
}

/// `GET /api/v1/trees/{tree_id}/pedigree/{root_person_id}/expand?direction=…&from_depth=…&to_depth=…&other_depth=…`
///
/// Returns only the nodes and edges a pedigree gains when expanded in one
/// direction, so the client can merge a delta rather than re-render.
/// `other_depth` is the depth already loaded in the opposite direction.
pub async fn expand_pedigree(
    State(state): State<AppState>,
    Path((tree_id, root_person_id)): Path<(Uuid, Uuid)>,
    Query(params): Query<PedigreeExpandQuery>,
) -> Result<Json<PedigreeDelta>, ApiError> {
    use crate::service::pedigrees::Expansion;
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

    let delta = crate::service::pedigrees::expand_pedigree(
        &state.profiles,
        tree_id,
        root_person_id,
        Expansion {
            direction,
            from_depth: params.from_depth.into(),
            to_depth: params.to_depth.into(),
            other_depth: params.other_depth.into(),
        },
    )
    .await?;

    Ok(Json(delta))
}
