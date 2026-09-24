//! Integration tests for the MCP server.
//!
//! Each test seeds an in-memory SQLite database through the REST router, then
//! drives the MCP server over an in-process duplex transport with the SDK's
//! own client — no external process or client is involved.

#![cfg(feature = "mcp")]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use rmcp::RoleClient;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt as _};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt as _;

struct Harness {
    rest: axum::Router,
    client: RunningService<RoleClient, ()>,
}

async fn setup() -> Harness {
    let db: DatabaseConnection = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    let rest = build_router(AppState::new(
        db.clone(),
        std::env::temp_dir().join("oxidgene-test-media"),
    ));

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    tokio::spawn(oxidgene_api::mcp::serve(db, server_transport));
    let client = ().serve(client_transport).await.expect("MCP handshake");
    Harness { rest, client }
}

impl Harness {
    async fn rest(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let body = body.map_or_else(Body::empty, |json| {
            Body::from(serde_json::to_vec(&json).unwrap())
        });
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body)
            .unwrap();
        let response = self.rest.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn created(&self, uri: &str, body: Value) -> String {
        let (status, body) = self.rest(Method::POST, uri, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{uri}: {body}");
        body["id"].as_str().unwrap().to_string()
    }

    async fn tree(&self, name: &str) -> String {
        self.created("/api/v1/trees", json!({ "name": name })).await
    }

    async fn person(&self, tree_id: &str, sex: &str, given_names: &str, surname: &str) -> String {
        let person_id = self
            .created(
                &format!("/api/v1/trees/{tree_id}/persons"),
                json!({ "sex": sex }),
            )
            .await;
        self.created(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
            json!({
                "name_type": "birth",
                "given_names": given_names,
                "surname": surname,
                "is_primary": true
            }),
        )
        .await;
        person_id
    }

    /// Call a tool and return `(is_error, structured content)`.
    async fn call(&self, tool: &'static str, arguments: Value) -> (bool, Value) {
        let Value::Object(arguments) = arguments else {
            panic!("tool arguments are an object");
        };
        let result = self
            .client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .unwrap_or_else(|error| panic!("{tool}: {error}"));
        (
            result.is_error.unwrap_or(false),
            result.structured_content.unwrap_or(Value::Null),
        )
    }

    async fn ok(&self, tool: &'static str, arguments: Value) -> Value {
        let (is_error, content) = self.call(tool, arguments).await;
        assert!(!is_error, "{tool} failed: {content}");
        content
    }

    async fn error_code(&self, tool: &'static str, arguments: Value) -> String {
        let (is_error, content) = self.call(tool, arguments).await;
        assert!(is_error, "{tool} should have failed: {content}");
        content["error"].as_str().unwrap_or_default().to_string()
    }
}

#[tokio::test]
async fn every_tool_is_read_only_and_every_tool_but_list_trees_requires_a_tree() {
    let harness = setup().await;
    let tools = harness.client.list_all_tools().await.unwrap();
    assert!(tools.len() > 10);

    for tool in &tools {
        let annotations = tool.annotations.as_ref().expect("annotated");
        assert_eq!(annotations.read_only_hint, Some(true), "{}", tool.name);
        assert_eq!(annotations.open_world_hint, Some(false), "{}", tool.name);

        let required = tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let requires_tree = required.contains(&json!("tree_id"));
        assert_eq!(requires_tree, tool.name != "list_trees", "{}", tool.name);
    }
}

#[tokio::test]
async fn a_call_without_a_tree_is_refused() {
    let harness = setup().await;
    let (is_error, _) = harness
        .call("search_persons", json!({ "q": "sample" }))
        .await;
    assert!(is_error);
}

#[tokio::test]
async fn tree_scoped_reads_follow_the_rest_representation() {
    let harness = setup().await;
    let tree_id = harness.tree("Sample tree").await;
    let father = harness.person(&tree_id, "male", "Alpha", "Sample").await;
    let mother = harness.person(&tree_id, "female", "Beta", "Example").await;
    let child = harness.person(&tree_id, "female", "Gamma", "Sample").await;
    let family_id = harness
        .created(&format!("/api/v1/trees/{tree_id}/families"), json!({}))
        .await;
    for (person_id, role) in [(&father, "husband"), (&mother, "wife")] {
        harness
            .created(
                &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
                json!({ "person_id": person_id, "role": role, "sort_order": 0 }),
            )
            .await;
    }
    harness
        .created(
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
            json!({ "person_id": child, "child_type": "biological", "sort_order": 0 }),
        )
        .await;
    let place_id = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/places"),
            json!({ "name": "Sampleville" }),
        )
        .await;
    harness
        .created(
            &format!("/api/v1/trees/{tree_id}/events"),
            json!({
                "event_type": "birth",
                "date_value": "1850",
                "date_qualifier": "about",
                "person_id": child,
                "place_id": place_id
            }),
        )
        .await;
    let source_id = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/sources"),
            json!({ "title": "Sample register" }),
        )
        .await;
    harness
        .created(
            &format!("/api/v1/trees/{tree_id}/citations"),
            json!({ "source_id": source_id, "person_id": child, "confidence": "high" }),
        )
        .await;

    let trees = harness.ok("list_trees", json!({})).await;
    assert_eq!(trees["edges"][0]["node"]["id"], tree_id);
    assert_eq!(
        harness.ok("get_tree", json!({ "tree_id": tree_id })).await["name"],
        "Sample tree"
    );

    let found = harness
        .ok(
            "search_persons",
            json!({ "tree_id": tree_id, "surname": "sample", "sex": "female", "sort": "name_asc" }),
        )
        .await;
    assert_eq!(found["total_count"], 1);
    assert_eq!(found["entries"][0]["person_id"], child);
    assert_eq!(found["entries"][0]["father_name"], "Alpha Sample");

    let profile = harness
        .ok(
            "get_person_profile",
            json!({ "tree_id": tree_id, "person_id": child }),
        )
        .await;
    assert_eq!(profile["birth"]["date_qualifier"], "about");
    assert_eq!(profile["family_as_child"]["mother_id"], mother);

    let pedigree = harness
        .ok(
            "get_pedigree",
            json!({ "tree_id": tree_id, "root_person_id": child, "ancestor_depth": 1, "descendant_depth": 0 }),
        )
        .await;
    assert_eq!(pedigree["persons"].as_object().unwrap().len(), 3);

    let labels = harness
        .ok(
            "get_relation_labels",
            json!({ "tree_id": tree_id, "family_ids": [family_id] }),
        )
        .await;
    assert_eq!(labels["spouses"].as_array().unwrap().len(), 2);

    let events = harness
        .ok(
            "list_events",
            json!({ "tree_id": tree_id, "person_id": child }),
        )
        .await;
    assert_eq!(events["total_count"], 1);
    assert_eq!(
        harness
            .ok(
                "get_place",
                json!({ "tree_id": tree_id, "place_id": place_id })
            )
            .await["name"],
        "Sampleville"
    );
    assert_eq!(
        harness
            .ok(
                "get_source",
                json!({ "tree_id": tree_id, "source_id": source_id })
            )
            .await["title"],
        "Sample register"
    );
    let citations = harness
        .ok(
            "list_citations",
            json!({ "tree_id": tree_id, "source_id": source_id }),
        )
        .await;
    assert_eq!(citations["total_count"], 1);
    let notes = harness
        .ok(
            "list_notes",
            json!({ "tree_id": tree_id, "person_id": child }),
        )
        .await;
    assert_eq!(notes["total_count"], 0);

    // A list is wrapped in an object, as MCP requires of structured content.
    let names = harness
        .ok(
            "list_dictionary",
            json!({ "tree_id": tree_id, "kind": "family_names" }),
        )
        .await;
    assert_eq!(names["items"][1]["value"], "Sample");
    assert_eq!(names["items"][1]["count"], 2);
    let usage = harness
        .ok(
            "dictionary_usage",
            json!({ "tree_id": tree_id, "kind": "places", "id": place_id }),
        )
        .await;
    assert_eq!(usage["items"][0]["person_id"], child);
    assert_eq!(
        harness
            .error_code(
                "dictionary_usage",
                json!({ "tree_id": tree_id, "kind": "family_names" }),
            )
            .await,
        "validation_error"
    );

    // The SOSA root is unset, so no number resolves.
    assert_eq!(
        harness
            .error_code(
                "get_person_by_sosa",
                json!({ "tree_id": tree_id, "number": 1 })
            )
            .await,
        "not_found"
    );
    assert_eq!(
        harness
            .error_code(
                "get_pedigree",
                json!({ "tree_id": tree_id, "root_person_id": child, "ancestor_depth": 11, "descendant_depth": 0 }),
            )
            .await,
        "validation_error"
    );
}

