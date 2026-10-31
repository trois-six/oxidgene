//! The HTTP application the server binds: the API under its time limits,
//! behind its CORS policy, host allowlist, same-origin write check, request
//! context and trace layer, plus the health check.

use axum::Router;
use axum::http::{HeaderValue, Method, header};
use oxidgene_api::access::{AllowedHosts, allowed_hosts, same_origin_writes};
use oxidgene_api::limits::{TimeLimits, with_time_limits};
use oxidgene_api::request_context;
use oxidgene_api::startup::with_health_check;
use oxidgene_observability::{make_http_span, on_http_response};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// How long a browser may reuse a preflight answer before asking again.
///
/// Every read too large for a URL — a pedigree batch, a page's pictures — is a
/// JSON `POST`, and so preflighted. Without a lifetime the browser keeps an
/// answer for five seconds, which puts an extra round trip in front of nearly
/// every such read. Two hours is the ceiling Chromium applies anyway; the
/// policy itself only changes with the server's configuration, which restarts
/// it.
const PREFLIGHT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(2 * 60 * 60);

/// Where the API is reached from.
#[derive(Debug, Clone)]
pub struct Exposure {
    /// The frontend origin CORS lets read, in the `none` mode. The `oidc`
    /// mode serves the frontend and the API under one origin and sends no
    /// CORS header (docs/authentication.md §9.1).
    pub cors: Option<HeaderValue>,
    /// The only origin a browser may write from.
    pub origin: HeaderValue,
    /// The host names the API answers under.
    pub hosts: AllowedHosts,
    /// Whether responses carry `Strict-Transport-Security`: the public URL
    /// is `https`.
    pub hsts: bool,
}

impl Exposure {
    /// A frontend on another origin, `origin`, which CORS lets read: the
    /// `none` mode.
    pub fn cors(origin: HeaderValue, hosts: AllowedHosts) -> Self {
        Self {
            cors: Some(origin.clone()),
            origin,
            hosts,
            hsts: false,
        }
    }
}

/// Compose the served application around the API router.
///
/// The request context sits outside the host and origin checks, so a refused
/// request still carries its route, and its panic boundary covers everything
/// below. Probes call `/healthz` every few seconds, under whatever address
/// the orchestrator dials: it is answered outside the host check and the
/// trace layer, so they are never refused and produce neither spans nor
/// metric points.
pub fn app(api_router: Router, exposure: Exposure) -> Router {
    let mut api = request_context::wrap(allowed_hosts(
        same_origin_writes(
            with_time_limits(api_router, TimeLimits::default()),
            exposure.origin,
        ),
        exposure.hosts,
    ));
    if let Some(cors_origin) = exposure.cors {
        api = api.layer(
            CorsLayer::new()
                .allow_origin(cors_origin)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers(tower_http::cors::Any)
                .max_age(PREFLIGHT_MAX_AGE),
        );
    }
    if exposure.hsts {
        api = api.layer(axum::middleware::map_response(strict_transport));
    }
    let api = api.layer(
        TraceLayer::new_for_http()
            .make_span_with(make_http_span)
            .on_response(on_http_response),
    );
    with_health_check(api)
}

