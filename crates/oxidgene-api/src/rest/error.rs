//! Error handling: maps `OxidGeneError` to Axum HTTP responses.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use serde::Serialize;

/// JSON error body returned to clients.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

/// Wrapper around `OxidGeneError` that implements `IntoResponse`.
pub struct ApiError(pub OxidGeneError);

impl From<OxidGeneError> for ApiError {
    fn from(err: OxidGeneError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_type) = match &self.0 {
            OxidGeneError::NotFound { .. } => (StatusCode::NOT_FOUND, "not_found"),
            OxidGeneError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
            OxidGeneError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
            OxidGeneError::Gedcom(_) => (StatusCode::BAD_REQUEST, "gedcom_error"),
            OxidGeneError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "io_error"),
            OxidGeneError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };

        let body = ErrorBody {
            error: error_type.to_string(),
            message: self.0.to_string(),
        };

        (status, axum::Json(body)).into_response()
    }
}
