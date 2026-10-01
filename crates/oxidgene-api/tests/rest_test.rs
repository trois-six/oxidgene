//! Integration tests for REST API handlers.
//!
//! All tests run against an in-memory SQLite database using Axum's tower
//! `ServiceExt::oneshot` for zero-network-overhead request testing.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use base64::Engine as _;
use http_body_util::BodyExt;
use oxidgene_api::media::store::{job_blob_key, job_input_blob_key};
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{BackgroundJobKind, BackgroundJobRepo, NewBackgroundJob};
use serde_json::Value;
use tower::ServiceExt;

use common::{send, setup_app, setup_db};

/// Every API response says it is not to be sniffed, framed or followed with a
/// referrer, and JSON says it is no page at all.
#[tokio::test]
async fn api_responses_carry_the_security_headers() {
    let app = setup_app().await;
    for uri in ["/api/v1/trees", "/api/v1/no-such-route"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let headers = response.headers();
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(header("x-content-type-options"), "nosniff", "{uri}");
        assert_eq!(header("x-frame-options"), "DENY", "{uri}");
        assert_eq!(header("referrer-policy"), "no-referrer", "{uri}");
        assert_eq!(
            header("content-security-policy"),
            "default-src 'none'; frame-ancestors 'none'",
            "{uri}"
        );
    }
}

