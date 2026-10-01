//! REST and GraphQL answer the same request the same way.
//!
//! Each test drives one behaviour through both surfaces — the validation they
//! apply, the records they accept from another tree, the error they report —
//! so that a rule enforced on one and forgotten on the other fails here. All
//! data is fictitious.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::json;

use common::{gql, gql_error_code, send, setup_app};

// ── Errors ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_second_job_on_a_busy_tree_is_a_conflict() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Busy").await;

    let uri = format!("/api/v1/trees/{tree_id}/export-jobs");
    let (status, _) = send(&app, Method::POST, &uri, None).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    // No worker runs in the tests, so the first job stays queued and holds
    // the tree.
    let (status, body) = send(&app, Method::POST, &uri, None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "conflict");
    assert!(
        body.get("request_id").is_none(),
        "an expected outcome: {body}"
    );

    let response = gql(
        &app,
        "mutation($t: ID!) { startExportJob(treeId: $t) { jobId } }",
        json!({ "t": tree_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "CONFLICT");
}

#[tokio::test]
async fn reference_lookups_report_errors_alike() {
    let app = setup_app().await;

    // A sheet that exists.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/fr/given-names?term=Jean",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/fr/occupations?term=Laboureur",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A term without a sheet: a REST 404 in the envelope, a GraphQL null.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/fr/occupations?term=__unknown__",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found", "{body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/fr/given-names?term=__unknown__",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found", "{body}");
    let response = gql(
        &app,
        r#"{ occupationReference(language: "fr", term: "__unknown__") { label } }"#,
        json!({}),
    )
    .await;
    assert!(
        response["data"]["occupationReference"].is_null(),
        "{response}"
    );

    // An unknown language: a validation error on both.
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/xx/given-names?term=Jean",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error", "{body}");
    let (status, body) = send(
        &app,
        Method::GET,
        "/api/v1/reference/xx/places?q=paris",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error", "{body}");
    let response = gql(
        &app,
        r#"{ givenNameReference(language: "xx", term: "Jean") { label } }"#,
        json!({}),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR");
}

// ── Live trees ──────────────────────────────────────────────────────────

#[tokio::test]
async fn a_deleted_or_unknown_tree_is_not_found_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Gone").await;
    common::new_person(&app, &tree_id).await;
    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let unknown = uuid::Uuid::now_v7().to_string();

    for tree in [&tree_id, &unknown] {
        let (status, body) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/persons"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        let (status, _) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/persons"),
            Some(json!({ "sex": "female" })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Reads answer NOT_FOUND rather than the purge window's rows, or a
        // database error for a tree that never existed.
        let response = gql(
            &app,
            "query($t: ID!) { persons(treeId: $t) { totalCount } }",
            json!({ "t": tree }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
        let response = gql(
            &app,
            "query($t: ID!) { events(treeId: $t) { totalCount } }",
            json!({ "t": tree }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");

        // Writes are refused.
        let response = gql(
            &app,
            "mutation($t: ID!) { createPerson(treeId: $t, input: { sex: FEMALE }) { id } }",
            json!({ "t": tree }),
        )
        .await;
        assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
    }
}

#[tokio::test]
async fn a_malformed_identifier_is_a_validation_error_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Ids").await;

    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/not-a-uuid"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");

    let response = gql(
        &app,
        "query($t: ID!) { person(treeId: $t, id: \"not-a-uuid\") { id } }",
        json!({ "t": tree_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
}
