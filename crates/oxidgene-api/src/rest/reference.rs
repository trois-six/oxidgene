//! REST handlers for read-only reference content (occupation sheets,
//! given-name meanings, the place dictionary). Not tied to a tree — a lookup
//! by raw value, independent of `AppState`.
//!
//! Failures use the error envelope like every other endpoint: an unknown
//! language or an oversized batch is a `validation_error`, a term without a
//! sheet a `not_found`.

use axum::Json;
use axum::extract::{Path, Query};
use oxidgene_core::OxidGeneError;
use serde::Deserialize;
use uuid::Uuid;

use super::dto::{PlaceSuggestionQuery, ReferenceTermQuery};
use super::error::ApiError;
use crate::reference;

#[derive(Debug, Deserialize)]
pub struct ReferenceTermsRequest {
    terms: Vec<String>,
}

/// A term that has no sheet. Reference terms are not records, so there is no
/// identifier to report.
fn no_sheet(entity: &'static str) -> ApiError {
    ApiError(OxidGeneError::NotFound {
        entity,
        id: Uuid::nil(),
    })
}

/// GET /api/v1/reference/:lang/occupations?term=...
pub async fn occupation(
    Path(lang): Path<String>,
    Query(query): Query<ReferenceTermQuery>,
) -> Result<Json<reference::OccupationEntry>, ApiError> {
    let lang = reference::language(&lang)?;
    reference::lookup_occupation(lang, &query.term)
        .map(Json)
        .ok_or_else(|| no_sheet("Occupation sheet"))
}

/// GET /api/v1/reference/:lang/given-names?term=...
pub async fn given_name(
    Path(lang): Path<String>,
    Query(query): Query<ReferenceTermQuery>,
) -> Result<Json<reference::GivenNameEntry>, ApiError> {
    let lang = reference::language(&lang)?;
    reference::lookup_given_name(lang, &query.term)
        .map(Json)
        .ok_or_else(|| no_sheet("Given name sheet"))
}

/// POST /api/v1/reference/:lang/given-names/bundle
pub async fn given_names(
    Path(lang): Path<String>,
    Json(request): Json<ReferenceTermsRequest>,
) -> Result<Json<Vec<reference::GivenNameMatch>>, ApiError> {
    let lang = reference::language(&lang)?;
    reference::check_terms(&request.terms)?;
    Ok(Json(reference::lookup_given_names(lang, &request.terms)))
}

/// POST /api/v1/reference/:lang/occupations/bundle
pub async fn occupations(
    Path(lang): Path<String>,
    Json(request): Json<ReferenceTermsRequest>,
) -> Result<Json<Vec<reference::OccupationMatch>>, ApiError> {
    let lang = reference::language(&lang)?;
    reference::check_terms(&request.terms)?;
    Ok(Json(reference::lookup_occupations(lang, &request.terms)))
}

/// GET /api/v1/reference/:lang/places?q=...&limit=...
pub async fn places(
    Path(lang): Path<String>,
    Query(query): Query<PlaceSuggestionQuery>,
) -> Result<Json<Vec<reference::PlaceSuggestion>>, ApiError> {
    let lang = reference::language(&lang)?;
    let limit = reference::place_limit(query.limit)?;
    // The first search decompresses and indexes the dictionary, and every
    // search scans it: kept off the async workers.
    reference::search_places_off_thread(lang, query.q, limit)
        .await
        .map(Json)
        .map_err(|_| ApiError(OxidGeneError::Internal("place search failed".into())))
}

/// GET /api/v1/reference/basemap
///
/// The country outlines the statistics heat map is drawn over.
pub async fn basemap() -> Json<&'static [reference::BasemapCountry]> {
    Json(reference::basemap())
}
