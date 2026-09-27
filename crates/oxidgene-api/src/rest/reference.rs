//! REST handlers for read-only reference content (occupation sheets,
//! given-name meanings, the place dictionary). Not tied to a tree — a lookup
//! by raw value, independent of `AppState`.

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use serde::Deserialize;

use super::dto::{PlaceSuggestionQuery, ReferenceTermQuery};
use crate::reference::{self, ReferenceLang};

#[derive(Debug, Deserialize)]
pub struct ReferenceTermsRequest {
    terms: Vec<String>,
}

/// GET /api/v1/reference/:lang/occupations?term=...
pub async fn occupation(
    Path(lang): Path<String>,
    Query(query): Query<ReferenceTermQuery>,
) -> Result<Json<reference::OccupationEntry>, StatusCode> {
    let lang = ReferenceLang::from_code(&lang).ok_or(StatusCode::BAD_REQUEST)?;
    reference::lookup_occupation(lang, &query.term)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// GET /api/v1/reference/:lang/given-names?term=...
pub async fn given_name(
    Path(lang): Path<String>,
    Query(query): Query<ReferenceTermQuery>,
) -> Result<Json<reference::GivenNameEntry>, StatusCode> {
    let lang = ReferenceLang::from_code(&lang).ok_or(StatusCode::BAD_REQUEST)?;
    reference::lookup_given_name(lang, &query.term)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// POST /api/v1/reference/:lang/given-names/bundle
pub async fn given_names(
    Path(lang): Path<String>,
    Json(request): Json<ReferenceTermsRequest>,
) -> Result<Json<Vec<reference::GivenNameMatch>>, StatusCode> {
    let lang = ReferenceLang::from_code(&lang).ok_or(StatusCode::BAD_REQUEST)?;
    if request.terms.len() > reference::MAX_REFERENCE_TERMS {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(reference::lookup_given_names(lang, &request.terms)))
}

/// POST /api/v1/reference/:lang/occupations/bundle
pub async fn occupations(
    Path(lang): Path<String>,
    Json(request): Json<ReferenceTermsRequest>,
) -> Result<Json<Vec<reference::OccupationMatch>>, StatusCode> {
    let lang = ReferenceLang::from_code(&lang).ok_or(StatusCode::BAD_REQUEST)?;
    if request.terms.len() > reference::MAX_REFERENCE_TERMS {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(reference::lookup_occupations(lang, &request.terms)))
}

/// GET /api/v1/reference/:lang/places?q=...&limit=...
pub async fn places(
    Path(lang): Path<String>,
    Query(query): Query<PlaceSuggestionQuery>,
) -> Result<Json<Vec<reference::PlaceSuggestion>>, StatusCode> {
    let lang = ReferenceLang::from_code(&lang).ok_or(StatusCode::BAD_REQUEST)?;
    let limit = query.limit.unwrap_or(reference::DEFAULT_PLACE_SUGGESTIONS);
    if !(1..=reference::MAX_PLACE_SUGGESTIONS).contains(&limit) {
        return Err(StatusCode::BAD_REQUEST);
    }
    // The first search decompresses and indexes the dictionary, and every
    // search scans it: kept off the async workers.
    tokio::task::spawn_blocking(move || reference::search_places(lang, &query.q, limit))
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
