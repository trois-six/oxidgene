//! Shared media validation must protect both public API surfaces.

mod common;

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_core::{EventType, Sex};
use oxidgene_db::repo::{
    EventRepo, MediaRepo, PersonRepo, PlaceRepo, TreeRepo, UploadedMedia, VignetteRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::setup_db;

async fn write_vignette(
    app: &Router,
    graphql: bool,
    tree: Uuid,
    target: Uuid,
    create: bool,
    mut body: Value,
) -> (bool, Value) {
    let (method, uri) = if graphql {
        for (rest, gql) in [("person_id", "personId"), ("event_id", "eventId")] {
            if let Some(value) = body.as_object_mut().unwrap().remove(rest) {
                body[gql] = value;
            }
        }
        let query = if create {
            body["mediaId"] = json!(target);
            "mutation($tree: ID!, $input: CreateVignetteInput!) { createVignette(treeId: $tree, input: $input) { id } }"
        } else {
            "mutation($tree: ID!, $id: ID!, $input: UpdateVignetteInput!) { updateVignette(treeId: $tree, id: $id, input: $input) { id } }"
        };
        body = json!({"query": query, "variables": {"tree": tree, "id": target, "input": body}});
        (Method::POST, "/graphql".to_string())
    } else if create {
        (
            Method::POST,
            format!("/api/v1/trees/{tree}/media/{target}/vignettes"),
        )
    } else {
        (
            Method::PUT,
            format!("/api/v1/trees/{tree}/vignettes/{target}"),
        )
    };
    send_write(app, graphql, method, uri, &body).await
}

/// Sends a write and tells whether it was accepted: a GraphQL response
/// without errors, or a REST success. A REST refusal must be a validation or
/// a not-found error.
async fn send_write(
    app: &Router,
    graphql: bool,
    method: Method,
    uri: String,
    body: &Value,
) -> (bool, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    if graphql {
        assert_eq!(status, StatusCode::OK);
        (value.get("errors").is_none(), value)
    } else {
        assert!(
            status.is_success()
                || status == StatusCode::BAD_REQUEST
                || status == StatusCode::NOT_FOUND,
            "unexpected response: {value}"
        );
        (status.is_success(), value)
    }
}

/// An app over a fresh in-memory database holding one fictional tree. The
/// returned directory holds the media and must outlive the app.
async fn app_with_tree() -> (DatabaseConnection, Router, Uuid, tempfile::TempDir) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db.clone(), root.path()));
    let tree = Uuid::now_v7();
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    (db, app, tree, root)
}

/// A document with a single scanned page, returned as `(document, page)`.
async fn document_with_page(db: &DatabaseConnection, tree: Uuid) -> (Uuid, Uuid) {
    let document = Uuid::now_v7();
    MediaRepo::create_document(db, document, tree, None, chrono::Utc::now())
        .await
        .unwrap();
    let page = Uuid::now_v7();
    MediaRepo::create(
        db,
        page,
        tree,
        Some(document),
        "scan.png".into(),
        "image/png".into(),
        "scan.png".into(),
        0,
        None,
        None,
    )
    .await
    .unwrap();
    (document, page)
}

/// A crop is drawn on a page only, within bounds, and a patch cannot empty
/// its geometry. Returns the one crop accepted.
async fn check_crop_geometry(
    app: &Router,
    db: &DatabaseConnection,
    graphql: bool,
    tree: Uuid,
    (document, page): (Uuid, Uuid),
    rect: &Value,
) -> Uuid {
    let (accepted, response) = write_vignette(app, graphql, tree, page, true, rect.clone()).await;
    assert!(accepted, "valid page rejected: {response}");
    let crop = VignetteRepo::list_for_media(db, page)
        .await
        .unwrap()
        .remove(0);
    assert!(
        !write_vignette(app, graphql, tree, document, true, rect.clone())
            .await
            .0
    );
    assert!(
        VignetteRepo::list_for_media(db, document)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !write_vignette(
            app,
            graphql,
            tree,
            page,
            true,
            json!({"x": i32::MAX, "y": 0, "width": 1, "height": 1})
        )
        .await
        .0
    );
    assert!(
        !write_vignette(app, graphql, tree, crop.id, false, json!({"x": 1}))
            .await
            .0
    );
    crop.id
}

