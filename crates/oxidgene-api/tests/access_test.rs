//! Who may reach the backend: the desktop's per-launch token and the
//! standalone server's same-origin writes.

use axum::body::Body;
use axum::http::{HeaderValue, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use oxidgene_api::access::{LocalToken, require_local_token, same_origin_writes};
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use tower::ServiceExt;

async fn router() -> axum::Router {
    let db = connect("sqlite::memory:").await.unwrap();
    run_migrations(&db).await.unwrap();
    build_router(AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-media"),
    ))
}

async fn send(
    app: &axum::Router,
    method: Method,
    uri: &str,
    headers: &[(header::HeaderName, &str)],
) -> (StatusCode, serde_json::Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    for (name, value) in headers {
        request = request.header(name, *value);
    }
    let body = if uri == "/api/v1/trees" {
        Body::from(r#"{"name":"Fixture"}"#)
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

#[tokio::test]
async fn the_embedded_backend_answers_only_its_own_client() {
    let token = LocalToken::generate();
    let app = require_local_token(router().await, token.clone());
    let bearer = format!("Bearer {}", token.as_str());

    let (status, body) = send(&app, Method::GET, "/api/v1/trees", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "unauthenticated");

    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/trees",
        &[(header::AUTHORIZATION, "Bearer not-the-token")],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A form post a web page could send without a preflight is refused too.
    let (status, _) = send(&app, Method::POST, "/graphql", &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/trees",
        &[(header::AUTHORIZATION, &bearer)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The API description holds no tree data and is opened in a browser.
    let (status, _) = send(&app, Method::GET, "/api/v1/openapi.json", &[]).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn only_the_frontend_origin_may_write() {
    let frontend = "https://genealogy.example";
    let app = same_origin_writes(router().await, HeaderValue::from_static(frontend));

    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        &[(header::ORIGIN, "https://elsewhere.example")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "forbidden");

    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        &[(header::ORIGIN, frontend)],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // No browser, no page acting on anyone's behalf.
    let (status, _) = send(&app, Method::POST, "/api/v1/trees", &[]).await;
    assert_eq!(status, StatusCode::CREATED);

    // Reads are CORS's business: it withholds the response from a foreign page.
    let (status, _) = send(
        &app,
        Method::GET,
        "/api/v1/trees",
        &[(header::ORIGIN, "https://elsewhere.example")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
