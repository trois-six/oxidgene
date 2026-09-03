//! REST handlers for read-only reference content (occupation sheets,
//! given-name meanings). Not tied to a tree — a lookup by GEDCOM raw value,
//! independent of `AppState`.

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use serde::Deserialize;

use super::dto::ReferenceTermQuery;
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
