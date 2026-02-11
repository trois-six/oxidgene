//! Integration tests for REST API handlers.
//!
//! All tests run against an in-memory SQLite database using Axum's tower
//! `ServiceExt::oneshot` for zero-network-overhead request testing.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use sea_orm::DatabaseConnection;
use serde_json::Value;
use tower::ServiceExt;

/// Helper: create a fresh in-memory DB with migrations applied.
async fn setup_db() -> DatabaseConnection {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    db
}

/// Helper: build a router with a fresh DB.
async fn setup_app() -> axum::Router {
    let db = setup_db().await;
    let state = AppState::new(db);
    build_router(state)
}

/// Helper: send a request and return (status, body as JSON Value).
async fn send_request(
    app: axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = match body {
        Some(json) => Body::from(serde_json::to_vec(&json).unwrap()),
        None => Body::empty(),
    };

    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(body)
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();

    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };

    (status, json)
}

// ───────────────────────── Tree tests ─────────────────────────

#[tokio::test]
async fn test_tree_crud() {
    let app = setup_app().await;

    // Create a tree
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({
            "name": "Erraud Family",
            "description": "The Erraud family tree"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "Erraud Family");
    assert_eq!(body["description"], "The Erraud family tree");
    let tree_id = body["id"].as_str().unwrap().to_string();

    // Get the tree
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Erraud Family");

    // Update the tree
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(serde_json::json!({
            "name": "Erraud-Perraud Family"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Erraud-Perraud Family");

    // List trees
    let (status, body) = send_request(app.clone(), Method::GET, "/api/v1/trees", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);

    // Delete the tree
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone (soft-deleted)
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_tree_create_validation() {
    let app = setup_app().await;

    // Empty name should fail
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({
            "name": "   "
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

#[tokio::test]
async fn test_tree_not_found() {
    let app = setup_app().await;

    let fake_id = uuid::Uuid::now_v7();
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{fake_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn test_tree_pagination() {
    let app = setup_app().await;

    // Create 3 trees
    for i in 0..3 {
        send_request(
            app.clone(),
            Method::POST,
            "/api/v1/trees",
            Some(serde_json::json!({
                "name": format!("Tree {i}")
            })),
        )
        .await;
    }

    // Get first 2
    let (status, body) =
        send_request(app.clone(), Method::GET, "/api/v1/trees?first=2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 2);
    assert!(body["page_info"]["has_next_page"].as_bool().unwrap());
    let cursor = body["page_info"]["end_cursor"].as_str().unwrap();

    // Get next page
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees?first=2&after={cursor}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    assert!(!body["page_info"]["has_next_page"].as_bool().unwrap());
}

// ───────────────────────── Person tests ─────────────────────────

/// Helper: create a tree via the API and return its ID.
async fn create_tree_via_api(app: &axum::Router) -> String {
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Test Tree" })),
    )
    .await;
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_person_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create a person
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "male" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["sex"], "male");
    let person_id = body["id"].as_str().unwrap().to_string();

    // Get the person
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sex"], "male");

    // Update the person
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        Some(serde_json::json!({ "sex": "female" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sex"], "female");

    // List persons
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the person
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ───────────────────────── PersonName tests ─────────────────────────

/// Helper: create a person via the API and return its ID.
async fn create_person_via_api(app: &axum::Router, tree_id: &str) -> String {
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "male" })),
    )
    .await;
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_person_name_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a name
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Pierre",
            "surname": "Erraud",
            "is_primary": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["given_names"], "Pierre");
    assert_eq!(body["surname"], "Erraud");
    let name_id = body["id"].as_str().unwrap().to_string();

    // List names
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Update name
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
        Some(serde_json::json!({
            "surname": "Perraud"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["surname"], "Perraud");

    // Delete name
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);
}

// ───────────────────────── Family tests ─────────────────────────

#[tokio::test]
async fn test_family_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create a family
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let family_id = body["id"].as_str().unwrap().to_string();

    // Get the family
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Update the family (touches updated_at)
    let (status, _) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // List families
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the family
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ───────────────────────── Family member tests ─────────────────────────

#[tokio::test]
async fn test_family_spouse_add_remove() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a family
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();

    // Add a spouse
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
        Some(serde_json::json!({
            "person_id": person_id,
            "role": "husband",
            "sort_order": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["role"], "husband");
    let spouse_id = body["id"].as_str().unwrap().to_string();

    // Remove the spouse
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_family_child_add_remove() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a family
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();

    // Add a child
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
        Some(serde_json::json!({
            "person_id": person_id,
            "child_type": "biological",
            "sort_order": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["child_type"], "biological");
    let child_id = body["id"].as_str().unwrap().to_string();

    // Remove the child
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}/children/{child_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Ancestry tests ─────────────────────────

#[tokio::test]
async fn test_ancestors_descendants_empty() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Ancestors — should be empty
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/ancestors"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);

    // Descendants — should be empty
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/descendants"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);
}

// ───────────────────────── Error handling tests ─────────────────────────

#[tokio::test]
async fn test_invalid_uuid_path_returns_400() {
    let app = setup_app().await;

    let (status, _) =
        send_request(app.clone(), Method::GET, "/api/v1/trees/not-a-uuid", None).await;
    // Axum returns 400 for path deserialization failures
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND,
        "Expected 400 or 404, got {status}"
    );
}

#[tokio::test]
async fn test_invalid_json_body_returns_error() {
    let app = setup_app().await;

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/trees")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"invalid json"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    // Axum returns 400 for JSON syntax errors, 422 for deserialization failures
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "Expected 400 or 422, got {status}"
    );
}
