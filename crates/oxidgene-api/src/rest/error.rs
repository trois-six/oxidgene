//! Error handling: maps `OxidGeneError` to Axum HTTP responses.

use axum::http::{StatusCode, header};
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

impl ErrorBody {
    /// The public envelope for a domain error: its stable code and safe
    /// message, plus a logged request ID when the failure was unexpected.
    ///
    /// Shared by every surface that reports errors as JSON, so they cannot
    /// disagree on what a failure discloses.
    pub(crate) fn from_error(error: &OxidGeneError) -> Self {
        let contract = classify(error);
        let request_id = contract.unexpected.then(Uuid::now_v7);
        if let Some(request_id) = request_id {
            error!(%request_id, error = contract.code, "request failed");
        }
        Self {
            error: contract.code.to_string(),
            message: contract.message.to_string(),
            request_id,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody::from_error(&self.0);
        (status(&self.0), axum::Json(body)).into_response()
    }
}

/// Give the documented error envelope to a client error no handler wrote.
///
/// Axum's extractors reject a request before the handler runs — an invalid
/// UUID in the path, a malformed or mistyped JSON body, a missing content
/// type, a body over the route's limit. Those answers are plain text, which a client parsing
/// [`ErrorBody`] cannot read. Applied once to the whole REST router, this
/// rewrites them into the envelope with the code their status stands for;
/// anything already JSON is left alone. A data error (`422`) is a validation
/// error like any other and is reported as `400`, the one status the
/// contract gives it.
pub async fn envelope_rejections(response: Response) -> Response {
    let status = response.status();
    let is_json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if is_json || !status.is_client_error() {
        return response;
    }
    let (status, code, message) = match status {
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => (
            StatusCode::BAD_REQUEST,
            "validation_error",
            "The request is invalid",
        ),
        StatusCode::NOT_FOUND => (status, "not_found", "The requested resource was not found"),
        StatusCode::PAYLOAD_TOO_LARGE => (
            status,
            "payload_too_large",
            "The request exceeds the size limit",
        ),
        StatusCode::UNSUPPORTED_MEDIA_TYPE => (
            status,
            "unsupported_media_type",
            "The request format is unsupported",
        ),
        _ => return response,
    };
    let body = ErrorBody {
        error: code.to_string(),
        message: message.to_string(),
        request_id: None,
    };
    (status, axum::Json(body)).into_response()
}

/// The answer to a path no route matches: `404 not_found`, in the envelope.
pub async fn unknown_route() -> Response {
    let body = ErrorBody {
        error: "not_found".to_string(),
        message: "The requested resource was not found".to_string(),
        request_id: None,
    };
    (StatusCode::NOT_FOUND, axum::Json(body)).into_response()
}

fn status(error: &OxidGeneError) -> StatusCode {
    match error {
        OxidGeneError::NotFound { .. } => StatusCode::NOT_FOUND,
        OxidGeneError::Validation(_) | OxidGeneError::Gedcom(_) => StatusCode::BAD_REQUEST,
        OxidGeneError::Conflict(_) => StatusCode::CONFLICT,
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
