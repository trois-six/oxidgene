//! REST handlers for the Dictionary page: distinct-value aggregations
//! (family names, sources, places, occupations) and usage drill-downs.

use crate::service::family_names::{self, FamilyNameChange, ParticleChange};
use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_db::repo::{DictionaryRepo, SOURCE_DRILL_THRESHOLD};
use uuid::Uuid;

use super::dto::{
    DictionaryEntryDto, DictionaryUsageQuery, FamilyNameParticleUpdateDto, FamilyNameRenameDto,
    PersonUsageEntryDto, PlaceDictionaryEntry, SourceDictionaryEntry, SourceDrillResponse,
    SourceGroupDto, SourcePrefixQuery,
};
use super::error::ApiError;
use super::state::AppState;
use crate::service::scope::{TreeResource, require_tree_resource};

/// GET /api/v1/trees/:tree_id/dictionary/family-names
pub async fn family_names(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<DictionaryEntryDto>>, ApiError> {
    let entries = DictionaryRepo::family_names(&state.reader, tree_id).await?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/trees/:tree_id/dictionary/occupations
pub async fn occupations(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<DictionaryEntryDto>>, ApiError> {
    let entries = DictionaryRepo::occupations(&state.reader, tree_id).await?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/trees/:tree_id/dictionary/sources?prefix=...
///
/// `prefix` narrows the result to sources whose title starts with it
/// (case-insensitive); absent/empty returns every source. Used both for the
/// legacy full fetch and as the final flat-list step of the Sources tab's
/// smart drill-down (see ui-dictionary.md §8) once a prefix's count drops
/// to <= 250.
pub async fn sources(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<SourcePrefixQuery>,
) -> Result<Json<Vec<SourceDictionaryEntry>>, ApiError> {
    let prefix = query.prefix.unwrap_or_default();
    Ok(Json(
        crate::service::source::dictionary_sources(&state.reader, tree_id, &prefix).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/dictionary/sources/groups?prefix=...
///
/// Resolves the Sources tab's smart drill-down from `prefix` (absent/empty
/// = start from the top): auto-skips forced single-choice levels (e.g. a
/// single town's records nested under a department that otherwise branches
/// many ways) and returns either the real next branch choices, or an empty
/// `groups` list once `total` has dropped to <= the drill threshold — the
/// frontend should then fetch the final flat list via the plain `sources`
/// endpoint using the returned (possibly extended) `prefix`. See
/// ui-dictionary.md §8.10.
pub async fn source_groups(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<SourcePrefixQuery>,
) -> Result<Json<SourceDrillResponse>, ApiError> {
    let prefix = query.prefix.unwrap_or_default();
    let (resolved_prefix, total, groups) = DictionaryRepo::resolve_source_drill_down(
        &state.reader,
        tree_id,
        &prefix,
        SOURCE_DRILL_THRESHOLD,
    )
    .await?;
    let sources = if groups.is_empty() {
        Some(
            crate::service::source::dictionary_sources(&state.reader, tree_id, &resolved_prefix)
                .await?,
        )
    } else {
        None
    };
    Ok(Json(SourceDrillResponse {
        prefix: resolved_prefix,
        total,
        groups: groups
            .into_iter()
            .map(|(label, count)| SourceGroupDto { label, count })
            .collect(),
        sources,
    }))
}

/// GET /api/v1/trees/:tree_id/dictionary/places
pub async fn places(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<PlaceDictionaryEntry>>, ApiError> {
    let entries = DictionaryRepo::places_with_usage(&state.reader, tree_id).await?;
    Ok(Json(
        entries
            .into_iter()
            .map(|(place, count)| PlaceDictionaryEntry { place, count })
            .collect(),
    ))
}

/// GET /api/v1/trees/:tree_id/dictionary/sources/:source_id/usage
pub async fn source_usage(
    State(state): State<AppState>,
    Path((tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Source, source_id).await?;
    let ids = DictionaryRepo::source_usage_person_ids(&state.reader, source_id).await?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/places/:place_id/usage
pub async fn place_usage(
    State(state): State<AppState>,
    Path((tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    require_tree_resource(&state.reader, tree_id, TreeResource::Place, place_id).await?;
    let ids = DictionaryRepo::place_usage_person_ids(&state.reader, place_id).await?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/occupations/usage?value=...
pub async fn occupation_usage(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<DictionaryUsageQuery>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids =
        DictionaryRepo::occupation_usage_person_ids(&state.reader, tree_id, &query.value).await?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/family-names/usage?value=...
pub async fn family_name_usage(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<DictionaryUsageQuery>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids =
        DictionaryRepo::family_name_usage_person_ids(&state.reader, tree_id, &query.value).await?;
    resolve_usage(&state, &ids).await
}

/// PATCH /api/v1/trees/:tree_id/dictionary/family-names/particle
///
/// Re-cuts every occurrence of one surname at a given particle — the bulk
/// repair for an import that guessed wrong across a whole family.
pub async fn set_family_name_particle(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<ParticleChange>,
) -> Result<Json<FamilyNameParticleUpdateDto>, ApiError> {
    let update = family_names::set_particle(&state.db, &state.profiles, tree_id, body).await?;
    Ok(Json(update.into()))
}

/// PATCH /api/v1/trees/:tree_id/dictionary/family-names/rename
///
/// Gives every person whose primary name carries one surname another one,
/// merging into that name when it is already listed.
pub async fn rename_family_name(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Json(body): Json<FamilyNameChange>,
) -> Result<Json<FamilyNameRenameDto>, ApiError> {
    let renamed = family_names::rename(&state.db, &state.profiles, tree_id, body).await?;
    Ok(Json(renamed.into()))
}

/// Shared tail of the four usage handlers: resolve raw person IDs into
/// name + birth/death year entries in one bulk query.
async fn resolve_usage(
    state: &AppState,
    person_ids: &[Uuid],
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let entries = DictionaryRepo::resolve_person_usage_entries(&state.reader, person_ids).await?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}
