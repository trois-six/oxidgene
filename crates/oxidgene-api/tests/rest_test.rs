//! Integration tests for REST API handlers.
//!
//! All tests run against an in-memory SQLite database using Axum's tower
//! `ServiceExt::oneshot` for zero-network-overhead request testing.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use http_body_util::BodyExt;
use oxidgene_api::media::store::{job_blob_key, job_input_blob_key};
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{
    BackgroundJobKind, BackgroundJobRepo, NewBackgroundJob, connect, run_migrations,
};
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
    // Media lands in a throwaway directory: these tests never upload,
    // but `AppState` needs a root and it must not be the developer's.
    let state = AppState::new(db, std::env::temp_dir().join("oxidgene-test-media"));
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

#[tokio::test]
async fn given_name_reference_bundle_is_bounded_per_request() {
    let app = setup_app().await;
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        "/api/v1/reference/fr/given-names/bundle",
        Some(serde_json::json!({ "terms": ["Jean", "Marie", "Jean", "__unknown__"] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array response").len(), 2);
    assert_eq!(body[0]["term"], "Jean");
    assert_eq!(body[1]["term"], "Marie");

    let terms = (0..129).map(|index| index.to_string()).collect::<Vec<_>>();
    let (status, _) = send_request(
        app,
        Method::POST,
        "/api/v1/reference/fr/given-names/bundle",
        Some(serde_json::json!({ "terms": terms })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn openapi_spec_is_generated_from_the_rest_router() {
    let response = setup_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let document: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["info"]["title"], "OxidGene REST API");
    assert_eq!(document["info"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(document["paths"]["/api/v1/trees"]["get"].is_object());
    assert!(document["paths"]["/api/v1/trees"]["post"].is_object());
    assert!(
        document["paths"]["/api/v1/trees/{tree_id}/persons/{person_id}"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .all(|parameter| parameter["schema"]["format"] == "uuid")
    );
    assert!(document["paths"]["/api/v1/openapi.json"]["get"].is_object());
    assert!(document["paths"].get("/graphql").is_none());
    let error_schema = &document["components"]["schemas"]["ErrorEnvelope"];
    assert_eq!(
        error_schema["required"],
        serde_json::json!(["error", "message"])
    );
    assert_eq!(error_schema["properties"]["error"]["type"], "string");
    assert_eq!(error_schema["properties"]["request_id"]["format"], "uuid");
}

// ───────────────────────── Tree guard tests ─────────────────────────

/// Deleting a tree is asynchronous, so its children must stop answering the
/// moment the flag is set — not only once the background purge has run.
#[tokio::test]
async fn deleted_tree_children_are_not_readable() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Reachable while the tree lives.
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The purge may not have run yet; the children must already be gone.
    for path in [
        format!("/api/v1/trees/{tree_id}"),
        format!("/api/v1/trees/{tree_id}/persons"),
        format!("/api/v1/trees/{tree_id}/families"),
        format!("/api/v1/trees/{tree_id}/events"),
        format!("/api/v1/trees/{tree_id}/notes"),
    ] {
        let (status, _) = send_request(app.clone(), Method::GET, &path, None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} must 404 once deleted"
        );
    }
}

/// A tree id that never existed is a 404, not an empty 200.
#[tokio::test]
async fn unknown_tree_id_is_not_found() {
    let app = setup_app().await;
    let missing = uuid::Uuid::now_v7();

    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{missing}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Listing and creating name no tree, so they stay reachable.
    let (status, _) = send_request(app.clone(), Method::GET, "/api/v1/trees", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn person_from_another_tree_is_not_readable_or_mutable() {
    let app = setup_app().await;
    let first_tree = create_tree_via_api(&app).await;
    let second_tree = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &second_tree).await;

    for (method, body) in [
        (Method::GET, None),
        (Method::PUT, Some(serde_json::json!({ "sex": "female" }))),
        (Method::DELETE, None),
    ] {
        let (status, _) = send_request(
            app.clone(),
            method,
            &format!("/api/v1/trees/{first_tree}/persons/{person_id}"),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let (status, _) = send_request(
        app,
        Method::GET,
        &format!("/api/v1/trees/{second_tree}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
            "name": "Doe Family",
            "description": "The Doe family tree"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "Doe Family");
    assert_eq!(body["description"], "The Doe family tree");
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
    assert_eq!(body["name"], "Doe Family");

    // Update the tree
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(serde_json::json!({
            "name": "Doe-Pdoe Family"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Doe-Pdoe Family");

    // An invalid rename uses the public validation contract.
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(serde_json::json!({ "name": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "The request is invalid");
    assert!(body.get("request_id").is_none());

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
async fn tree_self_person_can_be_set_replaced_and_cleared() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let first_person_id = create_person_via_api(&app, &tree_id).await;
    let second_person_id = create_person_via_api(&app, &tree_id).await;

    for expected in [
        Some(first_person_id.as_str()),
        Some(second_person_id.as_str()),
        None,
    ] {
        let (status, body) = send_request(
            app.clone(),
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}"),
            Some(serde_json::json!({ "self_person_id": expected })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["self_person_id"].as_str(), expected);
    }
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

async fn create_named_person_via_api(
    app: &axum::Router,
    tree_id: &str,
    sex: &str,
    given_names: &str,
    surname: &str,
) -> String {
    let (_, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": sex })),
    )
    .await;
    let person_id = body["id"].as_str().unwrap().to_string();
    let (status, _) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": given_names,
            "surname": surname,
            "is_primary": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    person_id
}

#[tokio::test]
async fn relation_labels_are_tree_scoped_and_bounded() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let other_tree_id = create_tree_via_api(&app).await;
    let person_id = create_named_person_via_api(&app, &tree_id, "male", "Alex", "Martin").await;
    let other_person_id =
        create_named_person_via_api(&app, &other_tree_id, "female", "Sam", "Bernard").await;

    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/relation-labels"),
        Some(serde_json::json!({
            "person_ids": [person_id, other_person_id],
            "family_ids": []
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["names"].as_array().unwrap().len(), 1);
    assert_eq!(body["names"][0]["person_id"], person_id);
    assert_eq!(body["spouses"], serde_json::json!([]));

    let person_ids = vec![person_id; 1_025];
    let (status, _) = send_request(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/relation-labels"),
        Some(serde_json::json!({ "person_ids": person_ids, "family_ids": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_update_can_clear_a_nullable_field() {
    // The reported bug: editing a birth name from "de MARTIN" down to "MARTIN"
    // left the person still named "de MARTIN". The UI correctly sent
    // `"surname_prefix": null`,
    // but serde read a JSON null as "field absent" for `Option<Option<T>>`, so
    // the update was accepted and the old particle silently kept.
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Jean",
            "surname": "MARTIN",
            "surname_prefix": "de",
            "nickname": "Jeannot",
            "is_primary": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["surname_prefix"], "de");
    let name_id = body["id"].as_str().unwrap().to_string();

    // An explicit null clears the field...
    let (status, body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
        Some(serde_json::json!({
            "surname": "MARTIN",
            "surname_prefix": null
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["surname_prefix"].is_null(),
        "an explicit null must clear the particle, got {}",
        body["surname_prefix"]
    );
    // ...while a field left out still means "leave unchanged".
    assert_eq!(body["nickname"], "Jeannot");
    assert_eq!(body["surname"], "MARTIN");
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
            "given_names": "John",
            "surname": "Doe",
            "is_primary": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["given_names"], "John");
    assert_eq!(body["surname"], "Doe");
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
            "surname": "Jdoe"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["surname"], "Jdoe");

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

/// Sprint E.6: free-text person search through the normal search path,
/// backed by the `person_search_fts` FTS5 table, end-to-end over HTTP.
#[tokio::test]
async fn test_person_search_free_text() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Two persons with primary names created through the REST API
    // (mutation handlers must keep person_search_fts in sync).
    let p1 = create_person_via_api(&app, &tree_id).await;
    send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Jean",
            "surname": "Dupont",
            "is_primary": true
        })),
    )
    .await;
    let p2 = create_person_via_api(&app, &tree_id).await;
    send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{p2}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Jane",
            "surname": "Smith",
            "is_primary": true
        })),
    )
    .await;

    // Free-text mode returns a SearchResult with entries + total_count.
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=dupont"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["entries"][0]["display_name"], "Jean Dupont");

    // Accent-folded matching (query without accents finds Jane Smith).
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=jane%20smith"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["entries"][0]["display_name"], "Jane Smith");

    // Empty query = browse mode: everyone, sorted by surname.
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q="),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 2);
    assert_eq!(body["entries"][0]["surname_normalized"], "dupont");

    // Renaming through the REST API refreshes the search row.
    let (_, names) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}/names"),
        None,
    )
    .await;
    let name_id = names[0]["id"].as_str().unwrap().to_string();
    let (status, put_body) = send_request(
        app.clone(),
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}/names/{name_id}"),
        Some(serde_json::json!({ "surname": "Martin" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "PUT name failed: {put_body}");

    let (_, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=dupont"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 0);
    let (_, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 1);

    // Deleting a person removes their search row.
    let (status, _) = send_request(
        app.clone(),
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 0);

    // The old cache search endpoint is gone (Sprint E.6).
    let (status, _) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/cache/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Missing `q` behaves like browse mode (only Éloïse remains).
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "browse mode failed: {body}");
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["entries"][0]["display_name"], "Jane Smith");
}

#[tokio::test]
async fn test_person_search_combines_relations_and_pagination() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let subject_one = create_named_person_via_api(&app, &tree_id, "male", "One", "Subject").await;
    let subject_two = create_named_person_via_api(&app, &tree_id, "male", "Two", "Subject").await;
    let relative_alpha =
        create_named_person_via_api(&app, &tree_id, "female", "Alpha", "RelativeMatch").await;
    let relative_beta =
        create_named_person_via_api(&app, &tree_id, "female", "Beta", "RelativeMatch").await;

    for (subject, relative) in [
        (&subject_one, &relative_alpha),
        (&subject_two, &relative_beta),
    ] {
        let (status, family) = send_request(
            app.clone(),
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let family_id = family["id"].as_str().unwrap();

        for (person_id, role) in [(subject, "husband"), (relative, "wife")] {
            let (status, _) = send_request(
                app.clone(),
                Method::POST,
                &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
                Some(serde_json::json!({
                    "person_id": person_id,
                    "role": role,
                    "sort_order": 0
                })),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED);
        }
    }

    let (status, body) = send_request(
        app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree_id}/persons/search?surname=subject&spouse_surname=relative&sort=name_asc&limit=1&offset=1"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "advanced search failed: {body}");
    assert_eq!(body["total_count"], 2);
    assert_eq!(body["entries"].as_array().unwrap().len(), 1);
    assert_eq!(body["entries"][0]["display_name"], "Two Subject");
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
            "description": "Born in London"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["event_type"], "birth");
    assert_eq!(body["description"], "Born in London");
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
            "description": "Born in London"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["description"], "Born in London");

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

#[tokio::test]
async fn person_detail_bundle_excludes_unrelated_person_citations() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let target_id = create_person_via_api(&app, &tree_id).await;
    let unrelated_id = create_person_via_api(&app, &tree_id).await;
    let relevant_source = create_source_via_api(&app, &tree_id).await;
    let unrelated_source = create_source_via_api(&app, &tree_id).await;

    for (source_id, person_id) in [
        (&relevant_source, &target_id),
        (&unrelated_source, &unrelated_id),
    ] {
        let (status, _) = send_request(
            app.clone(),
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/citations"),
            Some(serde_json::json!({
                "source_id": source_id,
                "person_id": person_id,
                "confidence": "high"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, body) = send_request(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{target_id}/detail-bundle"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["persons"].as_array().unwrap().len(), 1);
    assert_eq!(body["persons"][0]["id"], target_id);
    assert_eq!(body["citations"].as_array().unwrap().len(), 1);
    assert_eq!(body["citations"][0]["source_id"], relevant_source);
    assert_eq!(body["sources"].as_array().unwrap().len(), 1);
    assert_eq!(body["sources"][0]["id"], relevant_source);
    assert_eq!(body["profile_media"], serde_json::json!([]));
    assert_eq!(body["profile_vignettes"], serde_json::json!([]));
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
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);

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
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    assert_eq!(body["edges"][0]["node"]["text"], "Person note");

    // List by family — should get 1
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?family_id={family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    assert_eq!(body["edges"][0]["node"]["text"], "Family note");
}

#[tokio::test]
async fn notes_and_citations_use_cursor_pagination() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;
    let other_person_id = create_person_via_api(&app, &tree_id).await;
    let source_id = create_source_via_api(&app, &tree_id).await;

    for text in ["First note", "Second note"] {
        let (status, _) = send_request(
            app.clone(),
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/notes"),
            Some(serde_json::json!({ "text": text, "person_id": person_id })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Other person's note",
            "person_id": other_person_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    for page in ["1", "2"] {
        let (status, _) = send_request(
            app.clone(),
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/citations"),
            Some(serde_json::json!({
                "source_id": source_id,
                "person_id": person_id,
                "page": page,
                "confidence": "high"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/citations"),
        Some(serde_json::json!({
            "source_id": source_id,
            "person_id": other_person_id,
            "page": "3",
            "confidence": "high"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    for resource in ["notes", "citations"] {
        let (status, first_page) = send_request(
            app.clone(),
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/{resource}?person_id={person_id}&first=1"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first_page["total_count"], 2);
        assert_eq!(first_page["edges"].as_array().unwrap().len(), 1);
        assert_eq!(first_page["page_info"]["has_next_page"], true);
        let first_id = first_page["edges"][0]["node"]["id"].as_str().unwrap();
        let cursor = first_page["page_info"]["end_cursor"].as_str().unwrap();

        let (status, second_page) = send_request(
            app.clone(),
            Method::GET,
            &format!(
                "/api/v1/trees/{tree_id}/{resource}?person_id={person_id}&first=1&after={cursor}"
            ),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(second_page["total_count"], 2);
        assert_eq!(second_page["edges"].as_array().unwrap().len(), 1);
        assert_eq!(second_page["page_info"]["has_next_page"], false);
        assert_ne!(second_page["edges"][0]["node"]["id"], first_id);
    }
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

fn gedcom_over_insert_batch_size() -> String {
    let mut gedcom = String::from(
        "0 HEAD\n1 SOUR OxidGene\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n",
    );
    for index in 1..=501 {
        gedcom.push_str(&format!(
            "0 @I{index}@ INDI\n1 NAME Person{index} /Example/\n1 BIRT\n2 DATE 1 JAN 1900\n"
        ));
    }
    gedcom.push_str("0 TRLR\n");
    gedcom
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
        &format!("/api/v1/trees/{tree_id}/gedcom/import"),
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
async fn test_gedcom_import_spans_multiple_insert_batches() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    let (status, body) = send_request(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/gedcom/import"),
        Some(serde_json::json!({ "gedcom": gedcom_over_insert_batch_size() })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["persons_count"], 501);
    assert_eq!(body["events_count"], 501);
}

#[tokio::test]
async fn test_async_file_import_job() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-async-import-media"),
    );
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;

    let (status, started) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{tree_id}/import-jobs?format=gedcom"),
        minimal_gedcom().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = started["job_id"].as_str().expect("job id");
    let temporary = std::env::temp_dir().join("oxidgene-imports").join(job_id);
    assert!(worker.run_once().await.expect("run import job"));

    let completed = loop {
        let (status, progress) = send_request(
            app.clone(),
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        if progress["phase"] == "completed" {
            break progress;
        }
        assert_ne!(progress["phase"], "failed", "job failed: {progress}");
        tokio::task::yield_now().await;
    };

    assert_eq!(completed["result"]["persons_count"], 2);
    assert_eq!(completed["result"]["families_count"], 1);
    assert!(!temporary.exists(), "temporary upload was not removed");

    let (_, persons) = send_request(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(persons["edges"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_async_geneanet_import_stages_and_cleans_inputs() {
    use std::io::Write as _;

    let test_id = uuid::Uuid::now_v7();
    let media_root = std::env::temp_dir().join(format!("oxidgene-test-geneanet-media-{test_id}"));
    let input_root = std::env::temp_dir().join(format!("oxidgene-test-geneanet-input-{test_id}"));
    std::fs::create_dir_all(&input_root).expect("create Geneanet input directory");

    let archive_path = input_root.join("originals.zip");
    let archive = std::fs::File::create(&archive_path).expect("create archive");
    let mut archive = zip::ZipWriter::new(archive);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    archive
        .start_file("unused.txt", options)
        .expect("start archive entry");
    archive.write_all(b"unused").expect("write archive entry");
    archive.finish().expect("finish archive");

    let fetched_path = input_root.join("fetched.jpg");
    std::fs::write(&fetched_path, b"unused fetched medium").expect("write fetched medium");

    let db = setup_db().await;
    let state = AppState::new(db, &media_root).with_local_file_access();
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-geneanet-worker",
    );
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app).await;
    let geneweb = "encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";
    let fetched_url = "https://example.invalid/fetched.jpg";

    let (status, started) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/geneanet/import"),
        Some(serde_json::json!({
            "gw_base64": base64::engine::general_purpose::STANDARD.encode(geneweb),
            "file_name": "family.gw",
            "collection": r#"{"deposits":[],"references":[],"details":[],"view_references":{}}"#,
            "archive_paths": [archive_path],
            "fetched": { fetched_url: fetched_path },
            // The archives are staged only for a run that will read them.
            "media_fidelity": "originals",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "job response: {started}");
    let job_id = started["job_id"]
        .as_str()
        .expect("job id")
        .parse::<uuid::Uuid>()
        .expect("valid job id");
    let source_key = job_blob_key(job_id, "source", "gw").expect("source key");
    let archive_key = job_input_blob_key(job_id, 0);
    let fetched_key = job_input_blob_key(job_id, 1);
    assert!(state.media.exists(&source_key).await);
    assert!(state.media.exists(&archive_key).await);
    assert!(state.media.exists(&fetched_key).await);

    std::fs::remove_dir_all(&input_root).expect("remove original inputs");
    assert!(worker.run_once().await.expect("run Geneanet import job"));

    let (status, completed) = send_request(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["phase"], "completed", "job status: {completed}");
    assert_eq!(completed["geneanet_result"]["persons_count"], 2);
    assert_eq!(completed["geneanet_result"]["families_count"], 1);
    assert!(!state.media.exists(&source_key).await);
    assert!(!state.media.exists(&archive_key).await);
    assert!(!state.media.exists(&fetched_key).await);

    let _ = std::fs::remove_dir_all(media_root);
}

/// A renditions import stores what Geneanet re-encoded, so a data archive is
/// gigabytes copied into job storage to be ignored. It must not be staged, and
/// the media that *is* needed must still land — which is what makes the input
/// numbering worth asserting rather than the mere absence of the archive.
#[tokio::test]
async fn a_renditions_geneanet_import_stages_no_archive() {
    use std::io::Write as _;

    let test_id = uuid::Uuid::now_v7();
    let media_root =
        std::env::temp_dir().join(format!("oxidgene-test-geneanet-renditions-{test_id}"));
    let input_root =
        std::env::temp_dir().join(format!("oxidgene-test-geneanet-rend-input-{test_id}"));
    std::fs::create_dir_all(&input_root).expect("create Geneanet input directory");

    let archive_path = input_root.join("originals.zip");
    let archive = std::fs::File::create(&archive_path).expect("create archive");
    let mut archive = zip::ZipWriter::new(archive);
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
    archive
        .start_file("unused.txt", options)
        .expect("start archive entry");
    archive.write_all(b"unused").expect("write archive entry");
    archive.finish().expect("finish archive");

    let fetched_path = input_root.join("normal.jpg");
    std::fs::write(&fetched_path, b"unused rendition").expect("write fetched rendition");

    let db = setup_db().await;
    let state = AppState::new(db, &media_root).with_local_file_access();
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app).await;
    let geneweb = "encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";

    let (status, started) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/geneanet/import"),
        Some(serde_json::json!({
            "gw_base64": base64::engine::general_purpose::STANDARD.encode(geneweb),
            "file_name": "family.gw",
            "collection": r#"{"deposits":[],"references":[],"details":[],"view_references":{}}"#,
            "archive_paths": [archive_path],
            "fetched": { "https://example.invalid/normal.jpg": fetched_path },
            "media_fidelity": "renditions",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "job response: {started}");
    let job_id = started["job_id"]
        .as_str()
        .expect("job id")
        .parse::<uuid::Uuid>()
        .expect("valid job id");

    // The rendition is input 0, which it can only be if the archive claimed no
    // slot before it.
    assert!(state.media.exists(&job_input_blob_key(job_id, 0)).await);
    assert!(!state.media.exists(&job_input_blob_key(job_id, 1)).await);

    let _ = std::fs::remove_dir_all(input_root);
    let _ = std::fs::remove_dir_all(media_root);
}

#[tokio::test]
async fn geneanet_local_paths_are_refused_by_default() {
    let (status, body) = send_request(
        setup_app().await,
        Method::POST,
        "/api/v1/geneanet/archives",
        Some(serde_json::json!({ "paths": ["/does/not/exist"] })),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "The request is invalid");
    assert!(body["request_id"].is_null());
}

#[tokio::test]
async fn test_geneanet_import_resumes_from_projection_checkpoint() {
    let test_id = uuid::Uuid::now_v7();
    let media_root = std::env::temp_dir().join(format!("oxidgene-test-geneanet-resume-{test_id}"));
    let db = setup_db().await;
    let state = AppState::new(db, &media_root);
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app)
        .await
        .parse::<uuid::Uuid>()
        .expect("valid tree id");
    let job_id = oxidgene_api::service::background_job::stage_geneanet_import(
        &state.db,
        &*state.media,
        tree_id,
        b"encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n",
        "family.gw".to_string(),
        r#"{"deposits":[],"references":[],"details":[],"view_references":{}}"#.to_string(),
        std::collections::HashMap::new(),
        &[],
        &std::collections::HashMap::new(),
        oxidgene_api::service::geneanet::MediaFidelity::default(),
    )
    .await
    .expect("stage Geneanet import");

    let interrupted_worker = "interrupted-geneanet-worker";
    let claimed =
        BackgroundJobRepo::claim_next(&state.db, interrupted_worker, chrono::Duration::seconds(30))
            .await
            .expect("claim job")
            .expect("queued job");
    assert_eq!(claimed.id, job_id);
    let summary = oxidgene_api::service::geneanet::GeneanetImportSummary {
        persons_count: 7,
        families_count: 3,
        warnings: vec!["checkpoint restored".to_string()],
        ..Default::default()
    };
    assert!(
        BackgroundJobRepo::checkpoint_import_persisted(
            &state.db,
            job_id,
            interrupted_worker,
            serde_json::to_string(&summary).expect("serialize summary"),
            chrono::Duration::seconds(30),
        )
        .await
        .expect("checkpoint import")
    );
    let source_key = job_blob_key(job_id, "source", "gw").expect("source key");
    state
        .media
        .delete(&source_key)
        .await
        .expect("remove staged source");
    assert_eq!(
        BackgroundJobRepo::requeue_running(&state.db)
            .await
            .expect("requeue interrupted job"),
        1
    );

    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        state.profiles.clone(),
        state.media.clone(),
        "replacement-geneanet-worker",
    );
    assert!(worker.run_once().await.expect("resume Geneanet import job"));

    let (status, completed) = send_request(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["phase"], "completed", "job status: {completed}");
    assert_eq!(completed["geneanet_result"]["persons_count"], 7);
    assert_eq!(completed["geneanet_result"]["families_count"], 3);
    assert_eq!(
        completed["geneanet_result"]["warnings"],
        serde_json::json!(["checkpoint restored"])
    );

    let _ = std::fs::remove_dir_all(media_root);
}

#[tokio::test]
async fn test_async_export_job_downloads_the_completed_archive() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-async-export-media"),
    );
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-export-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;

    let (status, started) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/export-jobs"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = started["job_id"].as_str().expect("job id");
    assert!(worker.run_once().await.expect("run export job"));

    let (status, completed) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["phase"], "completed");
    let download_url = completed["download_url"].as_str().expect("download URL");

    let response = app
        .oneshot(
            Request::builder()
                .uri(download_url)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"export.gdz\""
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.starts_with(b"PK"));
}

#[tokio::test]
async fn tree_list_marks_only_running_file_imports() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-active-import-media"),
    );
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app)
        .await
        .parse::<uuid::Uuid>()
        .unwrap();
    let job_id = uuid::Uuid::now_v7();
    BackgroundJobRepo::create(
        &state.db,
        NewBackgroundJob {
            id: job_id,
            tree_id,
            kind: BackgroundJobKind::Import,
            format: "gedcom".into(),
            source_key: Some(format!("jobs/{job_id}/source.gedcom")),
            payload_json: None,
            original_filename: None,
            merge_occupations: false,
            merge_names: false,
        },
    )
    .await
    .expect("create import job");

    let (_, running) = send_request(app.clone(), Method::GET, "/api/v1/trees", None).await;
    assert_eq!(running["edges"][0]["node"]["import_in_progress"], true);
    assert_eq!(
        running["edges"][0]["node"]["import_job_id"],
        job_id.to_string()
    );

    let claimed =
        BackgroundJobRepo::claim_next(&state.db, "rest-test-worker", chrono::Duration::seconds(30))
            .await
            .expect("claim import job")
            .expect("queued job");
    BackgroundJobRepo::complete(&state.db, claimed.id, "rest-test-worker", None, None)
        .await
        .expect("complete import job");
    let (_, completed) = send_request(app, Method::GET, "/api/v1/trees", None).await;
    assert_eq!(completed["edges"][0]["node"]["import_in_progress"], false);
    assert!(completed["edges"][0]["node"]["import_job_id"].is_null());
}

#[tokio::test]
async fn test_gedcom_import_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let (status, _) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{fake_id}/gedcom/import"),
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
        &format!("/api/v1/trees/{tree_id}/gedcom/export"),
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
        &format!("/api/v1/trees/{tree_id}/gedcom/import"),
        Some(serde_json::json!({ "gedcom": minimal_gedcom() })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Export
    let (status, export_body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/gedcom/export"),
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
        &format!("/api/v1/trees/{fake_id}/gedcom/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── GeneWeb import ───────────────────────────────────────────────────

/// A `.gw` file: one couple and one child, in GeneWeb's own syntax.
fn minimal_geneweb() -> &'static str {
    concat!(
        "encoding: utf-8\n",
        "\n",
        "fam Doe Jean.0 1980 #bp Springfield +2005 Smith Jeanne.0\n",
        "beg\n",
        "- h Pierre.0 2007\n",
        "end\n",
    )
}

/// Helper: POST a raw binary body (the GeneWeb endpoint takes bytes, not JSON).
async fn send_bytes(app: axum::Router, uri: &str, body: Vec<u8>) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("content-type", "application/octet-stream")
        .body(Body::from(body))
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

#[tokio::test]
async fn test_geneweb_import() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    let (status, body) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{tree_id}/geneweb/import?filename=family.gw"),
        minimal_geneweb().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["persons_count"], 3);
    assert_eq!(body["families_count"], 1);

    // The entities really landed in the database.
    let (status, persons) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(persons["edges"].as_array().unwrap().len(), 3);
}

/// A `.gw` file is ISO-8859-1 unless it opts into UTF-8, so the endpoint takes
/// raw bytes; this is the regression test that nothing decodes them as UTF-8
/// along the way.
#[tokio::test]
async fn test_geneweb_import_latin1_bytes() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // "Émile" with É as the single Latin-1 byte 0xC9 — invalid UTF-8.
    let mut gw = Vec::new();
    gw.extend_from_slice(b"fam Doe \xC9mile.0 + Smith Jeanne.0\n");
    assert!(String::from_utf8(gw.clone()).is_err());

    let (status, body) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{tree_id}/geneweb/import?filename=latin1.gw"),
        gw,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["persons_count"], 2);

    // Search folds accents, so `emile` finds the person either way — what is
    // being asserted is the stored spelling: a lossy UTF-8 decode would have
    // left U+FFFD where the É is.
    let (_, found) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=emile"),
        None,
    )
    .await;
    assert_eq!(found["total_count"], 1, "search returned: {found}");
    assert_eq!(found["entries"][0]["display_name"], "Émile Doe");
}

#[tokio::test]
async fn test_geneweb_import_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let (status, _) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{fake_id}/geneweb/import"),
        minimal_geneweb().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_geneweb_import_unparseable_file() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    let (status, _) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{tree_id}/geneweb/import"),
        b"this is not a gw file at all\n".to_vec(),
    )
    .await;
    assert_ne!(status, StatusCode::CREATED);
}

// ───────────────────── Profile & pedigree routes ─────────────────────

/// The projection routes replaced `/cache/*` in Sprint E.9. This walks the
/// whole surface through the real router — route ordering included, since
/// `/profiles/rebuild` and `/profiles/{person_id}` share a path segment.
#[tokio::test]
async fn test_profile_routes() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Jean",
            "surname": "Dupont",
            "is_primary": true
        })),
    )
    .await;

    // Single projection.
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "GET profile failed: {body}");
    assert_eq!(body["person_id"], person_id);
    assert_eq!(body["primary_name"]["display_name"], "Jean Dupont");

    // Whole-tree listing.
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "GET profiles failed: {body}");
    assert_eq!(body.as_array().unwrap().len(), 1);

    // `rebuild` must not be swallowed by the `{person_id}` route.
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/rebuild"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tree rebuild failed: {body}");
    assert_eq!(body["persons_count"], 1);

    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/rebuild/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "person rebuild failed: {body}");
    assert_eq!(body["persons_count"], 1);

    // Pedigree rooted on the only person.
    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!(
            "/api/v1/trees/{tree_id}/pedigree/{person_id}?ancestor_depth=2&descendant_depth=1"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "GET pedigree failed: {body}");
    assert_eq!(body["root_person_id"], person_id);
    assert_eq!(body["persons"][&person_id]["display_name"], "Jean Dupont");
    assert_eq!(body["ancestor_depth_loaded"], 2);

    // Expansion returns a (here empty) delta, not an error.
    let (status, body) = send_request(
        app.clone(),
        Method::PATCH,
        &format!(
            "/api/v1/trees/{tree_id}/pedigree/{person_id}/expand\
             ?direction=ancestors&from_depth=2&to_depth=4&other_depth=1"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "expand failed: {body}");
    assert_eq!(body["ancestor_depth_loaded"], 4);
    assert_eq!(body["descendant_depth_loaded"], 1);
    assert!(body["new_nodes"].as_array().unwrap().is_empty());

    // Dropping clears the projections; the next read re-materializes them.
    let (status, body) = send_request(
        app.clone(),
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/drop"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drop failed: {body}");
    assert_eq!(body["dropped"], true);

    let (status, body) = send_request(
        app.clone(),
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "re-materialization failed: {body}");
    assert_eq!(body["primary_name"]["display_name"], "Jean Dupont");

    // The old `/cache/*` paths are gone (Sprint E.9).
    for path in [
        format!("/api/v1/trees/{tree_id}/cache/persons/{person_id}"),
        format!("/api/v1/trees/{tree_id}/cache/persons"),
        format!("/api/v1/trees/{tree_id}/cache/pedigree/{person_id}"),
    ] {
        let (status, _) = send_request(app.clone(), Method::GET, &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "still routed: {path}");
    }
}
