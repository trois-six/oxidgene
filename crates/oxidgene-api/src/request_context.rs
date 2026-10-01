//! The request an error event happened in, and the boundary that turns a
//! panicking handler into the standard error envelope.
//!
//! Console logs carry events only, never span context (see
//! `oxidgene-observability`), so an error event that should say which route
//! failed has to record it as a field of its own. [`wrap`] makes the matched
//! route template and method available to such events through a task-local,
//! and copies both onto the response for the HTTP duration metric. Route
//! templates are bounded (`/api/v1/trees/{tree_id}`), never the raw URI.

use std::any::Any;

use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use tower_http::catch_panic::CatchPanicLayer;
use uuid::Uuid;

use crate::rest::error::ErrorBody;

tokio::task_local! {
    static REQUEST: RequestContext;
}

/// The bounded description of the request being served.
#[derive(Debug, Clone)]
pub(crate) struct RequestContext {
    pub method: Method,
    /// The route template the request matched, if any.
    pub route: Option<MatchedPath>,
}

impl RequestContext {
    /// The context of the request the current task serves, if it serves one.
    /// Background jobs and MCP tool calls have none.
    pub(crate) fn current() -> Option<Self> {
        REQUEST.try_with(Clone::clone).ok()
    }

    pub(crate) fn route(&self) -> Option<&str> {
        self.route.as_ref().map(MatchedPath::as_str)
    }
}

/// Give every route of `router` its request context and a panic boundary.
///
/// A panic in a handler becomes a `500 internal_error` envelope carrying a
/// fresh request ID, logged with that ID, rather than a dropped connection.
/// The panic payload is never logged or returned: it is free text that may
/// hold anything the handler was working on.
pub fn wrap<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(CatchPanicLayer::custom(panic_response))
        .layer(middleware::from_fn(scope))
}

async fn scope(request: Request, next: Next) -> Response {
    let context = RequestContext {
        method: request.method().clone(),
        route: request.extensions().get::<MatchedPath>().cloned(),
    };
    let mut response = REQUEST.scope(context.clone(), next.run(request)).await;
    let extensions = response.extensions_mut();
    extensions.insert(context.method);
    if let Some(route) = context.route {
        extensions.insert(route);
    }
    response
}

fn panic_response(_payload: Box<dyn Any + Send + 'static>) -> Response {
    let request_id = Uuid::now_v7();
    let context = RequestContext::current();
    tracing::error!(
        %request_id,
        error = "internal_error",
        error.kind = "panic",
        http.request.method = context.as_ref().map(|context| context.method.as_str()),
        http.route = context.as_ref().and_then(RequestContext::route),
        "request panicked"
    );
    let body = ErrorBody {
        error: "internal_error".to_string(),
        message: "The request could not be completed".to_string(),
        request_id: Some(request_id),
    };
    (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::routing::get;
    use tower::ServiceExt as _;

    use super::*;

    async fn route_seen_by_handler() -> String {
        RequestContext::current()
            .and_then(|context| context.route().map(str::to_string))
            .unwrap_or_default()
    }

    async fn panicking_handler() -> &'static str {
        panic!("private payload")
    }

    #[tokio::test]
    async fn handlers_see_the_route_template_not_the_uri() {
        let app = wrap(Router::new().route("/trees/{tree_id}", get(route_seen_by_handler)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/trees/private-value")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router");

        assert_eq!(
            response
                .extensions()
                .get::<MatchedPath>()
                .map(MatchedPath::as_str),
            Some("/trees/{tree_id}")
        );
        assert_eq!(response.extensions().get::<Method>(), Some(&Method::GET));
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(&body[..], b"/trees/{tree_id}");
    }

    #[tokio::test]
    async fn a_panicking_handler_answers_the_standard_envelope() {
        let app = wrap(Router::new().route("/boom", get(panicking_handler)));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/boom")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON envelope");
        assert_eq!(body["error"], "internal_error");
        assert_eq!(body["message"], "The request could not be completed");
        assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
        assert!(!body.to_string().contains("private payload"));
    }
}
