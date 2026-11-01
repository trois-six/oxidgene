//! The reads of a restricted viewer leave hidden persons and their records
//! out whatever their number (docs/privacy.md §7): SQLite binds at most
//! 32,766 parameters in one statement and PostgreSQL 65,535, so a list that
//! named every hidden person in a `NOT IN (…)` failed past them.
//!
//! The fixture is a `site` tree whose hidden display mode is `hidden`, with
//! a few visible persons and more private ones than SQLite binds, each with
//! an event, a note and a citation. A Visitor reads every list, count and
//! search: each answers, and holds the visible persons and records alone.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use http_body_util::BodyExt as _;
use oxidgene_core::access::InstanceRole;
use oxidgene_core::sharing::TreeVisibility;
use oxidgene_db::entities::{citation, event, note, person, sea_enums};
use oxidgene_db::repo::{AppUserRepo, NewAppUser, TreeChanges, TreeRepo};
use oxidgene_db::sea_orm::{ActiveValue::Set, DatabaseConnection, EntityTrait};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use uuid::Uuid;

use crate::common::oidc::{MockProvider, TRUSTED_CLIENT};
use crate::common::{app_on, new_tree, ok, setup_db};

/// More hidden persons than SQLite binds parameters in one statement.
const HIDDEN: usize = 33_000;

/// Rows one insert writes: well within what either backend binds.
const BATCH: usize = 1_000;

/// The given name of the visible persons.
const VISIBLE: &str = "Brackenridge";

/// `HIDDEN` private persons of tree `tree`, each with a birth, a note and a
/// citation of `source`.
async fn hidden_persons(db: &DatabaseConnection, tree: Uuid, source: Uuid) {
    let now = Utc::now();
    let ids: Vec<Uuid> = (0..HIDDEN).map(|_| Uuid::now_v7()).collect();
    for chunk in ids.chunks(BATCH) {
        person::Entity::insert_many(chunk.iter().map(|&id| person::ActiveModel {
            id: Set(id),
            tree_id: Set(tree),
            sex: Set(sea_enums::Sex::Unknown),
            privacy: Set(sea_enums::Privacy::Private),
            portrait_media_id: Set(None),
            portrait_vignette_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        }))
        .exec(db)
        .await
        .expect("inserts the persons");
        event::Entity::insert_many(chunk.iter().map(|&id| event::ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree),
            event_type: Set(sea_enums::EventType::Birth),
            date_value: Set(None),
            date_value2: Set(None),
            date_sort: Set(None),
            date_qualifier: Set(sea_enums::DateQualifier::Exact),
            calendar: Set(sea_enums::Calendar::Gregorian),
            cause: Set(None),
            age: Set(None),
            agency: Set(None),
            classification: Set(None),
            place_id: Set(None),
            person_id: Set(Some(id)),
            family_id: Set(None),
            related_family_id: Set(None),
            description: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        }))
        .exec(db)
        .await
        .expect("inserts the events");
        note::Entity::insert_many(chunk.iter().map(|&id| note::ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree),
            text: Set("Fictitious hidden note".into()),
            person_id: Set(Some(id)),
            event_id: Set(None),
            family_id: Set(None),
            source_id: Set(None),
            media_id: Set(None),
            repository_id: Set(None),
            shared_group: Set(None),
            confidential: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        }))
        .exec(db)
        .await
        .expect("inserts the notes");
        citation::Entity::insert_many(chunk.iter().map(|&id| citation::ActiveModel {
            id: Set(Uuid::now_v7()),
            source_id: Set(source),
            person_id: Set(Some(id)),
            event_id: Set(None),
            family_id: Set(None),
            page: Set(None),
            confidence: Set(None),
            text: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }))
        .exec(db)
        .await
        .expect("inserts the citations");
    }
}

/// A visible person of `tree`: born in 1801, named, with a note and a
/// citation of `source`.
async fn visible_person(app: &axum::Router, tree: &str, source: &str) {
    let t = |path: &str| format!("/api/v1/trees/{tree}{path}");
    let person = ok(
        app,
        Method::POST,
        &t("/persons"),
        Some(json!({ "sex": "female", "privacy": "public" })),
    )
    .await;
    let id = person["id"].as_str().unwrap();
    ok(
        app,
        Method::POST,
        &t(&format!("/persons/{id}/names")),
        Some(json!({
            "name_type": "birth",
            "given_names": VISIBLE,
            "surname": "Fernhallow",
            "is_primary": true
        })),
    )
    .await;
    let birth = json!({ "event_type": "birth", "person_id": id, "date_value": "1801" });
    ok(app, Method::POST, &t("/events"), Some(birth)).await;
    let note = json!({ "text": "Fictitious visible note", "person_id": id });
    ok(app, Method::POST, &t("/notes"), Some(note)).await;
    let citation = json!({ "source_id": source, "person_id": id });
    ok(app, Method::POST, &t("/citations"), Some(citation)).await;
}

