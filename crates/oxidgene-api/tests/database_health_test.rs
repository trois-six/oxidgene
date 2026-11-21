//! What the API answers while the desktop's database file cannot be used
//! (docs/architecture.md §8.4). All data is fictitious.

use axum::http::{Method, StatusCode};
use oxidgene_api::backup::DatabaseHealth;
use oxidgene_api::{AppState, build_router};
use serde_json::json;

use crate::common::{new_person, new_tree, offline, ok, send, setup_db, test_media_root};

/// A file that failed its integrity check is still read — the `GET`
/// routes, a read sent as `POST`, an export — and no longer changed.
#[tokio::test]
async fn a_damaged_database_is_read_and_exported_but_not_changed() {
    let db = setup_db().await;
    let health = DatabaseHealth::default();
    let state = AppState::new(db, test_media_root()).with_database_health(health.clone());
    let app = build_router(offline(state));
    let tree = new_tree(&app, "Fictional tree").await;
    let person = new_person(&app, &tree).await;

    health.mark_damaged();

    let persons = format!("/api/v1/trees/{tree}/persons");
    for (method, uri, body) in [
        (Method::POST, persons.clone(), json!({ "sex": "unknown" })),
        (
            Method::PUT,
            format!("{persons}/{person}"),
            json!({ "sex": "female" }),
        ),
        (Method::DELETE, format!("{persons}/{person}"), json!(null)),
        (
            Method::POST,
            "/api/v1/trees".to_owned(),
            json!({ "name": "Another fictional tree" }),
        ),
    ] {
        let (status, answer) = send(&app, method.clone(), &uri, Some(body)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {uri}");
        assert_eq!(answer["error"], "database_damaged", "{method} {uri}");
        assert!(answer["request_id"].is_null(), "an expected answer");
    }

    // What only reads is answered: a listing, a read sent as POST, an
    // export.
    ok(&app, Method::GET, &persons, None).await;
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/pedigrees"),
        Some(json!({ "root_ids": [person], "ancestor_generations": 2 })),
    )
    .await;
    assert_ne!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "a read sent as POST"
    );
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/gedcom/export"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an export");
}

/// A file created with another schema answers nothing: every route says
/// what to do instead.
#[tokio::test]
async fn an_incompatible_database_answers_every_request_with_what_to_do() {
    let db = setup_db().await;
    let state =
        AppState::new(db, test_media_root()).with_database_health(DatabaseHealth::incompatible());
    let app = build_router(offline(state));
    for (method, uri) in [
        (Method::GET, "/api/v1/trees"),
        (Method::POST, "/api/v1/trees"),
    ] {
        let (status, answer) = send(
            &app,
            method.clone(),
            uri,
            Some(json!({ "name": "Fictional tree" })),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {uri}");
        assert_eq!(answer["error"], "database_incompatible", "{method} {uri}");
    }
}