/// A person with a birth in `tree` and another in a second tree, returned
/// as `(people, events)`, the ones of `tree` first.
async fn attribution_targets(db: &DatabaseConnection, tree: Uuid) -> (Vec<Uuid>, Vec<Uuid>) {
    let mut people = Vec::new();
    let mut events = Vec::new();
    for target_tree in [tree, Uuid::now_v7()] {
        if target_tree != tree {
            TreeRepo::create(db, target_tree, "Other fictional tree".into(), None)
                .await
                .unwrap();
        }
        let person = Uuid::now_v7();
        PersonRepo::create(db, person, target_tree, Sex::Unknown)
            .await
            .unwrap();
        let event = Uuid::now_v7();
        EventRepo::create(
            db,
            event,
            target_tree,
            EventType::Birth,
            None,
            None,
            None,
            Some(person),
            None,
            None,
            Default::default(),
            None,
            Default::default(),
            None,
        )
        .await
        .unwrap();
        people.push(person);
        events.push(event);
    }
    (people, events)
}

/// A crop names a person or an event of its own tree only, on creation and
/// on update, and can be cleared of either.
async fn check_attribution(
    app: &Router,
    graphql: bool,
    tree: Uuid,
    (page, crop): (Uuid, Uuid),
    rect: &Value,
    (people, events): (&[Uuid], &[Uuid]),
) {
    for (field, ids) in [("person_id", people), ("event_id", events)] {
        let (accepted, response) =
            write_vignette(app, graphql, tree, crop, false, json!({field: ids[0]})).await;
        assert!(accepted, "same-tree attribution rejected: {response}");
        for id in [ids[1], Uuid::now_v7()] {
            let mut body = rect.clone();
            body[field] = json!(id);
            assert!(!write_vignette(app, graphql, tree, page, true, body).await.0);
            assert!(
                !write_vignette(app, graphql, tree, crop, false, json!({field: id}))
                    .await
                    .0
            );
        }
        assert!(
            write_vignette(app, graphql, tree, crop, false, json!({field: null}))
                .await
                .0
        );
    }
}

/// The crop, cleared of its person and event, is still the page's only one.
async fn check_cleared(db: &DatabaseConnection, page: Uuid, crop: Uuid) {
    let current = VignetteRepo::get(db, crop).await.unwrap();
    assert_eq!(current.person_id, None);
    assert_eq!(current.event_id, None);
    assert_eq!(
        VignetteRepo::list_for_media(db, page).await.unwrap().len(),
        1
    );
}

/// A crop cannot name a person or an event once deleted.
async fn check_deleted_targets(
    app: &Router,
    db: &DatabaseConnection,
    graphql: bool,
    tree: Uuid,
    crop: Uuid,
    (person, event): (Uuid, Uuid),
) {
    PersonRepo::delete(db, person).await.unwrap();
    EventRepo::delete(db, event).await.unwrap();
    for (field, id) in [("person_id", person), ("event_id", event)] {
        assert!(
            !write_vignette(app, graphql, tree, crop, false, json!({field: id}))
                .await
                .0
        );
    }
}

async fn vignette_validation(graphql: bool) {
    let (db, app, tree, _root) = app_with_tree().await;
    let (document, page) = document_with_page(&db, tree).await;
    let rect = json!({"x": 0, "y": 0, "width": 10, "height": 10});

    let crop = check_crop_geometry(&app, &db, graphql, tree, (document, page), &rect).await;

    let (people, events) = attribution_targets(&db, tree).await;
    check_attribution(&app, graphql, tree, (page, crop), &rect, (&people, &events)).await;
    check_cleared(&db, page, crop).await;
    check_deleted_targets(&app, &db, graphql, tree, crop, (people[0], events[0])).await;
    MediaRepo::delete(&db, document).await.unwrap();
    assert!(
        !write_vignette(&app, graphql, tree, page, true, rect)
            .await
            .0
    );
    assert!(
        !write_vignette(&app, graphql, tree, crop, false, json!({}))
            .await
            .0
    );
}

