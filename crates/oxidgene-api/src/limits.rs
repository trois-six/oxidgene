//! How long the standalone server lets a request run.
//!
//! Three limits, so that a slow or stalled client cannot hold a connection,
//! an intake slot (see [`crate::service::intake`]) or a database transaction
//! forever:
//!
//! - a request must produce its response within [`TimeLimits::request`];
//! - an upload — an import job's source, a media file — gets
//!   [`TimeLimits::upload`] instead, since a large file over a slow link is
//!   legitimately slow;
//! - a request body that sends nothing for [`TimeLimits::body_idle`] is cut
//!   off, however long its overall allowance.
//!
//! A request over its limit answers `503 timeout` in the standard envelope.
//! A response already started — a download streaming — is not cut. The
//! desktop's embedded server applies none of this: it answers its own window
//! alone, and a long local operation is not an attack.

use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::timeout::RequestBodyTimeoutLayer;

use crate::access::refusal;

/// The limits [`with_time_limits`] applies.
#[derive(Clone, Copy, Debug)]
pub struct TimeLimits {
    /// Until the response of an ordinary request.
    pub request: Duration,
    /// Until the response of an upload.
    pub upload: Duration,
    /// Between two pieces of a request body.
    pub body_idle: Duration,
}

impl Default for TimeLimits {
    /// Five minutes for a request — the longest exports and rebuilds of a
    /// large tree take well under one — an hour for an upload, and a minute
    /// of silence in a body.
    fn default() -> Self {
        Self {
            request: Duration::from_secs(5 * 60),
            upload: Duration::from_secs(60 * 60),
            body_idle: Duration::from_secs(60),
        }
    }
}

/// `router` under `limits`.
pub fn with_time_limits(router: Router, limits: TimeLimits) -> Router {
    router
        .layer(middleware::from_fn_with_state(limits, limit_time))
        .layer(RequestBodyTimeoutLayer::new(limits.body_idle))
}

/// Whether `request` uploads a file: a POST to an import job's or a media
/// file's upload route.
fn is_upload(request: &Request) -> bool {
    let path = request.uri().path();
    request.method() == Method::POST
        && (path.ends_with("/import-jobs") || path.ends_with("/media/upload"))
}

async fn limit_time(State(limits): State<TimeLimits>, request: Request, next: Next) -> Response {
    let limit = if is_upload(&request) {
        limits.upload
    } else {
        limits.request
    };
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "timeout",
            "The request took longer than the server allows",
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    fn limits() -> TimeLimits {
        TimeLimits {
            request: Duration::from_millis(100),
            upload: Duration::from_secs(5),
            body_idle: Duration::from_secs(5),
        }
    }

    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_millis(400)).await;
        "done"
    }

    async fn status(app: &Router, method: Method, uri: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn a_request_past_its_limit_answers_the_timeout_envelope() {
        let app = with_time_limits(Router::new().route("/api/v1/slow", get(slow)), limits());
        let (status, body) = status(&app, Method::GET, "/api/v1/slow").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains(r#""error":"timeout""#), "{body}");
    }

    #[tokio::test]
    async fn an_upload_gets_the_longer_limit() {
        let app = with_time_limits(
            Router::new()
                .route("/api/v1/trees/{tree_id}/import-jobs", post(slow))
                .route("/api/v1/trees/{tree_id}/media/upload", post(slow)),
            limits(),
        );
        for uri in [
            "/api/v1/trees/t/import-jobs",
            "/api/v1/trees/t/media/upload",
        ] {
            assert_eq!(
                status(&app, Method::POST, uri).await.0,
                StatusCode::OK,
                "{uri}"
            );
        }
    }
}
