//! REST handlers for the Dictionary page: distinct-value aggregations
//! (family names, sources, places, occupations) and usage drill-downs.

use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_db::repo::{DictionaryRepo, SOURCE_DRILL_THRESHOLD};
use uuid::Uuid;

use super::dto::{
    DictionaryEntryDto, DictionaryUsageQuery, PersonUsageEntryDto, PlaceDictionaryEntry,
    SourceDictionaryEntry, SourceDrillResponse, SourceGroupDto, SourcePrefixQuery,
};
use super::error::ApiError;
use super::state::AppState;

/// GET /api/v1/trees/:tree_id/dictionary/family-names
pub async fn family_names(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<DictionaryEntryDto>>, ApiError> {
    let entries = DictionaryRepo::family_names(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}

/// GET /api/v1/trees/:tree_id/dictionary/occupations
pub async fn occupations(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<DictionaryEntryDto>>, ApiError> {
    let entries = DictionaryRepo::occupations(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
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
    let entries = DictionaryRepo::sources_with_usage_by_prefix(&state.db, tree_id, &prefix)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        entries
            .into_iter()
            .map(|(source, count)| SourceDictionaryEntry { source, count })
            .collect(),
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
        &state.db,
        tree_id,
        &prefix,
        SOURCE_DRILL_THRESHOLD,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(SourceDrillResponse {
        prefix: resolved_prefix,
        total,
        groups: groups
            .into_iter()
            .map(|(label, count)| SourceGroupDto { label, count })
            .collect(),
    }))
}

/// GET /api/v1/trees/:tree_id/dictionary/places
pub async fn places(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<PlaceDictionaryEntry>>, ApiError> {
    let entries = DictionaryRepo::places_with_usage(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
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
    Path((_tree_id, source_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids = DictionaryRepo::source_usage_person_ids(&state.db, source_id)
        .await
        .map_err(ApiError::from)?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/places/:place_id/usage
pub async fn place_usage(
    State(state): State<AppState>,
    Path((_tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids = DictionaryRepo::place_usage_person_ids(&state.db, place_id)
        .await
        .map_err(ApiError::from)?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/occupations/usage?value=...
pub async fn occupation_usage(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<DictionaryUsageQuery>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids = DictionaryRepo::occupation_usage_person_ids(&state.db, tree_id, &query.value)
        .await
        .map_err(ApiError::from)?;
    resolve_usage(&state, &ids).await
}

/// GET /api/v1/trees/:tree_id/dictionary/family-names/usage?value=...
pub async fn family_name_usage(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<DictionaryUsageQuery>,
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let ids = DictionaryRepo::family_name_usage_person_ids(&state.db, tree_id, &query.value)
        .await
        .map_err(ApiError::from)?;
    resolve_usage(&state, &ids).await
}

/// Shared tail of the four usage handlers: resolve raw person IDs into
/// name + birth/death year entries in one bulk query.
async fn resolve_usage(
    state: &AppState,
    person_ids: &[Uuid],
) -> Result<Json<Vec<PersonUsageEntryDto>>, ApiError> {
    let entries = DictionaryRepo::resolve_person_usage_entries(&state.db, person_ids)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(entries.into_iter().map(Into::into).collect()))
}