#[tokio::test]
async fn an_id_from_another_tree_is_not_found() {
    let harness = setup().await;
    let tree_id = harness.tree("Scope tree").await;
    let other_tree_id = harness.tree("Other tree").await;
    let person_id = harness
        .person(&other_tree_id, "female", "Delta", "Sample")
        .await;
    let place_id = harness
        .created(
            &format!("/api/v1/trees/{other_tree_id}/places"),
            json!({ "name": "Sampleville" }),
        )
        .await;
    let source_id = harness
        .created(
            &format!("/api/v1/trees/{other_tree_id}/sources"),
            json!({ "title": "Sample register" }),
        )
        .await;

    for (tool, arguments) in [
        (
            "get_person_profile",
            json!({ "tree_id": tree_id, "person_id": person_id }),
        ),
        (
            "get_pedigree",
            json!({ "tree_id": tree_id, "root_person_id": person_id, "ancestor_depth": 1, "descendant_depth": 1 }),
        ),
        (
            "get_place",
            json!({ "tree_id": tree_id, "place_id": place_id }),
        ),
        (
            "get_source",
            json!({ "tree_id": tree_id, "source_id": source_id }),
        ),
        (
            "list_citations",
            json!({ "tree_id": tree_id, "source_id": source_id }),
        ),
        (
            "dictionary_usage",
            json!({ "tree_id": tree_id, "kind": "sources", "id": source_id }),
        ),
    ] {
        assert_eq!(
            harness.error_code(tool, arguments).await,
            "not_found",
            "{tool}"
        );
    }

    // Nothing from the other tree leaked into this one's search either.
    let found = harness
        .ok(
            "search_persons",
            json!({ "tree_id": tree_id, "q": "delta" }),
        )
        .await;
    assert_eq!(found["total_count"], 0);
}

#[tokio::test]
async fn a_tree_deleted_mid_session_disappears() {
    let harness = setup().await;
    let tree_id = harness.tree("Doomed tree").await;
    assert_eq!(harness.ok("list_trees", json!({})).await["total_count"], 1);

    let (status, _) = harness
        .rest(Method::DELETE, &format!("/api/v1/trees/{tree_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert_eq!(harness.ok("list_trees", json!({})).await["total_count"], 0);
    for (tool, arguments) in [
        ("get_tree", json!({ "tree_id": tree_id })),
        ("search_persons", json!({ "tree_id": tree_id })),
        (
            "list_dictionary",
            json!({ "tree_id": tree_id, "kind": "occupations" }),
        ),
    ] {
        assert_eq!(
            harness.error_code(tool, arguments).await,
            "not_found",
            "{tool}"
        );
    }
}