#[tokio::test]
async fn given_name_reference_bundle_is_bounded_per_request() {
    let app = setup_app().await;
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/reference/fr/given-names/bundle",
        Some(serde_json::json!({ "terms": terms })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn occupation_reference_bundle_is_bounded_per_request() {
    let app = setup_app().await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/reference/fr/occupations/bundle",
        Some(serde_json::json!({
            "terms": ["Laboureur", "Forgeron", "Laboureur", "__unknown__"]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().expect("array response").len(), 2);
    assert_eq!(body[0]["term"], "Laboureur");
    assert_eq!(body[1]["term"], "Forgeron");
    // The entry itself is flattened alongside the term, as for given names.
    assert_eq!(body[0]["label"], "Laboureur");

    let terms = (0..129).map(|index| index.to_string()).collect::<Vec<_>>();
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/v1/reference/fr/occupations/bundle",
        Some(serde_json::json!({ "terms": terms })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_basemap_names_its_populated_places_by_zoom() {
    let app = setup_app().await;
    let (status, body) = send(&app, Method::GET, "/api/v1/reference/basemap", None).await;
    assert_eq!(status, StatusCode::OK);
    let france = body
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["iso"] == "FR")
        .expect("France is on the basemap");
    let cities = france["cities"].as_array().unwrap();
    assert!(cities.len() > 10);
    // The first labelled is the capital, from a low zoom.
    assert!(cities[0]["zoom"].as_i64().unwrap() < 30);
    assert!(cities[0]["names"].is_array());
}

#[tokio::test]
async fn place_suggestions_come_from_the_place_dictionary() {
    let app = setup_app().await;
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/en/places?q=paris&limit=3",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let places = body.as_array().expect("array response");
    assert_eq!(places.len(), 3);
    assert_eq!(
        places[0]["label"],
        "Paris, 75056, Paris, Île-de-France, France"
    );
    assert_eq!(places[0]["kind"], "commune");
    assert_eq!(places[0]["code"], "75056");
    assert_eq!(places[0]["current"], true);

    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/de/places?q=paris&limit=1",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body[0]["label"],
        "Paris, 75056, Paris, Île-de-France, Frankreich"
    );

    let (status, body) = send(&app, Method::GET, "/api/v1/reference/fr/places?q=%20", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    for uri in [
        "/api/v1/reference/fr/places?q=paris&limit=51",
        "/api/v1/reference/fr/places?q=paris&limit=0",
        "/api/v1/reference/xx/places?q=paris",
    ] {
        let (status, _) = send(&app, Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
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
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = send(
        &app,
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
        let (status, _) = send(&app, Method::GET, &path, None).await;
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

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{missing}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Listing and creating name no tree, so they stay reachable.
    let (status, _) = send(&app, Method::GET, "/api/v1/trees", None).await;
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
        let (status, _) = send(
            &app,
            method,
            &format!("/api/v1/trees/{first_tree}/persons/{person_id}"),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    let (status, _) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(&app, Method::GET, &format!("/api/v1/trees/{tree_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Doe Family");

    // Update the tree
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(&app, Method::GET, "/api/v1/trees", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);

    // Delete the tree
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone (soft-deleted)
    let (status, _) = send(&app, Method::GET, &format!("/api/v1/trees/{tree_id}"), None).await;
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
        let (status, body) = send(
            &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(&app, Method::GET, &format!("/api/v1/trees/{fake_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn test_tree_pagination() {
    let app = setup_app().await;

    // Create 3 trees
    for i in 0..3 {
        send(
            &app,
            Method::POST,
            "/api/v1/trees",
            Some(serde_json::json!({
                "name": format!("Tree {i}")
            })),
        )
        .await;
    }

    // Get first 2
    let (status, body) = send(&app, Method::GET, "/api/v1/trees?first=2", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 2);
    assert!(body["page_info"]["has_next_page"].as_bool().unwrap());
    let cursor = body["page_info"]["end_cursor"].as_str().unwrap();

    // Get next page
    let (status, body) = send(
        &app,
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
    let (_, body) = send(
        app,
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
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "male" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["sex"], "male");
    let person_id = body["id"].as_str().unwrap().to_string();

    // Get the person
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sex"], "male");

    // Update the person
    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        Some(serde_json::json!({ "sex": "female" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sex"], "female");

    // List persons
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the person
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let (_, body) = send(
        app,
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
    let (_, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": sex })),
    )
    .await;
    let person_id = body["id"].as_str().unwrap().to_string();
    let (status, _) = send(
        app,
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

/// The entry forms' suggestions: the tree's own values, per word for given
/// names, then the reference sheets' terms.
#[tokio::test]
async fn value_suggestions_come_from_the_tree_then_the_sheets() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let other_tree_id = create_tree_via_api(&app).await;
    let person_id =
        create_named_person_via_api(&app, &tree_id, "male", "Jean Given_a", "Sample").await;
    create_named_person_via_api(&app, &other_tree_id, "male", "Jeannot", "Samplex").await;
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({
            "event_type": "occupation",
            "person_id": person_id,
            "description": "Laboureur"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(serde_json::json!({ "title": "Sample register" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let get = |uri: String| {
        let app = app.clone();
        async move { send(&app, Method::GET, &uri, None).await }
    };

    let (status, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/family-names?q=sam&lang=en"
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!([{ "value": "Sample", "count": 1, "reference": false }])
    );

    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/given-names?q=giv&lang=en"
    ))
    .await;
    assert_eq!(body[0]["value"], "Given_a");

    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/given-names?q=jea&lang=fr&limit=3"
    ))
    .await;
    assert_eq!(
        body[0],
        serde_json::json!({ "value": "Jean", "count": 1, "reference": true })
    );
    assert!(
        body.as_array().unwrap()[1..]
            .iter()
            .all(|s| s["count"] == 0 && s["reference"] == true)
    );
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .all(|s| s["value"] != "Jeannot" || s["count"] == 0)
    );

    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/occupations?q=labou&lang=fr"
    ))
    .await;
    assert_eq!(
        body[0],
        serde_json::json!({ "value": "Laboureur", "count": 1, "reference": true })
    );

    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/sources?q=regis&lang=fr"
    ))
    .await;
    assert_eq!(
        body,
        serde_json::json!([{ "value": "Sample register", "count": 0, "reference": false }])
    );

    let (status, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/sources?q=%20&lang=fr"
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!([]));

    // Scoped by the other name field, a name counts only the persons a
    // search on both would find, and no sheet term fills the list.
    create_named_person_via_api(&app, &tree_id, "male", "Jean", "Otherx").await;
    let (status, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/given-names?q=jea&lang=fr&surname=sampl"
    ))
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        serde_json::json!([{ "value": "Jean", "count": 1, "reference": true }])
    );
    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/given-names?q=jea&lang=fr&limit=1"
    ))
    .await;
    assert_eq!(body[0]["count"], 2, "unscoped, the whole tree counts");
    let (_, body) = get(format!(
        "/api/v1/trees/{tree_id}/suggestions/family-names?q=s&lang=fr&given_names=given_a"
    ))
    .await;
    assert_eq!(
        body,
        serde_json::json!([{ "value": "Sample", "count": 1, "reference": false }])
    );

    for uri in [
        format!("/api/v1/trees/{tree_id}/suggestions/places?q=a&lang=fr"),
        format!("/api/v1/trees/{tree_id}/suggestions/sources?q=a&lang=xx"),
        format!("/api/v1/trees/{tree_id}/suggestions/sources?q=a&lang=fr&limit=0"),
        format!("/api/v1/trees/{tree_id}/suggestions/sources?q=a&lang=fr&limit=51"),
        format!("/api/v1/trees/{tree_id}/suggestions/occupations?q=a&lang=fr&surname=sam"),
    ] {
        let (status, _) = get(uri.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
    }
}

/// Reads keyed by a record ID must not answer for a record of another tree,
/// even when the handler goes through a projection or an aggregate rather than
/// the record's own table.
#[tokio::test]
async fn projection_and_usage_reads_are_tree_scoped() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let other_tree_id = create_tree_via_api(&app).await;
    let other_person_id =
        create_named_person_via_api(&app, &other_tree_id, "female", "Sam", "Sample").await;
    let (status, source) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{other_tree_id}/sources"),
        Some(serde_json::json!({ "title": "Sample register" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let source_id = source["id"].as_str().unwrap().to_string();
    let (status, place) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{other_tree_id}/places"),
        Some(serde_json::json!({ "name": "Sampleville" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let place_id = place["id"].as_str().unwrap().to_string();

    for path in [
        format!("/api/v1/trees/{tree_id}/profiles/{other_person_id}"),
        format!(
            "/api/v1/trees/{tree_id}/pedigree/{other_person_id}?ancestor_depth=1&descendant_depth=1"
        ),
        format!("/api/v1/trees/{tree_id}/dictionary/sources/{source_id}/usage"),
        format!("/api/v1/trees/{tree_id}/dictionary/places/{place_id}/usage"),
    ] {
        let (status, body) = send(&app, Method::GET, &path, None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{path} answered across trees: {body}"
        );
    }

    // The same reads answer in the record's own tree.
    for path in [
        format!("/api/v1/trees/{other_tree_id}/profiles/{other_person_id}"),
        format!(
            "/api/v1/trees/{other_tree_id}/pedigree/{other_person_id}?ancestor_depth=1&descendant_depth=1"
        ),
        format!("/api/v1/trees/{other_tree_id}/dictionary/sources/{source_id}/usage"),
        format!("/api/v1/trees/{other_tree_id}/dictionary/places/{place_id}/usage"),
    ] {
        let (status, body) = send(&app, Method::GET, &path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
    }
}

#[tokio::test]
async fn relation_labels_are_tree_scoped_and_bounded() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let other_tree_id = create_tree_via_api(&app).await;
    let person_id = create_named_person_via_api(&app, &tree_id, "male", "Alex", "Martin").await;
    let other_person_id =
        create_named_person_via_api(&app, &other_tree_id, "female", "Sam", "Bernard").await;

    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
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

    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);

    // Update name
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, body) = send(
        &app,
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
    send(
        &app,
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
    send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=dupont"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["entries"][0]["display_name"], "Jean Dupont");

    // Accent-folded matching (query without accents finds Jane Smith).
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=jane%20smith"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);
    assert_eq!(body["entries"][0]["display_name"], "Jane Smith");

    // Empty query = browse mode: everyone, sorted by surname.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q="),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 2);
    assert_eq!(body["entries"][0]["surname_normalized"], "dupont");

    // Renaming through the REST API refreshes the search row.
    let (_, names) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}/names"),
        None,
    )
    .await;
    let name_id = names[0]["id"].as_str().unwrap().to_string();
    let (status, put_body) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}/names/{name_id}"),
        Some(serde_json::json!({ "surname": "Martin" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "PUT name failed: {put_body}");

    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=dupont"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 0);
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 1);

    // Deleting a person removes their search row.
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/persons/{p1}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(body["total_count"], 0);

    // The old cache search endpoint is gone (Sprint E.6).
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/cache/search?q=martin"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Missing `q` behaves like browse mode (only Éloïse remains).
    let (status, body) = send(
        &app,
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
        let (status, family) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let family_id = family["id"].as_str().unwrap();

        for (person_id, role) in [(subject, "husband"), (relative, "wife")] {
            let (status, _) = send(
                &app,
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

    let (status, body) = send(
        &app,
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

    // An accent in the filter must not change the answer: the relative names
    // on the search row are accent-folded, like the subject's own.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?spouse_surname=relativem%C3%A1tch"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "accented spouse filter: {body}");
    assert_eq!(body["total_count"], 2);

    // The relatives ride on the result, so a caller needs no second request.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?surname=subject&sort=name_asc"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "search failed: {body}");
    let entry = &body["entries"][0];
    assert_eq!(entry["display_name"], "One Subject");
    assert_eq!(entry["spouse_names"][0], "Alpha RelativeMatch");
    assert!(entry["father_name"].is_null());
    assert!(entry["mother_name"].is_null());
    assert_eq!(entry["children_count"], 0);
    assert_eq!(entry["birth_qualifier"], "exact");
}

/// The filters are read by their own extractor, beside the one for text and
/// paging: typed values (an enum, a boolean, a year) must still parse from the
/// shared query string, and an oversized page is capped rather than refused.
#[tokio::test]
async fn test_person_search_reads_typed_filters_beside_paging() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    for (sex, given_names) in [("male", "Alpha"), ("female", "Beta"), ("female", "Gamma")] {
        create_named_person_via_api(&app, &tree_id, sex, given_names, "Sample").await;
    }

    let (status, body) = send(
        &app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree_id}/persons/search?q=sample&sex=female&has_media=false&sort=name_desc&limit=500"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "typed filters failed: {body}");
    assert_eq!(body["total_count"], 2);
    assert_eq!(body["entries"][0]["display_name"], "Gamma Sample");
    assert_eq!(body["entries"][1]["display_name"], "Beta Sample");

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?surname=sample&has_media=true"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "media filter failed: {body}");
    assert_eq!(body["total_count"], 0);
}

/// The filters on events — place, type and years together, the family's
/// events through its spouses, occupations — each matching only the tree
/// searched, though a second tree holds the same people.
#[tokio::test]
async fn test_person_search_filters_on_events() {
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let gedcom = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n\
        0 @I1@ INDI\n1 NAME Alpha /Sample/\n1 SEX M\n1 BIRT\n2 DATE 1850\n\
        2 PLAC Springfield\n1 OCCU Baker\n1 FAMS @F1@\n\
        0 @I2@ INDI\n1 NAME Beta /Sample/\n1 SEX F\n1 FAMS @F1@\n\
        0 @I3@ INDI\n1 NAME Gamma /Sample/\n1 SEX F\n\
        0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 MARR\n2 DATE 1875\n2 PLAC Riverside\n\
        0 TRLR\n";
    let mut trees = Vec::new();
    for _ in 0..2 {
        let tree_id = create_tree_via_api(&app).await;
        common::import_gedcom(&app, &db, &tree_id, gedcom).await;
        trees.push(tree_id);
    }

    let names = |body: &Value| -> Vec<String> {
        let mut names: Vec<String> = body["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["display_name"].as_str().unwrap().to_owned())
            .collect();
        names.sort();
        names
    };
    for (query, expected) in [
        // The marriage is the family's event: both spouses match it.
        ("place=riverside", vec!["Alpha Sample", "Beta Sample"]),
        ("place=springfield&event_type=birth", vec!["Alpha Sample"]),
        (
            "event_type=birth&event_from=1840&event_to=1860",
            vec!["Alpha Sample"],
        ),
        ("event_type=birth&event_from=1860", vec![]),
        ("place=riverside&event_type=birth", vec![]),
        ("occupation=bak", vec!["Alpha Sample"]),
        ("occupation=smith", vec![]),
    ] {
        let (status, body) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{}/persons/search?{query}", trees[0]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{query} failed: {body}");
        assert_eq!(names(&body), expected, "{query}");
        assert_eq!(body["total_count"], expected.len(), "{query}");
    }
}

// ───────────────────────── Family tests ─────────────────────────

#[tokio::test]
async fn test_family_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Create a family
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let family_id = body["id"].as_str().unwrap().to_string();

    // Get the family
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Update the family (touches updated_at)
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // List families
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the family
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();

    // Add a spouse
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
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
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();

    // Add a child
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/families/{family_id}/children/{child_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Homonym tests ─────────────────────────

/// Helper: create a family and link its members; returns the family ID.
async fn create_family_via_api(
    app: &axum::Router,
    tree_id: &str,
    spouses: &[(&str, &str)],
    children: &[&str],
) -> String {
    let (_, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();
    for (person_id, role) in spouses {
        let (status, _) = send(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
            Some(serde_json::json!({ "person_id": person_id, "role": role, "sort_order": 0 })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    for person_id in children {
        let (status, _) = send(
            app,
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
    }
    family_id
}

async fn homonym_ids(app: &axum::Router, tree_id: &str, person_id: &str) -> Vec<String> {
    let (status, body) = send(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/homonyms"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "homonyms: {body}");
    body.as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["person_id"].as_str().unwrap().to_string())
        .collect()
}

/// Case and accents do not make two names different; a different surname
/// does. Confirming two persons distinct takes each off the other's list.
#[tokio::test]
async fn homonyms_are_listed_until_confirmed_distinct() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let first = create_named_person_via_api(&app, &tree_id, "female", "Élise", "Sample").await;
    let second = create_named_person_via_api(&app, &tree_id, "female", "elise", "SAMPLE").await;
    create_named_person_via_api(&app, &tree_id, "female", "Élise", "Other").await;

    assert_eq!(
        homonym_ids(&app, &tree_id, &first).await,
        vec![second.clone()]
    );
    assert_eq!(
        homonym_ids(&app, &tree_id, &second).await,
        vec![first.clone()]
    );

    for _ in 0..2 {
        // Answering twice records one pair and changes nothing.
        let (status, body) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/persons/{first}/distinct"),
            Some(serde_json::json!({ "person_ids": [second] })),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "distinct: {body}");
    }
    assert!(homonym_ids(&app, &tree_id, &first).await.is_empty());
    assert!(homonym_ids(&app, &tree_id, &second).await.is_empty());

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{first}/distinct"),
        Some(serde_json::json!({ "person_ids": [first] })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "nobody differs from themselves"
    );

    let other_tree_id = create_tree_via_api(&app).await;
    let stranger =
        create_named_person_via_api(&app, &other_tree_id, "female", "Élise", "Sample").await;
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{first}/distinct"),
        Some(serde_json::json!({ "person_ids": [stranger] })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another tree's person is not found"
    );
}

/// Everything the duplicate carried lands on the kept person, links that
/// would double are not doubled, and the relatives' projections follow.
#[tokio::test]
async fn merging_moves_the_duplicate_onto_the_kept_person() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let kept = create_named_person_via_api(&app, &tree_id, "female", "Élise", "Sample").await;
    let (_, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "unknown" })),
    )
    .await;
    let duplicate = body["id"].as_str().unwrap().to_string();
    for (name_type, surname, primary) in [("birth", "SAMPLE", true), ("married", "Spouse", false)] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/persons/{duplicate}/names"),
            Some(serde_json::json!({
                "name_type": name_type,
                "given_names": "Élise",
                "surname": surname,
                "is_primary": primary
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let parent = create_named_person_via_api(&app, &tree_id, "male", "Parent", "Sample").await;
    let partner = create_named_person_via_api(&app, &tree_id, "male", "Partner", "Spouse").await;
    let child = create_named_person_via_api(&app, &tree_id, "male", "Child", "Spouse").await;
    // Both records are the parent's child: after the merge, once.
    let parents = create_family_via_api(
        &app,
        &tree_id,
        &[(&parent, "husband")],
        &[&kept, &duplicate],
    )
    .await;
    let union = create_family_via_api(
        &app,
        &tree_id,
        &[(&partner, "husband"), (&duplicate, "wife")],
        &[&child],
    )
    .await;
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({
            "event_type": "occupation",
            "person_id": duplicate,
            "description": "Weaver"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({ "text": "A note", "person_id": duplicate })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        homonym_ids(&app, &tree_id, &kept).await,
        vec![duplicate.clone()]
    );

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{kept}/merge"),
        Some(serde_json::json!({ "duplicate_id": duplicate })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "merge: {body}");
    assert_eq!(body["id"], kept.as_str());
    assert_eq!(body["sex"], "female", "the kept person's sex wins");

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{duplicate}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, profile) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{kept}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_name"]["surname"], "Sample");
    let other_names = profile["other_names"].as_array().unwrap();
    assert_eq!(
        other_names.len(),
        1,
        "the identical birth name is not doubled"
    );
    assert_eq!(other_names[0]["surname"], "Spouse");
    assert_eq!(profile["occupation"], "Weaver");
    assert_eq!(profile["note_count"], 1);
    assert_eq!(profile["family_as_child"]["family_id"], parents.as_str());
    let unions = profile["families_as_spouse"].as_array().unwrap();
    assert_eq!(unions.len(), 1);
    assert_eq!(unions[0]["family_id"], union.as_str());
    assert_eq!(unions[0]["spouse_id"], partner.as_str());
    assert_eq!(unions[0]["children_ids"], serde_json::json!([child]));

    let (_, parent_profile) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{parent}"),
        None,
    )
    .await;
    assert_eq!(
        parent_profile["families_as_spouse"][0]["children_ids"],
        serde_json::json!([kept]),
        "the parent's projection no longer counts the duplicate"
    );
    let (_, partner_profile) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{partner}"),
        None,
    )
    .await;
    assert_eq!(
        partner_profile["families_as_spouse"][0]["spouse_id"],
        kept.as_str()
    );
    assert!(homonym_ids(&app, &tree_id, &kept).await.is_empty());
}

/// A merge that would marry somebody to themselves or make them their own
/// ancestor is refused, and so is merging a person with themselves.
#[tokio::test]
async fn merging_refuses_spouses_ancestors_and_the_same_person() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let husband = create_named_person_via_api(&app, &tree_id, "male", "Sam", "Sample").await;
    let wife = create_named_person_via_api(&app, &tree_id, "female", "Sam", "Sample").await;
    let child = create_named_person_via_api(&app, &tree_id, "male", "Sam", "Sample").await;
    create_family_via_api(
        &app,
        &tree_id,
        &[(&husband, "husband"), (&wife, "wife")],
        &[&child],
    )
    .await;

    for (kept, duplicate) in [
        (&husband, &wife),
        (&husband, &child),
        (&child, &wife),
        (&husband, &husband),
    ] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/persons/{kept}/merge"),
            Some(serde_json::json!({ "duplicate_id": duplicate })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    assert_eq!(homonym_ids(&app, &tree_id, &husband).await.len(), 2);
}

// ───────────────────────── Ancestry tests ─────────────────────────

#[tokio::test]
async fn test_ancestors_descendants_empty() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Ancestors — should be empty
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/ancestors"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 0);

    // Descendants — should be empty
    let (status, body) = send(
        &app,
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

    let (status, body) = send(&app, Method::GET, "/api/v1/trees/not-a-uuid", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error", "{body}");
    assert_eq!(body["message"], "The request is invalid");
}

#[tokio::test]
async fn test_invalid_json_body_returns_error() {
    let app = setup_app().await;

    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/trees")
        .header("content-type", "application/json")
        .body(Body::from("{\"invalid json"))
        .unwrap();

    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("an error envelope");
    assert_eq!(body["error"], "validation_error", "{body}");

    // Well-formed JSON missing a required field: Axum's `422`, reported as
    // the contract's `400`.
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "description": "no name" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error", "{body}");
}

#[tokio::test]
async fn rejections_outside_the_handlers_use_the_error_envelope() {
    let app = setup_app().await;

    // No JSON content type at all.
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/trees")
        .body(Body::from("{}"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).expect("an error envelope");
    assert_eq!(body["error"], "unsupported_media_type", "{body}");

    // A route that does not exist.
    let (status, body) = send(&app, Method::GET, "/api/v1/no-such-route", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found", "{body}");
}

// ───────────────────────── Event tests ─────────────────────────

#[tokio::test]
async fn test_event_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create an event
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["event_type"], "birth");

    // Update the event
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List events (filter by person_id)
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List events (filter by event_type)
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?event_type=birth"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the event
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places/{place_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Paris, France");

    // Update the place
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // List places with search
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places?search=Lyon"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Search for non-existent place
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places?search=Berlin"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 0);

    // Delete the place
    let (status, _) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["title"], "Parish Records of Lyon");

    // Update the source
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/sources"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    // Delete the source
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (_, body) = send(
        app,
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
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
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
        let (status, _) = send(
            &app,
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

    let (status, body) = send(
        &app,
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

#[tokio::test]
async fn person_detail_bundle_names_the_couple_a_profile_media_comes_from() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;
    let partner_id = create_person_via_api(&app, &tree_id).await;

    let (status, family) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let family_id = family["id"].as_str().unwrap().to_string();
    for (spouse_id, role) in [(&person_id, "husband"), (&partner_id, "wife")] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
            Some(serde_json::json!({ "person_id": spouse_id, "role": role })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let own_document = create_document_via_api(&app, &tree_id).await;
    let couple_document = create_document_via_api(&app, &tree_id).await;
    for link in [
        serde_json::json!({ "media_id": own_document, "person_id": person_id }),
        serde_json::json!({ "media_id": couple_document, "family_id": family_id }),
    ] {
        let (status, body) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media-links"),
            Some(link),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let tiles = body["profile_media"].as_array().unwrap();
    assert_eq!(tiles.len(), 2, "{body}");
    for tile in tiles {
        let expected = if tile["id"] == own_document {
            serde_json::Value::Null
        } else {
            serde_json::json!(family_id)
        };
        assert_eq!(tile["family_id"], expected, "{tile}");
    }
}

// ───────────────────────── Media tests ─────────────────────────

async fn create_document_via_api(app: &axum::Router, tree_id: &str) -> String {
    let (status, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/document"),
        Some(serde_json::json!({"title": "Sample document"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["title"], "Sample document");
    assert_eq!(body["page_count"], 0);
    body["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn test_media_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let document_id = create_document_via_api(&app, &tree_id).await;

    // Create media
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "document_id": document_id,
            "file_name": "photo.jpg",
            "mime_type": "image/jpeg",
            "file_path": "/uploads/photo.jpg",
            "file_size": 1024000
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["file_name"], "photo.jpg");
    assert_eq!(body["parent_media_id"], document_id);
    let media_id = body["id"].as_str().unwrap().to_string();

    // Get media
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/media/{media_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["file_name"], "photo.jpg");

    // Descriptive metadata belongs to the document, not its page.
    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/media/{document_id}"),
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/media"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total_count"], 1);

    assert_eq!(body["edges"][0]["node"]["id"], document_id);

    // Deleting the document also removes its page.
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media/{document_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let document_id = create_document_via_api(&app, &tree_id).await;

    // Empty file_name should fail
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "document_id": document_id,
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
    let document_id = create_document_via_api(&app, &tree_id).await;

    // Create media first
    let (status, media_body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(serde_json::json!({
            "document_id": document_id,
            "file_name": "doc.pdf",
            "mime_type": "application/pdf",
            "file_path": "/uploads/doc.pdf",
            "file_size": 2048
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{media_body}");
    assert_eq!(media_body["parent_media_id"], document_id);
    let media_id = document_id;

    // Create a media link
    let (status, body) = send(
        &app,
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
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media-links/{link_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

// ───────────────────────── Note tests ─────────────────────────

#[tokio::test]
async fn a_note_without_text_is_refused() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({ "text": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_note_crud() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_person_via_api(&app, &tree_id).await;

    // Create a note
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["text"], "Important note about this person");

    // Update the note
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);

    // Delete the note
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Verify it's gone
    let (status, _) = send(
        &app,
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
    let (status, body) = send(
        &app,
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
    send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Person note",
            "person_id": person_id
        })),
    )
    .await;

    // Create a family and a note linked to it
    let (_, fam_body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = fam_body["id"].as_str().unwrap().to_string();

    send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(serde_json::json!({
            "text": "Family note",
            "family_id": family_id
        })),
    )
    .await;

    // List by person — should get 1
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes?person_id={person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    assert_eq!(body["edges"][0]["node"]["text"], "Person note");

    // List by family — should get 1
    let (status, body) = send(
        &app,
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
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/notes"),
            Some(serde_json::json!({ "text": text, "person_id": person_id })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = send(
        &app,
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
        let (status, _) = send(
            &app,
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
    let (status, _) = send(
        &app,
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
        let (status, first_page) = send(
            &app,
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

        let (status, second_page) = send(
            &app,
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
    let db = setup_db().await;
    let app = common::app_on(db.clone());

    // Create tree
    let (_, tree_body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "GEDCOM Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Import GEDCOM
    let body = common::import_gedcom(&app, &db, tree_id, minimal_gedcom()).await;
    assert_eq!(body["persons_count"], 2);
    assert_eq!(body["families_count"], 1);
    assert!(body["events_count"].as_i64().unwrap() >= 2); // BIRT + MARR
    assert!(body["places_count"].as_i64().unwrap() >= 1); // Springfield

    // Verify persons are actually in the DB
    let (status, persons) = send(
        &app,
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
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = create_tree_via_api(&app).await;

    let body = common::import_gedcom(&app, &db, &tree_id, &gedcom_over_insert_batch_size()).await;

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
        let (status, progress) = send(
            &app,
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

    let (_, persons) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(persons["edges"].as_array().unwrap().len(), 2);
}

/// A `.ged` written in Windows-1252, as `CHAR ANSI` software does, imports
/// through the job queue with its accents instead of failing as invalid
/// UTF-8.
#[tokio::test]
async fn a_queued_gedcom_import_decodes_its_declared_character_set() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join("oxidgene-test-ansi-import-media"),
    );
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-ansi-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;

    let gedcom = b"0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR ANSI\n\
                   0 @I1@ INDI\n1 NAME Ren\xe9e /Alpha/\n0 TRLR\n";
    let (status, started) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{tree_id}/import-jobs?format=gedcom"),
        gedcom.to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = started["job_id"].as_str().expect("job id");
    assert!(worker.run_once().await.expect("run import job"));
    loop {
        let (_, progress) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
            None,
        )
        .await;
        assert_ne!(progress["phase"], "failed", "job failed: {progress}");
        if progress["phase"] == "completed" {
            break;
        }
        tokio::task::yield_now().await;
    }

    let (_, persons) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/search?q=Alpha"),
        None,
    )
    .await;
    assert!(persons.to_string().contains("Renée"), "{persons}");
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

    let (status, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/geneanet/import"),
        Some(serde_json::json!({
            "gw_base64": base64::engine::general_purpose::STANDARD.encode(geneweb),
            "file_name": "family.gw",
            // One photograph naming somebody outside the tree, whom the
            // receipt lists; its bytes were never gathered, so it is skipped.
            "collection": serde_json::json!({
                "deposits": [{"id": 1, "views": [{"id": 10, "files": {"normal": "https://example.invalid/normal.jpg"}}]}],
                "references": [{
                    "deposit": {"id": 1, "views": [{"id": 10}]},
                    "firstname": "person_c",
                    "lastname": "BRANCH_C",
                }],
            })
            .to_string(),
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

    let (status, completed) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["phase"], "completed", "job status: {completed}");
    assert_eq!(completed["geneanet_result"]["persons_count"], 2);
    assert_eq!(completed["geneanet_result"]["families_count"], 1);
    assert_eq!(completed["geneanet_result"]["isolated_count"], 1);
    let isolated = &completed["geneanet_result"]["isolated_people"][0];
    assert_eq!(isolated["surname"], "BRANCH_C");
    assert_eq!(isolated["given_names"], "person_c");
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

    let (status, started) = send(
        &app,
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
    let (status, body) = send(
        &setup_app().await,
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

    let (status, completed) = send(
        &app,
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
    let media_root = std::env::temp_dir().join(format!(
        "oxidgene-test-async-export-{}",
        uuid::Uuid::now_v7()
    ));
    let state = AppState::new(db, &media_root);
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-export-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;

    let (status, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/export-jobs"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let job_id = started["job_id"].as_str().expect("job id");
    assert!(worker.run_once().await.expect("run export job"));

    let (status, completed) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(completed["phase"], "completed");
    let download_url = completed["download_url"].as_str().expect("download URL");
    let expires_at: chrono::DateTime<chrono::Utc> = completed["expires_at"]
        .as_str()
        .expect("expiry")
        .parse()
        .expect("RFC 3339 expiry");
    let remaining = expires_at - chrono::Utc::now();
    assert!(
        remaining > chrono::Duration::minutes(59) && remaining <= chrono::Duration::hours(1),
        "an hour after completion: {expires_at}"
    );

    let artifact = media_root.join("jobs").join(job_id);
    assert!(artifact.exists());

    // Downloaded any number of times within its hour: a save that went
    // wrong can be downloaded again.
    for _ in 0..2 {
        let response = app
            .clone()
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
    assert!(artifact.exists(), "a download keeps the artifact");
    let status_uri = format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}");
    let (_, after) = send(&app, Method::GET, &status_uri, None).await;
    assert_eq!(after["download_url"], download_url);
    assert_eq!(after["expires_at"], completed["expires_at"]);
    let _ = std::fs::remove_dir_all(&media_root);
}

/// Past its hour an export cannot be downloaded any more, even before the
/// maintenance pass has deleted its artifact; that pass then deletes it.
#[tokio::test]
async fn an_expired_export_is_refused_then_deleted() {
    use oxidgene_db::entities::background_job;
    use oxidgene_db::sea_orm::{ActiveModelTrait as _, ActiveValue::Set};

    let db = setup_db().await;
    let media_root = std::env::temp_dir().join(format!(
        "oxidgene-test-expired-export-{}",
        uuid::Uuid::now_v7()
    ));
    let state = AppState::new(db.clone(), &media_root);
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-expired-export-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;
    let (_, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/export-jobs"),
        None,
    )
    .await;
    let job_id = started["job_id"].as_str().expect("job id").to_string();
    assert!(worker.run_once().await.expect("run export job"));
    let status_uri = format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}");
    let (_, completed) = send(&app, Method::GET, &status_uri, None).await;
    let download_url = completed["download_url"].as_str().expect("download URL");

    // Completed an hour and a minute ago.
    background_job::ActiveModel {
        id: Set(job_id.parse().unwrap()),
        finished_at: Set(Some(chrono::Utc::now() - chrono::Duration::minutes(61))),
        ..Default::default()
    }
    .update(&db)
    .await
    .expect("backdate the export");
    let (status, expired) = send(&app, Method::GET, &status_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expired["phase"], "completed");
    assert!(expired["download_url"].is_null(), "{expired}");
    assert!(expired["expires_at"].is_null(), "{expired}");
    let (status, _) = send(&app, Method::GET, download_url, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let artifact = media_root.join("jobs").join(&job_id);
    assert!(artifact.exists(), "not swept yet");
    worker.maintain(chrono::Utc::now()).await;
    assert!(!artifact.exists(), "the maintenance pass deletes it");
    let _ = std::fs::remove_dir_all(&media_root);
}

/// What ended jobs leave behind is bounded: an export never downloaded
/// loses its artifact after an hour, an ended job its row after a day, and
/// objects under `jobs/` that no job needs go once a day old.
#[tokio::test]
async fn job_maintenance_bounds_what_ended_jobs_leave_behind() {
    let db = setup_db().await;
    let media_root = std::env::temp_dir().join(format!(
        "oxidgene-test-job-maintenance-{}",
        uuid::Uuid::now_v7()
    ));
    let state = AppState::new(db, &media_root);
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-maintenance-worker",
    );
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app).await;
    let (_, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/export-jobs"),
        None,
    )
    .await;
    let job_id = started["job_id"].as_str().expect("job id").to_string();
    assert!(worker.run_once().await.expect("run export job"));
    let artifact = media_root.join("jobs").join(&job_id);
    assert!(artifact.exists());

    // An object no job row points at: inputs stored by a crashed request.
    let orphan = uuid::Uuid::now_v7();
    let input = media_root.join("orphan-input");
    std::fs::write(&input, b"fixture").unwrap();
    state
        .media
        .put_file(&format!("jobs/{orphan}/source.ged"), &input)
        .await
        .unwrap();

    let now = chrono::Utc::now();
    worker.maintain(now).await;
    assert!(artifact.exists(), "a fresh artifact waits for its download");
    assert!(media_root.join("jobs").join(orphan.to_string()).exists());

    worker.maintain(now + chrono::Duration::hours(2)).await;
    assert!(!artifact.exists(), "an undownloaded artifact expires");
    let status_uri = format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}");
    let (status, body) = send(&app, Method::GET, &status_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["download_url"].is_null());
    assert!(media_root.join("jobs").join(orphan.to_string()).exists());

    worker.maintain(now + chrono::Duration::hours(25)).await;
    let (status, _) = send(&app, Method::GET, &status_uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "an ended job is pruned");
    assert!(!media_root.join("jobs").join(orphan.to_string()).exists());
    let _ = std::fs::remove_dir_all(&media_root);
}

/// Deleting a tree deletes its export artifacts, which live outside the
/// tree's own media prefix.
#[tokio::test]
async fn deleting_a_tree_deletes_its_export_artifacts() {
    let db = setup_db().await;
    let media_root = std::env::temp_dir().join(format!(
        "oxidgene-test-export-purge-{}",
        uuid::Uuid::now_v7()
    ));
    let state = AppState::new(db, &media_root);
    let worker = BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "rest-test-export-purge-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree_via_api(&app).await;
    let (_, started) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/export-jobs"),
        None,
    )
    .await;
    let job_id = started["job_id"].as_str().expect("job id").to_string();
    assert!(worker.run_once().await.expect("run export job"));
    let artifact = media_root.join("jobs").join(&job_id);
    assert!(artifact.exists());

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while artifact.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the purge deletes the tree's export artifact");
    let _ = std::fs::remove_dir_all(&media_root);
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

    let (_, running) = send(&app, Method::GET, "/api/v1/trees", None).await;
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
    let (_, completed) = send(&app, Method::GET, "/api/v1/trees", None).await;
    assert_eq!(completed["edges"][0]["node"]["import_in_progress"], false);
    assert!(completed["edges"][0]["node"]["import_job_id"].is_null());
}

#[tokio::test]
async fn test_gedcom_import_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let (status, _) = send_bytes(
        app.clone(),
        &format!("/api/v1/trees/{fake_id}/import-jobs?format=gedcom"),
        minimal_gedcom().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Imports are jobs: the synchronous format endpoints are gone, and their
/// paths are unknown routes rather than a second way in.
#[tokio::test]
async fn files_are_imported_only_through_jobs() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    for path in ["gedcom/import", "gedzip/import", "geneweb/import"] {
        let (status, body) = send_bytes(
            app.clone(),
            &format!("/api/v1/trees/{tree_id}/{path}"),
            minimal_gedcom().as_bytes().to_vec(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {body}");
        assert_eq!(body["error"], "not_found", "{path}");
    }
}

#[tokio::test]
async fn test_gedcom_export_empty_tree() {
    let app = setup_app().await;

    // Create tree
    let (_, tree_body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Empty Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Export (empty tree)
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/gedcom/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["gedcom"].as_str().unwrap().contains("HEAD"));
    assert!(body["warnings"].as_array().unwrap().is_empty());
}

/// The export's `SUBM` record carries the tree's "Who am I?" person, and
/// `Not Provided` once the tree names nobody.
#[tokio::test]
async fn the_gedcom_export_names_the_trees_own_person_as_submitter() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_named_person_via_api(&app, &tree_id, "female", "Ada", "Alpha").await;
    for (self_person_id, expected) in [
        (Some(person_id), "0 @SUBM1@ SUBM\n1 NAME Ada Alpha\n"),
        (None, "0 @SUBM1@ SUBM\n1 NAME Not Provided\n"),
    ] {
        let (status, _) = send(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}"),
            Some(serde_json::json!({ "self_person_id": self_person_id })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/gedcom/export"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let gedcom = body["gedcom"].as_str().expect("gedcom");
        assert!(gedcom.contains("\n1 SUBM @SUBM1@\n"), "{gedcom}");
        assert!(gedcom.contains(expected), "{gedcom}");
    }
}

#[tokio::test]
async fn test_gedcom_roundtrip() {
    let db = setup_db().await;
    let app = common::app_on(db.clone());

    // Create tree
    let (_, tree_body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(serde_json::json!({ "name": "Roundtrip Tree" })),
    )
    .await;
    let tree_id = tree_body["id"].as_str().unwrap();

    // Import
    let import_body = common::import_gedcom(&app, &db, tree_id, minimal_gedcom()).await;

    // Export
    let (status, export_body) = send(
        &app,
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

    let (status, _) = send(
        &app,
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

/// Helper: POST a raw binary body (an import job takes bytes, not JSON).
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
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = create_tree_via_api(&app).await;

    let job = common::import_job(
        &app,
        &common::worker_on(&db),
        &tree_id,
        "format=geneweb&filename=family.gw",
        minimal_geneweb().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(job["phase"], "completed", "{job}");
    let body = &job["result"];
    assert_eq!(body["persons_count"], 3);
    assert_eq!(body["families_count"], 1);

    // The entities really landed in the database.
    let (status, persons) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(persons["edges"].as_array().unwrap().len(), 3);
}

/// A `.gw` file is ISO-8859-1 unless it opts into UTF-8, so the import job
/// takes raw bytes; this is the regression test that nothing decodes them as
/// UTF-8 along the way.
#[tokio::test]
async fn test_geneweb_import_latin1_bytes() {
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = create_tree_via_api(&app).await;

    // "Émile" with É as the single Latin-1 byte 0xC9 — invalid UTF-8.
    let mut gw = Vec::new();
    gw.extend_from_slice(b"fam Doe \xC9mile.0 + Smith Jeanne.0\n");
    assert!(String::from_utf8(gw.clone()).is_err());

    let job = common::import_job(
        &app,
        &common::worker_on(&db),
        &tree_id,
        "format=geneweb&filename=latin1.gw",
        gw,
    )
    .await;
    assert_eq!(job["phase"], "completed", "{job}");
    assert_eq!(job["result"]["persons_count"], 2);

    // Search folds accents, so `emile` finds the person either way — what is
    // being asserted is the stored spelling: a lossy UTF-8 decode would have
    // left U+FFFD where the É is.
    let (_, found) = send(
        &app,
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
        &format!("/api/v1/trees/{fake_id}/import-jobs?format=geneweb"),
        minimal_geneweb().as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_geneweb_import_unparseable_file() {
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = create_tree_via_api(&app).await;

    let job = common::import_job(
        &app,
        &common::worker_on(&db),
        &tree_id,
        "format=geneweb",
        b"this is not a gw file at all\n".to_vec(),
    )
    .await;
    assert_eq!(job["phase"], "failed", "{job}");
    assert_eq!(job["error"], "invalid_job_input", "{job}");
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

    send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "GET profile failed: {body}");
    assert_eq!(body["person_id"], person_id);
    assert_eq!(body["primary_name"]["display_name"], "Jean Dupont");

    // Whole-tree listing.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "GET profiles failed: {body}");
    assert_eq!(body.as_array().unwrap().len(), 1);

    // `rebuild` must not be swallowed by the `{person_id}` route.
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/rebuild"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "tree rebuild failed: {body}");
    assert_eq!(body["persons_count"], 1);

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/rebuild/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "person rebuild failed: {body}");
    assert_eq!(body["persons_count"], 1);

    // Pedigree rooted on the only person.
    let (status, body) = send(
        &app,
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

    // The batched form answers for several roots at once, in request order,
    // and refuses a batch larger than the bound rather than truncating it.
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/pedigrees"),
        Some(serde_json::json!({
            "root_person_ids": [person_id, person_id],
            "ancestor_depth": 2,
            "descendant_depth": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "POST pedigrees failed: {body}");
    let entries = body.as_array().expect("array response");
    assert_eq!(entries.len(), 2, "{body}");
    assert_eq!(entries[0]["root_person_id"], person_id);
    assert_eq!(entries[0]["pedigree"]["ancestor_depth_loaded"], 2);

    let roots = (0..65)
        .map(|_| uuid::Uuid::now_v7().to_string())
        .collect::<Vec<_>>();
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/pedigrees"),
        Some(serde_json::json!({
            "root_person_ids": roots,
            "ancestor_depth": 2,
            "descendant_depth": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Expansion returns a (here empty) delta, not an error.
    let (status, body) = send(
        &app,
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
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/profiles/drop"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "drop failed: {body}");
    assert_eq!(body["dropped"], true);

    let (status, body) = send(
        &app,
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
        let (status, _) = send(&app, Method::GET, &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "still routed: {path}");
    }
}

// ───────────────────────── Statistics ─────────────────────────

#[tokio::test]
async fn statistics_count_the_tree_and_its_ages() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person_id = create_named_person_via_api(&app, &tree_id, "male", "Jean", "BRANCH_A").await;
    for (kind, date) in [("birth", "3 MAR 1820"), ("death", "3 MAR 1890")] {
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/events"),
            Some(serde_json::json!({
                "event_type": kind,
                "date_value": date,
                "person_id": person_id,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/statistics"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persons"], 1);
    assert_eq!(body["men"], 1);
    assert_eq!(body["top_surnames"][0]["label"], "BRANCH_A");
    // Filed by the year of death, as a sum and a count.
    let men = body["age_at_death"]["men"].as_array().unwrap();
    assert_eq!(men.len(), 1);
    assert_eq!(men[0]["year"], 1890);
    assert_eq!(men[0]["count"], 1);
    assert!((men[0]["sum"].as_f64().unwrap() - 70.0).abs() < 0.01);
    assert_eq!(body["births_by_month"][0]["year"], 1820);
    assert_eq!(body["births_by_month"][0]["counts"][2], 1);
    assert_eq!(body["longest_lives"][0]["age"], 70);
    assert_eq!(body["lifespan"]["men"]["mean"], 70.0);
    assert_eq!(body["event_types"][0]["count"], 1);
    assert_eq!(body["records"][0]["kind"], "longest_life_man");
    assert_eq!(body["records"][0]["persons"][0]["name"], "Jean BRANCH_A");

    // The options are checked and passed on.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/statistics?approximate=true&lang=fr"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persons"], 1);
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/statistics?lang=xx"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{}/statistics", uuid::Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ───────────────────────── Tools ─────────────────────────

async fn add_event_via_api(
    app: &axum::Router,
    tree_id: &str,
    owner: (&str, &str),
    kind: &str,
    date: &str,
) {
    let (key, id) = owner;
    let (status, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({ "event_type": kind, "date_value": date, key: id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// Creates an event and returns its id.
async fn event_id_via_api(
    app: &axum::Router,
    tree_id: &str,
    person_id: &str,
    kind: &str,
    date: &str,
) -> String {
    let (status, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({ "event_type": kind, "date_value": date, "person_id": person_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

/// A merge takes only what the user ticked of the duplicate: the events and
/// media links left out are dropped, the rest moves; and nobody else's
/// event can be left out.
#[tokio::test]
async fn a_merge_leaves_out_the_duplicates_items_not_taken() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let kept = create_named_person_via_api(&app, &tree_id, "female", "Anna", "BRANCH_A").await;
    let duplicate = create_named_person_via_api(&app, &tree_id, "female", "Anna", "BRANCH_A").await;
    let stranger = create_named_person_via_api(&app, &tree_id, "male", "Otto", "BRANCH_B").await;
    let kept_birth = event_id_via_api(&app, &tree_id, &kept, "birth", "12 MAR 1850").await;
    let duplicate_birth = event_id_via_api(&app, &tree_id, &duplicate, "birth", "1850").await;
    let residence = event_id_via_api(&app, &tree_id, &duplicate, "residence", "1880").await;
    let stranger_birth = event_id_via_api(&app, &tree_id, &stranger, "birth", "1851").await;
    let (status, document) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/document"),
        Some(serde_json::json!({ "title": "Scan A" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{document}");
    let (status, link) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media-links"),
        Some(serde_json::json!({ "media_id": document["id"], "person_id": duplicate, "sort_order": 0 })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{link}");
    let merge = |body: serde_json::Value| {
        let app = app.clone();
        let tree_id = tree_id.clone();
        let kept = kept.clone();
        async move {
            send(
                &app,
                Method::POST,
                &format!("/api/v1/trees/{tree_id}/persons/{kept}/merge"),
                Some(body),
            )
            .await
        }
    };

    // A third person's event is not the merge's to leave out.
    let (status, _) = merge(serde_json::json!({
        "duplicate_id": duplicate, "choices": { "left_out_events": [stranger_birth] }
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = merge(serde_json::json!({
        "duplicate_id": duplicate,
        "choices": {
            "left_out_events": [duplicate_birth],
            "left_out_media_links": [link["id"]],
        },
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, events) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?person_id={kept}"),
        None,
    )
    .await;
    let mut ids: Vec<&str> = events["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"]["id"].as_str().unwrap())
        .collect();
    ids.sort_unstable();
    let mut expected = vec![kept_birth.as_str(), residence.as_str()];
    expected.sort_unstable();
    assert_eq!(
        ids, expected,
        "the residence moved, the duplicate birth did not"
    );
    let (_, links) = send(
        &app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree_id}/media-links?media_id={}",
            document["id"].as_str().unwrap()
        ),
        None,
    )
    .await;
    assert_eq!(links, serde_json::json!([]), "the media was left out");
}

/// Generation by generation from the SOSA root: found ancestors with their
/// facts, missing parents listed, their own parents implied.
#[tokio::test]
async fn ancestry_completeness_walks_up_from_the_sosa_root() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;

    // Without a root there is nothing to walk.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/ancestry-completeness"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["root"].is_null());
    assert_eq!(body["generations"], serde_json::json!([]));

    let root = create_named_person_via_api(&app, &tree_id, "female", "Child", "BRANCH_A").await;
    let father = create_named_person_via_api(&app, &tree_id, "male", "Parent", "BRANCH_A").await;
    let family = create_family_via_api(&app, &tree_id, &[(&father, "husband")], &[&root]).await;
    add_event_via_api(&app, &tree_id, ("person_id", &father), "birth", "1900").await;
    add_event_via_api(&app, &tree_id, ("family_id", &family), "marriage", "1925").await;
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(serde_json::json!({ "sosa_root_person_id": root })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/ancestry-completeness?generations=3"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["root"]["person_id"], root.as_str());
    let generations = body["generations"].as_array().unwrap();
    assert_eq!(generations.len(), 3);
    let parents = &generations[1];
    assert_eq!(parents["expected"], 2);
    assert_eq!(parents["found"], 1);
    assert_eq!(parents["with_birth"], 1);
    assert_eq!(parents["with_union"], 1);
    assert_eq!(parents["entries"][0]["sosa"], 2);
    assert_eq!(
        parents["entries"][0]["person"]["person_id"],
        father.as_str()
    );
    assert_eq!(parents["entries"][0]["person"]["birth"]["value"], "1900");
    assert!(parents["entries"][1]["person"].is_null());
    let grandparents = &generations[2];
    assert_eq!(grandparents["entries"].as_array().unwrap().len(), 2);
    assert_eq!(grandparents["implied_missing"], 2);

    for bad in ["0", "16"] {
        let (status, _) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/ancestry-completeness?generations={bad}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "generations={bad}");
    }
    let (status, _) = send(
        &app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/ancestry-completeness",
            uuid::Uuid::now_v7()
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Anomalies come grouped by rule with the persons concerned; the places the
/// statistics cannot locate are listed with their usage.
#[tokio::test]
async fn anomalies_and_unlocated_places_are_listed() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let person = create_named_person_via_api(&app, &tree_id, "male", "Backwards", "BRANCH_A").await;
    add_event_via_api(&app, &tree_id, ("person_id", &person), "birth", "1850").await;
    add_event_via_api(&app, &tree_id, ("person_id", &person), "death", "1840").await;

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/anomalies"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["persons"], 1);
    let rule = body["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "death_before_birth")
        .expect("the death before the birth is found");
    assert_eq!(rule["category"], "dates");
    assert_eq!(rule["severity"], "error");
    assert_eq!(rule["count"], 1);
    assert_eq!(rule["items"][0]["persons"][0]["person_id"], person.as_str());

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/places"),
        Some(serde_json::json!({ "name": "Qzxv Nowhere Hamlet" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let place_id = body["id"].as_str().unwrap().to_string();
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(serde_json::json!({
            "event_type": "residence",
            "person_id": person,
            "place_id": place_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/unlocated-places"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body[0]["place_id"], place_id.as_str());
    assert_eq!(body[0]["name"], "Qzxv Nowhere Hamlet");
    assert_eq!(body[0]["count"], 1);

    for path in ["anomalies", "unlocated-places"] {
        let (status, _) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{}/{path}", uuid::Uuid::now_v7()),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

/// Two records sharing a name and a birth year are a pair until confirmed
/// to be two people.
#[tokio::test]
async fn potential_duplicates_are_listed_until_confirmed_distinct() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let first = create_named_person_via_api(&app, &tree_id, "female", "Anna", "BRANCH_A").await;
    let second = create_named_person_via_api(&app, &tree_id, "female", "Anna", "BRANCH_A").await;
    for person in [&first, &second] {
        add_event_via_api(&app, &tree_id, ("person_id", person), "birth", "1850").await;
    }
    let list = |app: axum::Router| {
        let tree_id = tree_id.clone();
        async move {
            let (status, body) = send(
                &app,
                Method::GET,
                &format!("/api/v1/trees/{tree_id}/duplicates"),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            body
        }
    };
    let body = list(app.clone()).await;
    assert_eq!(body["count"], 1);
    let pair = &body["pairs"][0];
    assert_eq!(pair["score"], 50);
    assert_eq!(
        pair["reasons"],
        serde_json::json!(["same_name", "same_birth_year"])
    );
    let ids = [
        pair["first"]["person_id"].as_str().unwrap(),
        pair["second"]["person_id"].as_str().unwrap(),
    ];
    assert!(ids.contains(&first.as_str()) && ids.contains(&second.as_str()));
    assert_eq!(pair["first"]["surname"], "BRANCH_A");
    // The full dates, which the search rows reduce to a year.
    assert_eq!(pair["first_dates"]["birth"]["value"], "1850");
    assert_eq!(pair["first_dates"]["birth"]["qualifier"], "exact");
    assert!(pair["second_dates"]["death"].is_null());

    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{first}/distinct"),
        Some(serde_json::json!({ "person_ids": [second] })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let body = list(app.clone()).await;
    assert_eq!(body["count"], 0);

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{}/duplicates", uuid::Uuid::now_v7()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The comparison's other choices: the duplicate's given names on the kept
/// surname, the duplicate's sex, and the duplicate's birth replacing the
/// kept one. The former primary name stays as a secondary name.
#[tokio::test]
async fn a_merge_takes_the_chosen_name_sex_and_events() {
    let app = setup_app().await;
    let tree_id = create_tree_via_api(&app).await;
    let kept = create_named_person_via_api(&app, &tree_id, "male", "Anna", "BRANCH_A").await;
    let duplicate =
        create_named_person_via_api(&app, &tree_id, "female", "Anna Maria", "BRANCH_B").await;
    let kept_birth = event_id_via_api(&app, &tree_id, &kept, "birth", "1850").await;
    let duplicate_birth =
        event_id_via_api(&app, &tree_id, &duplicate, "birth", "12 MAR 1850").await;

    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{kept}/merge"),
        Some(serde_json::json!({
            "duplicate_id": duplicate,
            "choices": {
                "left_out_events": [kept_birth],
                "given_names_from_duplicate": true,
                "sex_from_duplicate": true,
            },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sex"], "female");

    let (_, names) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{kept}/names"),
        None,
    )
    .await;
    let names: Vec<(bool, String)> = names
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            (
                n["is_primary"].as_bool().unwrap(),
                format!(
                    "{} {}",
                    n["given_names"].as_str().unwrap(),
                    n["surname"].as_str().unwrap()
                ),
            )
        })
        .collect();
    assert_eq!(
        names.iter().filter(|(primary, _)| *primary).count(),
        1,
        "{names:?}"
    );
    assert!(
        names.contains(&(true, "Anna Maria BRANCH_A".to_string())),
        "{names:?}"
    );
    assert!(
        names.contains(&(false, "Anna BRANCH_A".to_string())),
        "{names:?}"
    );
    assert!(
        names.contains(&(false, "Anna Maria BRANCH_B".to_string())),
        "{names:?}"
    );

    let (_, events) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?person_id={kept}"),
        None,
    )
    .await;
    let ids: Vec<&str> = events["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["node"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![duplicate_birth.as_str()],
        "the duplicate's birth replaced the kept one"
    );
}

/// The Playwright suite seeds its trees from `e2e/fixtures/family-blocks.ged`,
/// committed so that suite needs no Rust to run: it must stay the generated
/// `family_blocks_gedcom(3)`, thirty persons whose block 0 root is "Anchor".
/// After changing the generator, rewrite it with
/// `OXIDGENE_BLESS_E2E_FIXTURE=1 cargo nextest run -p oxidgene-api --test rest_test e2e_fixture`.
#[tokio::test]
async fn e2e_fixture_is_the_generated_family_blocks_tree() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../e2e/fixtures/family-blocks.ged");
    let generated = common::family_blocks_gedcom(3);
    if std::env::var_os("OXIDGENE_BLESS_E2E_FIXTURE").is_some() {
        std::fs::write(&path, &generated).expect("write the e2e fixture");
    }
    let committed = std::fs::read_to_string(&path).expect("read the e2e fixture");
    assert!(
        committed == generated,
        "e2e/fixtures/family-blocks.ged is stale: rewrite it with OXIDGENE_BLESS_E2E_FIXTURE=1"
    );

    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let (tree_id, _anchor) = common::family_blocks_tree(&app, &db, 3).await;
    let (_, profiles) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles"),
        None,
    )
    .await;
    assert_eq!(profiles.as_array().map(Vec::len), Some(30));
}

/// Once a tree is purged, nothing of it can be read back out of the SQLite
/// file: not from its freed pages, its write-ahead log, nor the full-text
/// index, which keeps a deleted row's words until its segments merge.
#[tokio::test]
async fn a_purged_tree_leaves_no_trace_in_the_database_file() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("fixture.db");
    let db = oxidgene_db::repo::connect(&format!("sqlite://{}?mode=rwc", file.display()))
        .await
        .unwrap();
    oxidgene_db::repo::run_migrations(&db).await.unwrap();
    let app = build_router(AppState::new(db, directory.path().join("media")));
    let tree_id = create_tree_via_api(&app).await;
    let (_, person) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(serde_json::json!({ "sex": "unknown" })),
    )
    .await;
    let person_id = person["id"].as_str().unwrap();
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Quintessa",
            "surname": "Zyxwvfixture",
            "is_primary": true,
        })),
    )
    .await;
    assert!(status.is_success());
    let traces = ["Zyxwvfixture", "zyxwvfixture", "Quintessa", "quintessa"];
    let readable = |path: &std::path::Path| {
        let bytes = std::fs::read(path).unwrap_or_default();
        traces.iter().any(|trace| {
            bytes
                .windows(trace.len())
                .any(|window| window == trace.as_bytes())
        })
    };
    let wal = directory.path().join("fixture.db-wal");
    assert!(readable(&file) || readable(&wal), "the name was written");

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while readable(&file) || readable(&wal) {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the purge erases the tree from the database file");
}

/// A purge gives the pages its tree occupied back to the file system, by an
/// incremental vacuum rather than a rewrite of the file: a database this
/// application creates is in incremental auto-vacuum mode.
#[tokio::test]
async fn a_purge_shrinks_the_database_file() {
    use oxidgene_db::sea_orm::{ConnectionTrait as _, DatabaseBackend, Statement};

    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("fixture.db");
    let db = oxidgene_db::repo::connect(&format!("sqlite://{}?mode=rwc", file.display()))
        .await
        .unwrap();
    oxidgene_db::repo::run_migrations(&db).await.unwrap();
    let auto_vacuum = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA auto_vacuum",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i32>("", "auto_vacuum")
        .unwrap();
    assert_eq!(auto_vacuum, 2, "incremental auto-vacuum");
    let checkpointed_size = || async {
        db.execute_unprepared("PRAGMA wal_checkpoint(TRUNCATE)")
            .await
            .unwrap();
        std::fs::metadata(&file).unwrap().len()
    };
    let empty = checkpointed_size().await;

    let app = common::app_on(db.clone());
    let (tree_id, _) = common::family_blocks_tree(&app, &db, 40).await;
    let loaded = checkpointed_size().await;
    assert!(loaded > empty + 1_000_000, "{empty} -> {loaded} bytes");

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The purge's own checkpoint applies the vacuum to the file.
    let target = empty + (loaded - empty) / 4;
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while std::fs::metadata(&file).unwrap().len() > target {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "the purge shrinks the file: {empty} empty, {loaded} loaded, {} now",
            std::fs::metadata(&file).unwrap().len()
        )
    });
}

/// On a SQLite file, a read is answered while a write holds the single
/// writer — an import holds it for its whole transaction, played here by a
/// transaction the test keeps open — and a write waits for its turn.
#[tokio::test]
async fn reads_are_answered_while_a_write_holds_the_writer() {
    use oxidgene_db::sea_orm::{ConnectionTrait as _, TransactionTrait as _};

    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(
        common::setup_file_db(directory.path()).await,
        directory.path().join("media"),
    );
    let app = build_router(state.clone());
    let tree_id = create_tree_via_api(&app).await;
    common::new_person(&app, &tree_id).await;

    let import = state.db.begin().await.unwrap();
    import
        .execute_unprepared("UPDATE tree SET name = name")
        .await
        .unwrap();

    let within = std::time::Duration::from_secs(5);
    for uri in [
        format!("/api/v1/trees/{tree_id}"),
        format!("/api/v1/trees/{tree_id}/persons"),
        format!("/api/v1/trees/{tree_id}/profiles"),
        format!("/api/v1/trees/{tree_id}/persons/search?q=a"),
    ] {
        let (status, _) = tokio::time::timeout(within, send(&app, Method::GET, &uri, None))
            .await
            .unwrap_or_else(|_| panic!("{uri} waited for the writer"));
        assert_eq!(status, StatusCode::OK, "{uri}");
    }

    let write = tokio::spawn({
        let app = app.clone();
        let uri = format!("/api/v1/trees/{tree_id}/persons");
        async move {
            send(
                &app,
                Method::POST,
                &uri,
                Some(serde_json::json!({ "sex": "unknown" })),
            )
            .await
            .0
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!write.is_finished(), "a write waits for the writer");
    import.commit().await.unwrap();
    let status = tokio::time::timeout(within, write)
        .await
        .expect("the write proceeds once the writer is free")
        .unwrap();
    assert_eq!(status, StatusCode::CREATED);
}
