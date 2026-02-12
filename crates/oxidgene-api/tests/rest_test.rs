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

// ───────────────────────── Event tests ─────────────────────────

#[tokio::test]
async fn test_event_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create an event
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({
            "event_type": "birth",
            "date_value": "1 JAN 1990",
            "date_sort": "1990-01-01",
            "person_id": person_id,
            "description": "Born in Paris"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["event_type"], "birth");
    assert_eq!(body["description"], "Born in Paris");
    let event_id = body["id"].as_str().unwrap().to_string();

    // Get the event
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["event_type"], "birth");

    // Update the event
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        Some(serde_json::json!({
            "description": "Born in Lyon"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["description"], "Born in Lyon");

    // List events (no filter)
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List events (filter by person_id)
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List events (filter by event_type)
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?event_type=birth"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the event
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ───────────────────────── Place tests ─────────────────────────

#[tokio::test]
async fn test_place_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create a place
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/places"),
        Some(serde_json::json!({
            "name": "Paris, France",
            "latitude": 48.8566,
            "longitude": 2.3522
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "Paris, France");
    let place_id = body["id"].as_str().unwrap().to_string();

    // Get the place
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places/{place_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Paris, France");

    // Update the place
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/places/{place_id}"),
        Some(serde_json::json!({
            "name": "Lyon, France"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Lyon, France");

    // List places
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List places with search
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places?search=Lyon"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Search for non-existent place
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places?search=Berlin"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 0);

    // Delete the place
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/places/{place_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_place_create_validation() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Empty name should fail
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/places"),
        Some(serde_json::json!({
            "name": "   "
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

// ───────────────────────── Source tests ─────────────────────────

#[tokio::test]
async fn test_source_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create a source
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(serde_json::json!({
            "title": "Parish Records of Lyon",
            "author": "Catholic Church",
            "publisher": "Diocese of Lyon"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["title"], "Parish Records of Lyon");
    assert_eq!(body["author"], "Catholic Church");
    let source_id = body["id"].as_str().unwrap().to_string();

    // Get the source
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Parish Records of Lyon");

    // Update the source
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        Some(serde_json::json!({
            "title": "Parish Records of Paris",
            "author": "Archdiocese of Paris"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Parish Records of Paris");
    assert_eq!(body["author"], "Archdiocese of Paris");

    // List sources
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/sources"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the source
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_source_create_validation() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Empty title should fail
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(serde_json::json!({
            "title": ""
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

// ───────────────────────── Citation tests ─────────────────────────

/// Helper: create a source via the API and return its ID.
async fn create_source_via_api(app: &axum::Router, tree_id: &str) -> String {
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(serde_json::json!({
            "title": "Test Source"
        })),
    )
    .await;
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_citation_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let source_id = create_source_via_api(&app, &tree_id).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a citation
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/citations"),
        Some(serde_json::json!({
            "source_id": source_id,
            "person_id": person_id,
            "page": "p. 42",
            "confidence": "high",
            "text": "Birth record found"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["page"], "p. 42");
    assert_eq!(body["confidence"], "high");
    let citation_id = body["id"].as_str().unwrap().to_string();

    // Update the citation
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/citations/{citation_id}"),
        Some(serde_json::json!({
            "page": "p. 43",
            "text": "Updated record"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["page"], "p. 43");
    assert_eq!(body["text"], "Updated record");

    // Delete the citation
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/citations/{citation_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Media tests ─────────────────────────

#[tokio::test]
async fn test_media_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create media
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "file_name": "photo.jpg",
            "mime_type": "image/jpeg",
            "file_path": "/uploads/photo.jpg",
            "file_size": 1024000,
            "title": "Family portrait",
            "description": "Summer 1990"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["file_name"], "photo.jpg");
    assert_eq!(body["title"], "Family portrait");
    let media_id = body["id"].as_str().unwrap().to_string();

    // Get media
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/media/{media_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["file_name"], "photo.jpg");

    // Update media
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/media/{media_id}"),
        Some(serde_json::json!({
            "title": "Updated portrait",
            "description": "Winter 1990"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Updated portrait");
    assert_eq!(body["description"], "Winter 1990");

    // List media
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/media"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete media
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media/{media_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/media/{media_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_media_create_validation() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Empty file_name should fail
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "file_name": "  ",
            "mime_type": "image/jpeg",
            "file_path": "/uploads/photo.jpg",
            "file_size": 1024
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

// ───────────────────────── MediaLink tests ─────────────────────────

#[tokio::test]
async fn test_media_link_create_delete() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create media first
    let (_, media_body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "file_name": "doc.pdf",
            "mime_type": "application/pdf",
            "file_path": "/uploads/doc.pdf",
            "file_size": 2048
        })),
    )
    .await;
    let media_id = media_body["id"].as_str().unwrap().to_string();

    // Create a media link
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media-links"),
        Some(serde_json::json!({
            "media_id": media_id,
            "person_id": person_id,
            "sort_order": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["media_id"], media_id);
    assert_eq!(body["person_id"], person_id);
    let link_id = body["id"].as_str().unwrap().to_string();

    // Delete the media link
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media-links/{link_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Note tests ─────────────────────────

#[tokio::test]
async fn test_note_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a note
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Important note about this person",
            "person_id": person_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["text"], "Important note about this person");
    let note_id = body["id"].as_str().unwrap().to_string();

    // Get the note
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["text"], "Important note about this person");

    // Update the note
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        Some(serde_json::json!({
            "text": "Updated note text"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["text"], "Updated note text");

    // List notes by person
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Delete the note
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_note_create_validation() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Empty text should fail
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "   "
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

#[tokio::test]
async fn test_note_list_by_multiple_entities() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a note linked to a person
    send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Person note",
            "person_id": person_id
        })),
    )
    .await;

    // Create a family and a note linked to it
    let (_, fam_body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = fam_body["id"].as_str().unwrap().to_string();

    send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Family note",
            "family_id": family_id
        })),
    )
    .await;

    // List by person — should get 1
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["text"], "Person note");

    // List by family — should get 1
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?family_id={family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    assert_eq!(body[0]["text"], "Family note");
}

// ── GEDCOM Import/Export ─────────────────────────────────────────────

fn minimal_gedcom() -> &'static str {
    concat!(
        "0 HEAD\n",
        "1 SOUR OxidGene\n",
        "1 GEDC\n",
        "2 VERS 5.5.1\n",
        "2 FORM LINEAGE-LINKED\n",
        "1 CHAR UTF-8\n",
        "0 @I1@ INDI\n",
        "1 NAME John /Doe/\n",
        "1 SEX M\n",
        "1 BIRT\n",
        "2 DATE 1 JAN 1980\n",
        "2 PLAC Springfield\n",
        "0 @I2@ INDI\n",
        "1 NAME Jane /Smith/\n",
        "1 SEX F\n",
        "0 @F1@ FAM\n",
        "1 HUSB @I1@\n",
        "1 WIFE @I2@\n",
        "1 MARR\n",
        "2 DATE 15 JUN 2005\n",
        "0 TRLR\n",
    )
}

#[tokio::test]
async fn test_gedcom_import() {
    let app = setup_app().await;

    // Create tree
    let (_, tree_body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "GEDCOM Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Import GEDCOM
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/import"),
        Some(serde_json::json!({ "gedcom": minimal_gedcom() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["persons_count"], 2);
    assert_eq!(body["families_count"], 1);
    assert!(body["events_count"].as_i64().unwrap() >= 2); // BIRT + MARR
    assert!(body["places_count"].as_i64().unwrap() >= 1); // Springfield

    // Verify persons are actually in the DB
    let (status, persons) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let edges = persons["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
}

#[tokio::test]
async fn test_gedcom_import_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let (status, _) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{fake_id}/import"),
        Some(serde_json::json!({ "gedcom": minimal_gedcom() })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_gedcom_export_empty_tree() {
    let app = setup_app().await;

    // Create tree
    let (_, tree_body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Empty Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Export (empty tree)
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["gedcom"].as_str().unwrap().contains("HEAD"));
    assert!(body["warnings"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_gedcom_roundtrip() {
    let app = setup_app().await;

    // Create tree
    let (_, tree_body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Roundtrip Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Import
    let (status, import_body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/import"),
        Some(serde_json::json!({ "gedcom": minimal_gedcom() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Export
    let (status, export_body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exported = export_body["gedcom"].as_str().unwrap();

    // Verify the exported GEDCOM contains the imported data
    assert!(exported.contains("HEAD"));
    assert!(exported.contains("INDI"));
    assert!(exported.contains("FAM"));

    // Verify counts match what we imported
    assert_eq!(import_body["persons_count"], 2);
    assert_eq!(import_body["families_count"], 1);
}

#[tokio::test]
async fn test_gedcom_export_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{fake_id}/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
