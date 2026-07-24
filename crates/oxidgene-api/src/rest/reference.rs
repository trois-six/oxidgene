//! REST handlers for read-only reference content (occupation sheets,
//! given-name meanings). Not tied to a tree — a lookup by GEDCOM raw value,
//! independent of `AppState`.

use axum::Json;
use axum::extract::{Path, Query};
use axum::http::StatusCode;

use super::dto::ReferenceTermQuery;
use crate::reference::{self, ReferenceLang};

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
