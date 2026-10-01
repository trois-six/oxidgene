//! The HTTP application the server binds: the API behind its CORS policy,
//! same-origin write check, request context and trace layer, plus the
//! health check.

use axum::Router;
use axum::http::{HeaderValue, Method};
use oxidgene_api::access::same_origin_writes;
use oxidgene_api::request_context;
use oxidgene_api::startup::with_health_check;
use oxidgene_observability::{make_http_span, on_http_response};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Compose the served application around the API router.
///
/// The request context sits outside the origin check, so a refused write
/// still carries its route, and its panic boundary covers everything below.
/// Probes call `/healthz` every few seconds: it is answered outside the
/// trace layer, so they produce neither spans nor metric points.
pub fn app(api_router: Router, cors_origin: HeaderValue) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(cors_origin.clone())
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(tower_http::cors::Any);
    let api = request_context::wrap(same_origin_writes(api_router, cors_origin))
        .layer(cors)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(make_http_span)
                .on_response(on_http_response),
        );
    with_health_check(api)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use tower::ServiceExt as _;
    use tracing::Subscriber;
    use tracing::span::{Attributes, Id};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};

    use super::*;

    #[derive(Clone, Default)]
    struct SpanNames(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for SpanNames {
        fn on_new_span(&self, attributes: &Attributes<'_>, _id: &Id, _context: Context<'_, S>) {
            self.0
                .lock()
                .expect("capture lock")
                .push(attributes.metadata().name().to_string());
        }
    }

    async fn get_status(app: &Router, uri: &str) -> StatusCode {
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router")
            .status()
    }

    #[tokio::test]
    async fn the_frontend_origin_may_send_every_method_the_ui_uses() {
        let api = Router::new().route("/api/v1/ping", get(|| async { "pong" }));
        let app = app(api, HeaderValue::from_static("http://127.0.0.1:8081"));
        for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("OPTIONS")
                        .uri("/api/v1/ping")
                        .header("origin", "http://127.0.0.1:8081")
                        .header("access-control-request-method", method)
                        .body(Body::empty())
                        .expect("valid request"),
                )
                .await
                .expect("infallible router");
            let allowed = response
                .headers()
                .get("access-control-allow-methods")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(allowed.contains(method), "{method} not in {allowed:?}");
        }
    }

    #[tokio::test]
    async fn health_probes_are_not_traced_but_api_requests_are() {
        let names = SpanNames::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(names.clone()));
        let api = Router::new().route("/api/v1/ping", get(|| async { "pong" }));
        let app = app(api, HeaderValue::from_static("http://127.0.0.1:8081"));

        assert_eq!(get_status(&app, "/healthz").await, StatusCode::OK);
        assert!(
            names.0.lock().expect("capture lock").is_empty(),
            "a probe opened a span"
        );

        assert_eq!(get_status(&app, "/api/v1/ping").await, StatusCode::OK);
        assert_eq!(
            *names.0.lock().expect("capture lock"),
            vec!["http.server.request".to_string()]
        );
    }
}