/// `Strict-Transport-Security` on every response of an `https` deployment.
async fn strict_transport(mut response: axum::response::Response) -> axum::response::Response {
    response.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=63072000; includeSubDomains"),
    );
    response
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
            Exposure::cors(
                HeaderValue::from_static("https://genealogy.example.invalid"),
                AllowedHosts::new(["https://genealogy.example.invalid"]),
            ),
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
            Exposure::cors(
                HeaderValue::from_static("http://127.0.0.1:8081"),
                AllowedHosts::loopback(),
            ),
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

    /// `OXIDGENE_AUTH_MODE=none` and no other authentication setting: a
    /// personal install serving the whole API as before authentication
    /// existed — no issuer, no session store, no CSRF token — and nothing
    /// that needs an identity.
    #[tokio::test]
    async fn the_none_mode_needs_no_other_setting_and_serves_the_whole_api() {
        use oxidgene_api::auth::{Auth, AuthSettings};
        use oxidgene_api::{AppState, build_router};

        let settings: AuthSettings = config::Config::builder()
            .set_override("auth_mode", "none")
            .and_then(|builder| builder.build())
            .and_then(config::Config::try_deserialize)
            .expect("settings");
        let auth = Auth::start(settings.resolve().expect("valid"))
            .await
            .expect("no store to reach");
        assert_eq!(auth.mode_name(), "none");
        let db = oxidgene_api::startup::connect_and_migrate("sqlite::memory:").await;
        let media = std::env::temp_dir().join("oxidgene-server-none-mode-test");
        let state = AppState::new(db, media).with_auth(std::sync::Arc::new(auth));
        let app = app(
            build_router(state),
            Exposure::cors(
                HeaderValue::from_static("http://127.0.0.1:8081"),
                AllowedHosts::loopback(),
            ),
        );
        let send = |method: &str, uri: &str, body: &'static str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .header("host", "127.0.0.1:8080")
                .header("origin", "http://127.0.0.1:8081")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("valid request")
        };
        let created = app
            .clone()
            .oneshot(send("POST", "/api/v1/trees", r#"{"name":"Fictive"}"#))
            .await
            .expect("infallible router");
        assert_eq!(created.status(), StatusCode::CREATED);
        for (uri, status) in [
            ("/api/v1/trees", StatusCode::OK),
            ("/auth/session", StatusCode::OK),
            ("/api/v1/me", StatusCode::NOT_FOUND),
            ("/api/v1/admin/users", StatusCode::NOT_FOUND),
            ("/auth/login", StatusCode::NOT_FOUND),
        ] {
            let response = app
                .clone()
                .oneshot(send("GET", uri, ""))
                .await
                .expect("infallible router");
            assert_eq!(response.status(), status, "{uri}");
            if uri == "/auth/session" {
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body");
                assert!(String::from_utf8_lossy(&body).contains(r#""mode":"none""#));
            }
        }
    }

    /// The `oidc` mode serves the frontend and the API under one `https`
    /// origin: no CORS header, every response pinned to HTTPS, and writes
    /// from another origin refused.
    #[tokio::test]
    async fn one_https_origin_has_no_cors_and_pins_transport_security() {
        let api = Router::new().route(
            "/api/v1/ping",
            get(|| async { "pong" }).post(|| async { "pong" }),
        );
        let app = app(
            api,
            Exposure {
                cors: None,
                origin: HeaderValue::from_static("https://genealogy.example.invalid"),
                hosts: AllowedHosts::new(["https://genealogy.example.invalid"]),
                hsts: true,
            },
        );
        let send = |method: &str, origin: &str| {
            Request::builder()
                .method(method)
                .uri("/api/v1/ping")
                .header("host", "genealogy.example.invalid")
                .header("origin", origin)
                .header("access-control-request-method", "POST")
                .body(Body::empty())
                .expect("valid request")
        };
        let response = app
            .clone()
            .oneshot(send("GET", "https://genealogy.example.invalid"))
            .await
            .expect("infallible router");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["strict-transport-security"],
            "max-age=63072000; includeSubDomains"
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let foreign = app
            .oneshot(send("POST", "https://attacker.example.invalid"))
            .await
            .expect("infallible router");
        assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    }

    /// The preflight of a cross-origin read sent as `POST` is kept by the
    /// browser for two hours rather than asked again every few seconds.
    #[tokio::test]
    async fn preflights_are_cacheable() {
        let api = Router::new().route("/api/v1/ping", axum::routing::post(|| async { "pong" }));
        let app = app(
            api,
            Exposure::cors(
                HeaderValue::from_static("http://127.0.0.1:8081"),
                AllowedHosts::loopback(),
            ),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/v1/ping")
                    .header("origin", "http://127.0.0.1:8081")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router");
        assert_eq!(response.headers()["access-control-max-age"], "7200");
    }

    #[tokio::test]
    async fn health_probes_are_not_traced_but_api_requests_are() {
        let names = SpanNames::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(names.clone()));
        let api = Router::new().route("/api/v1/ping", get(|| async { "pong" }));
        let app = app(
            api,
            Exposure::cors(
                HeaderValue::from_static("http://127.0.0.1:8081"),
                AllowedHosts::loopback(),
            ),
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
