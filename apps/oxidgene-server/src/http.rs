//! The HTTP application the server binds: the API under its time limits,
//! behind its CORS policy, host allowlist, same-origin write check, request
//! context and trace layer, plus the health check.

use axum::Router;
use axum::http::{HeaderValue, Method};
use oxidgene_api::access::{AllowedHosts, allowed_hosts, same_origin_writes};
use oxidgene_api::limits::{TimeLimits, with_time_limits};
use oxidgene_api::request_context;
use oxidgene_api::startup::with_health_check;
use oxidgene_observability::{make_http_span, on_http_response};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Compose the served application around the API router.
///
/// The request context sits outside the host and origin checks, so a refused
/// request still carries its route, and its panic boundary covers everything
/// below. Probes call `/healthz` every few seconds, under whatever address
/// the orchestrator dials: it is answered outside the host check and the
/// trace layer, so they are never refused and produce neither spans nor
/// metric points.
pub fn app(api_router: Router, cors_origin: HeaderValue, hosts: AllowedHosts) -> Router {
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
    let api = request_context::wrap(allowed_hosts(
        same_origin_writes(
            with_time_limits(api_router, TimeLimits::default()),
            cors_origin,
        ),
        hosts,
    ))
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
        get_as(app, uri, "127.0.0.1:8080").await.0
    }

    /// GET `uri` naming `host`; the status and the body.
    async fn get_as(app: &Router, uri: &str, host: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("host", host)
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
    async fn only_known_hosts_reach_the_api() {
        let api = Router::new().route("/api/v1/ping", get(|| async { "pong" }));
        let app = app(
            api,
            HeaderValue::from_static("https://genealogy.example.invalid"),
            AllowedHosts::new(["https://genealogy.example.invalid"]),
        );

        assert_eq!(
            get_as(&app, "/api/v1/ping", "genealogy.example.invalid")
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(
            get_as(&app, "/api/v1/ping", "127.0.0.1:8080").await.0,
            StatusCode::OK
        );
        // A page that rebound its own name to the server's address.
        let (status, body) = get_as(&app, "/api/v1/ping", "rebound.example:8080").await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(body.contains(r#""error":"forbidden""#), "{body}");
        // Probes dial the pod's address, which no allowlist names.
        assert_eq!(
            get_as(&app, "/healthz", "10.0.0.7:8080").await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn the_frontend_origin_may_send_every_method_the_ui_uses() {
        let api = Router::new().route("/api/v1/ping", get(|| async { "pong" }));
        let app = app(
            api,
            HeaderValue::from_static("http://127.0.0.1:8081"),
            AllowedHosts::loopback(),
        );
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
        let app = app(
            api,
            HeaderValue::from_static("http://127.0.0.1:8081"),
            AllowedHosts::loopback(),
        );

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
