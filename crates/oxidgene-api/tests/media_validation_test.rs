//! Shared media validation must protect both public API surfaces.

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_core::{EventType, Sex};
use oxidgene_db::repo::{
    EventRepo, MediaRepo, PersonRepo, PlaceRepo, TreeRepo, UploadedMedia, VignetteRepo, connect,
    run_migrations,
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

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

async fn vignette_validation(graphql: bool) {
    let db = connect("sqlite::memory:").await.unwrap();
    run_migrations(&db).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db.clone(), root.path()));
    let tree = Uuid::now_v7();
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    let document = Uuid::now_v7();
    MediaRepo::create_document(&db, document, tree, None, chrono::Utc::now())
        .await
        .unwrap();
    let page = Uuid::now_v7();
    MediaRepo::create(
        &db,
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
    let rect = json!({"x": 0, "y": 0, "width": 10, "height": 10});

    let (accepted, response) = write_vignette(&app, graphql, tree, page, true, rect.clone()).await;
    assert!(accepted, "valid page rejected: {response}");
    let crop = VignetteRepo::list_for_media(&db, page)
        .await
        .unwrap()
        .remove(0);
    assert!(
        !write_vignette(&app, graphql, tree, document, true, rect.clone())
            .await
            .0
    );
    assert!(
        VignetteRepo::list_for_media(&db, document)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !write_vignette(
            &app,
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
        !write_vignette(&app, graphql, tree, crop.id, false, json!({"x": 1}))
            .await
            .0
    );

    let mut people = Vec::new();
    let mut events = Vec::new();
    for target_tree in [tree, Uuid::now_v7()] {
        if target_tree != tree {
            TreeRepo::create(&db, target_tree, "Other fictional tree".into(), None)
                .await
                .unwrap();
        }
        let person = Uuid::now_v7();
        PersonRepo::create(&db, person, target_tree, Sex::Unknown)
            .await
            .unwrap();
        let event = Uuid::now_v7();
        EventRepo::create(
            &db,
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
    for (field, ids) in [("person_id", &people), ("event_id", &events)] {
        let (accepted, response) =
            write_vignette(&app, graphql, tree, crop.id, false, json!({field: ids[0]})).await;
        assert!(accepted, "same-tree attribution rejected: {response}");
        for id in [ids[1], Uuid::now_v7()] {
            let mut body = rect.clone();
            body[field] = json!(id);
            assert!(
                !write_vignette(&app, graphql, tree, page, true, body)
                    .await
                    .0
            );
            assert!(
                !write_vignette(&app, graphql, tree, crop.id, false, json!({field: id}))
                    .await
                    .0
            );
        }
        assert!(
            write_vignette(&app, graphql, tree, crop.id, false, json!({field: null}))
                .await
                .0
        );
    }
    let current = VignetteRepo::get(&db, crop.id).await.unwrap();
    assert_eq!(current.person_id, None);
    assert_eq!(current.event_id, None);
    assert_eq!(
        VignetteRepo::list_for_media(&db, page).await.unwrap().len(),
        1
    );
    PersonRepo::delete(&db, people[0]).await.unwrap();
    EventRepo::delete(&db, events[0]).await.unwrap();
    for (field, id) in [("person_id", people[0]), ("event_id", events[0])] {
        assert!(
            !write_vignette(&app, graphql, tree, crop.id, false, json!({field: id}))
                .await
                .0
        );
    }
    MediaRepo::delete(&db, document).await.unwrap();
    assert!(
        !write_vignette(&app, graphql, tree, page, true, rect)
            .await
            .0
    );
    assert!(
        !write_vignette(&app, graphql, tree, crop.id, false, json!({}))
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

async fn media_update_validation(graphql: bool) {
    let db = connect("sqlite::memory:").await.unwrap();
    run_migrations(&db).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db.clone(), root.path()));
    let tree = Uuid::now_v7();
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    let document = Uuid::now_v7();
    MediaRepo::create_document(&db, document, tree, None, chrono::Utc::now())
        .await
        .unwrap();
    let page = Uuid::now_v7();
    MediaRepo::create_uploaded(
        &db,
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
    let foreign_tree = Uuid::now_v7();
    TreeRepo::create(&db, foreign_tree, "Other fictional tree".into(), None)
        .await
        .unwrap();
    let foreign_place = Uuid::now_v7();
    PlaceRepo::create(
        &db,
        foreign_place,
        foreign_tree,
        "Fictional place".into(),
        None,
        None,
    )
    .await
    .unwrap();

    for id in [document, page] {
        for (field, value, accepted) in [
            ("title", json!("Fictional scan"), true),
            ("file_path", json!("https://example.org/other.png"), false),
            ("mime_type", json!("text/html"), false),
            ("place_id", json!(foreign_place), false),
            ("place_id", json!(Uuid::now_v7()), false),
            ("place_id", Value::Null, true),
        ] {
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
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let response: Value = serde_json::from_slice(&bytes).unwrap();
            if graphql {
                assert_eq!(status, StatusCode::OK);
                assert_eq!(response.get("errors").is_none(), accepted, "{response}");
            } else {
                assert_eq!(status.is_success(), accepted, "{response}");
                assert!(
                    status.is_success()
                        || status == StatusCode::BAD_REQUEST
                        || status == StatusCode::NOT_FOUND
                );
            }
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
