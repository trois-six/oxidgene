//! Error handling: maps `OxidGeneError` to Axum HTTP responses.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;
use serde::Serialize;
use tracing::error;
use uuid::Uuid;

use crate::error_contract::classify;

/// JSON error body returned to clients.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<Uuid>,
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
        let contract = classify(&self.0);
        let status = status(&self.0);

        let request_id = contract.unexpected.then(Uuid::now_v7);
        if let Some(request_id) = request_id {
            error!(%request_id, error = contract.code, "request failed");
        }

        let body = ErrorBody {
            error: contract.code.to_string(),
            message: contract.message.to_string(),
            request_id,
        };

        (status, axum::Json(body)).into_response()
    }
}

fn status(error: &OxidGeneError) -> StatusCode {
    match error {
        OxidGeneError::NotFound { .. } => StatusCode::NOT_FOUND,
        OxidGeneError::Validation(_) | OxidGeneError::Gedcom(_) => StatusCode::BAD_REQUEST,
        OxidGeneError::Database(_) | OxidGeneError::Io(_) | OxidGeneError::Internal(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_errors_do_not_expose_their_internal_message() {
        let contract = classify(&OxidGeneError::Validation(
            "private field value".to_string(),
        ));

        assert_eq!(contract.code, "validation_error");
        assert_eq!(contract.message, "The request is invalid");
        assert!(!contract.unexpected);
    }

    #[test]
    fn unexpected_errors_receive_a_request_id() {
        let contract = classify(&OxidGeneError::Database("private SQL".to_string()));
        let request_id = contract.unexpected.then(Uuid::now_v7);

        assert_eq!(contract.code, "database_error");
        assert_eq!(contract.message, "The request could not be completed");
        assert!(request_id.is_some());
    }
}
