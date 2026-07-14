//! REST handlers for the Dictionary page: distinct-value aggregations
//! (family names, sources, places, occupations) and usage drill-downs.

use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_db::repo::DictionaryRepo;
use uuid::Uuid;

use super::dto::{
    DictionaryEntryDto, OccupationUsageQuery, PlaceDictionaryEntry, SourceDictionaryEntry,
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

/// GET /api/v1/trees/:tree_id/dictionary/sources
pub async fn sources(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<SourceDictionaryEntry>>, ApiError> {
    let entries = DictionaryRepo::sources_with_usage(&state.db, tree_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        entries
            .into_iter()
            .map(|(source, count)| SourceDictionaryEntry { source, count })
            .collect(),
    ))
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
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = DictionaryRepo::source_usage_person_ids(&state.db, source_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ids))
}

/// GET /api/v1/trees/:tree_id/dictionary/places/:place_id/usage
pub async fn place_usage(
    State(state): State<AppState>,
    Path((_tree_id, place_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = DictionaryRepo::place_usage_person_ids(&state.db, place_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ids))
}

/// GET /api/v1/trees/:tree_id/dictionary/occupations/usage?value=...
pub async fn occupation_usage(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<OccupationUsageQuery>,
) -> Result<Json<Vec<Uuid>>, ApiError> {
    let ids = DictionaryRepo::occupation_usage_person_ids(&state.db, tree_id, &query.value)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(ids))
}