/// `body`'s `total_count`, else the length of its `edges`.
fn count(body: &Value) -> Option<u64> {
    body["total_count"]
        .as_u64()
        .or_else(|| body["edges"].as_array().map(|edges| edges.len() as u64))
}

#[tokio::test]
async fn a_visitor_reads_a_tree_hiding_more_persons_than_a_statement_binds() {
    let provider = MockProvider::start().await;
    let db = setup_db().await;
    let local = app_on(db.clone());
    let tree = new_tree(&local, "Bounds Fixture").await;
    let base = format!("/api/v1/trees/{tree}");
    ok(
        &local,
        Method::PUT,
        &base,
        Some(json!({ "hidden_display": "hidden" })),
    )
    .await;
    let source = ok(
        &local,
        Method::POST,
        &format!("{base}/sources"),
        Some(json!({ "title": "Fictitious parish register" })),
    )
    .await;
    let source = source["id"].as_str().unwrap().to_string();
    for _ in 0..2 {
        visible_person(&local, &tree, &source).await;
    }
    let tree_id: Uuid = tree.parse().unwrap();
    hidden_persons(&db, tree_id, source.parse().unwrap()).await;
    TreeRepo::update(
        &db,
        tree_id,
        TreeChanges {
            visibility: Some(TreeVisibility::Site),
            ..TreeChanges::default()
        },
    )
    .await
    .expect("opens the tree to the site");
    AppUserRepo::create(
        &db,
        NewAppUser {
            issuer: provider.issuer().to_string(),
            subject: "visitor".into(),
            email: Some("visitor@example.invalid".into()),
            email_verified: true,
            provider_name: Some("Fictive visitor".into()),
            username: Some("visitor".into()),
            locale: None,
            instance_role: InstanceRole::User,
            can_create_trees: false,
        },
    )
    .await
    .expect("creates the visitor");
    let token = provider.access_token("visitor", TRUSTED_CLIENT, 3600);
    let (app, _) = provider.app(&db, &[]);

    let read = |uri: String, body: Option<Value>| {
        let app = app.clone();
        let token = token.clone();
        async move {
            let method = if body.is_some() {
                Method::POST
            } else {
                Method::GET
            };
            let request = Request::builder()
                .method(method)
                .uri(&uri)
                .header(header::HOST, "genealogy.example.invalid")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            assert_eq!(status, StatusCode::OK, "{uri}: {text}");
            serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)
        }
    };

    // Each list holds the two visible persons, or their two records.
    for (path, expected) in [
        ("/persons?first=100", 2),
        ("/persons/search?q=Brackenridge", 2),
        ("/profiles?first=100", 2),
        ("/events?first=100", 2),
        ("/notes?first=100", 2),
        ("/citations?first=100", 2),
    ] {
        let body = read(format!("{base}{path}"), None).await;
        assert_eq!(count(&body), Some(expected), "{path}: {body}");
    }
    // The reads that only answer must answer.
    for path in [
        "/persons/recently-modified",
        "/places",
        "/sources",
        "/media",
        "/statistics/growth",
        "/statistics?lang=en",
        "/dictionary/family-names",
    ] {
        read(format!("{base}{path}"), None).await;
    }
    #[cfg(feature = "graphql")]
    for (field, query) in [
        ("persons", "persons(treeId: $t, first: 100) { totalCount }"),
        ("events", "events(treeId: $t, first: 100) { totalCount }"),
        ("notes", "notes(treeId: $t, first: 100) { totalCount }"),
        (
            "citations",
            "citations(treeId: $t, first: 100) { totalCount }",
        ),
    ] {
        let body = read(
            "/graphql".into(),
            Some(json!({
                "query": format!("query($t: ID!) {{ {query} }}"),
                "variables": { "t": tree },
            })),
        )
        .await;
        assert!(body.get("errors").is_none(), "{field}: {body}");
        assert_eq!(body["data"][field]["totalCount"], 2, "{field}: {body}");
    }
}