#[tokio::test]
async fn rest_vignette_validation() {
    vignette_validation(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_vignette_validation() {
    vignette_validation(true).await;
}

/// A document holding one uploaded page, returned as `(document, page)`.
async fn document_with_upload(db: &DatabaseConnection, tree: Uuid) -> (Uuid, Uuid) {
    let document = Uuid::now_v7();
    MediaRepo::create_document(db, document, tree, None, chrono::Utc::now())
        .await
        .unwrap();
    let page = Uuid::now_v7();
    MediaRepo::create_uploaded(
        db,
        page,
        tree,
        Some(document),
        UploadedMedia {
            file_name: "scan.png".into(),
            mime_type: "image/png".into(),
            storage_key: "test/scan.png".into(),
            sha256: "test-digest".into(),
            file_size: 1,
            thumbnail_key: None,
            width: Some(100),
            height: Some(100),
            page_count: 1,
            title: None,
            description: None,
            created_at: chrono::Utc::now(),
            metadata: Default::default(),
        },
    )
    .await
    .unwrap();
    (document, page)
}

/// A place of a second tree.
async fn foreign_place(db: &DatabaseConnection) -> Uuid {
    let foreign_tree = Uuid::now_v7();
    TreeRepo::create(db, foreign_tree, "Other fictional tree".into(), None)
        .await
        .unwrap();
    let foreign_place = Uuid::now_v7();
    PlaceRepo::create(
        db,
        foreign_place,
        foreign_tree,
        "Fictional place".into(),
        None,
        None,
    )
    .await
    .unwrap();
    foreign_place
}

/// Patches one field of a medium and checks it is `accepted` or refused.
async fn check_media_update(
    app: &Router,
    graphql: bool,
    tree: Uuid,
    id: Uuid,
    (field, value): (&str, Value),
    accepted: bool,
) {
    let (method, uri, body) = if graphql {
        let field = match field {
            "file_path" => "filePath",
            "mime_type" => "mimeType",
            "place_id" => "placeId",
            other => other,
        };
        (
            Method::POST,
            "/graphql".to_string(),
            json!({
                "query": "mutation($tree: ID!, $id: ID!, $input: UpdateMediaInput!) { updateMedia(treeId: $tree, id: $id, input: $input) { id } }",
                "variables": {"tree": tree, "id": id, "input": {field: value}}
            }),
        )
    } else {
        (
            Method::PUT,
            format!("/api/v1/trees/{tree}/media/{id}"),
            json!({field: value}),
        )
    };
    let (written, response) = send_write(app, graphql, method, uri, &body).await;
    assert_eq!(written, accepted, "{response}");
}

async fn media_update_validation(graphql: bool) {
    let (db, app, tree, _root) = app_with_tree().await;
    let (document, page) = document_with_upload(&db, tree).await;
    let foreign_place = foreign_place(&db).await;

    for id in [document, page] {
        for (field, value, accepted) in [
            ("title", json!("Fictional scan"), true),
            ("file_path", json!("https://example.org/other.png"), false),
            ("mime_type", json!("text/html"), false),
            ("place_id", json!(foreign_place), false),
            ("place_id", json!(Uuid::now_v7()), false),
            ("place_id", Value::Null, true),
        ] {
            check_media_update(&app, graphql, tree, id, (field, value), accepted).await;
        }
        assert!(MediaRepo::get(&db, id).await.unwrap().place_id.is_none());
    }
    assert_eq!(
        MediaRepo::get(&db, page).await.unwrap().mime_type,
        "image/png"
    );
    assert!(
        MediaRepo::get(&db, document)
            .await
            .unwrap()
            .storage_key
            .is_none()
    );
}

#[tokio::test]
async fn rest_media_update_validation() {
    media_update_validation(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_media_update_validation() {
    media_update_validation(true).await;
}
