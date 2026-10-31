//! Integration tests for the MCP server.
//!
//! Each test seeds an in-memory SQLite database through the REST router, then
//! drives the MCP server over an in-process duplex transport with the SDK's
//! own client — no external process or client is involved.

#![cfg(feature = "mcp")]

use crate::common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::mcp::Access;
use rmcp::RoleClient;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceExt as _};
use serde_json::{Value, json};
use tower::ServiceExt as _;

use common::{app_on, setup_db};

struct Harness {
    rest: axum::Router,
    client: RunningService<RoleClient, ()>,
}

/// A read-only session, as `oxidgene-desktop mcp` starts by default.
async fn setup() -> Harness {
    setup_with(Access::ReadOnly).await
}

/// A session that may write, as `oxidgene-desktop mcp --allow-writes` starts.
async fn setup_writes() -> Harness {
    setup_with(Access::ReadWrite).await
}

async fn setup_with(access: Access) -> Harness {
    let db = setup_db().await;
    let rest = app_on(db.clone());

    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    tokio::spawn(oxidgene_api::mcp::serve(db, access, server_transport));
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
    let repository_id = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/repositories"),
            json!({ "name": "Sample archives" }),
        )
        .await;
    harness
        .created(
            &format!("/api/v1/trees/{tree_id}/sources/{source_id}/repositories"),
            json!({ "repository_id": repository_id, "call_number": "E 1" }),
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
    let source = harness
        .ok(
            "get_source",
            json!({ "tree_id": tree_id, "source_id": source_id }),
        )
        .await;
    assert_eq!(source["title"], "Sample register");
    assert_eq!(
        source["repositories"][0]["repository_name"],
        "Sample archives"
    );
    assert_eq!(source["repositories"][0]["call_number"], "E 1");
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

#[tokio::test]
async fn tags_are_listed_and_filter_the_person_search() {
    let harness = setup().await;
    let tree_id = harness.tree("Tag tree").await;
    let ledger = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/tags"),
            json!({ "label": "Ledger" }),
        )
        .await;
    let tagged = harness.person(&tree_id, "male", "Alpha", "Sample").await;
    harness.person(&tree_id, "female", "Beta", "Sample").await;
    let (status, body) = harness
        .rest(
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}/tags/{ledger}/records/person/{tagged}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let tags = harness.ok("list_tags", json!({ "tree_id": tree_id })).await;
    assert_eq!(tags["items"][0]["id"], ledger);
    assert_eq!(tags["items"][0]["label"], "Ledger");
    assert_eq!(tags["items"][0]["counts"]["persons"], 1);

    let found = harness
        .ok(
            "search_persons",
            json!({ "tree_id": tree_id, "surname": "sample", "tags": [ledger] }),
        )
        .await;
    assert_eq!(found["total_count"], 1);
    assert_eq!(found["entries"][0]["person_id"], tagged);
}

#[tokio::test]
async fn place_and_note_reads_carry_their_tags() {
    let harness = setup().await;
    let tree_id = harness.tree("Tagged place tree").await;
    // Created in this order, read back sorted by folded label.
    let bravo = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/tags"),
            json!({ "label": "Bravo" }),
        )
        .await;
    let alpha = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/tags"),
            json!({ "label": "alpha" }),
        )
        .await;
    let person = harness.person(&tree_id, "male", "Piers", "Hollowell").await;
    let place = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/places"),
            json!({ "name": "Harbour Town" }),
        )
        .await;
    let plain_place = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/places"),
            json!({ "name": "Millbrook" }),
        )
        .await;
    let note = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/notes"),
            json!({ "text": "Remark", "person_id": person }),
        )
        .await;
    let plain_note = harness
        .created(
            &format!("/api/v1/trees/{tree_id}/notes"),
            json!({ "text": "Plain", "person_id": person }),
        )
        .await;
    for (tag, kind, id) in [
        (&bravo, "place", &place),
        (&alpha, "place", &place),
        (&alpha, "note", &note),
    ] {
        let (status, body) = harness
            .rest(
                Method::PUT,
                &format!("/api/v1/trees/{tree_id}/tags/{tag}/records/{kind}/{id}"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let labels = |tags: &Value| -> Vec<String> {
        tags.as_array()
            .expect("an array of tags")
            .iter()
            .map(|tag| tag["label"].as_str().unwrap().to_owned())
            .collect()
    };

    let tagged = harness
        .ok(
            "get_place",
            json!({ "tree_id": tree_id, "place_id": place }),
        )
        .await;
    assert_eq!(labels(&tagged["tags"]), ["alpha", "Bravo"]);
    let untagged = harness
        .ok(
            "get_place",
            json!({ "tree_id": tree_id, "place_id": plain_place }),
        )
        .await;
    assert_eq!(untagged["tags"], json!([]));

    let notes = harness
        .ok(
            "list_notes",
            json!({ "tree_id": tree_id, "person_id": person }),
        )
        .await;
    let node = |id: &str| {
        notes["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| &edge["node"])
            .find(|node| node["id"] == id)
            .unwrap_or_else(|| panic!("no note {id}"))
            .clone()
    };
    assert_eq!(labels(&node(&note)["tags"]), ["alpha"]);
    assert_eq!(node(&plain_note)["tags"], json!([]));
}

// ── Write tools ──────────────────────────────────────────────────────────────

/// The write tools that delete a record or remove a link.
const DESTRUCTIVE_TOOLS: [&str; 13] = [
    "untag_record",
    "remove_event_witness",
    "remove_spouse",
    "remove_child",
    "delete_person_name",
    "delete_event",
    "delete_citation",
    "delete_note",
    "delete_tag",
    "delete_source",
    "delete_place",
    "delete_family",
    "delete_person",
];

/// The write tools that only add or change.
const ADDITIVE_WRITE_TOOLS: [&str; 21] = [
    "create_person",
    "update_person",
    "add_person_name",
    "update_person_name",
    "create_event",
    "update_event",
    "add_event_witness",
    "create_family",
    "update_family",
    "add_spouse",
    "add_child",
    "create_place",
    "update_place",
    "create_source",
    "update_source",
    "create_citation",
    "update_citation",
    "create_note",
    "update_note",
    "create_tag",
    "tag_record",
];

impl Harness {
    /// The tree's audit log, newest first, as REST lists it.
    async fn audit(&self, tree_id: &str) -> Vec<Value> {
        let (status, body) = self
            .rest(
                Method::GET,
                &format!("/api/v1/trees/{tree_id}/audit?first=100"),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| edge["node"].clone())
            .collect()
    }

    /// Whether the audit log holds an entry of `action` on `entity`.
    async fn recorded(&self, tree_id: &str, expected: &[(&str, &str)]) {
        let audit = self.audit(tree_id).await;
        for (entity, action) in expected {
            assert!(
                audit
                    .iter()
                    .any(|entry| entry["entity"] == *entity && entry["action"] == *action),
                "{entity} {action} not recorded"
            );
        }
    }

    async fn profile(&self, tree_id: &str, person_id: &str) -> Value {
        self.ok(
            "get_person_profile",
            json!({ "tree_id": tree_id, "person_id": person_id }),
        )
        .await
    }

    async fn id_of(&self, tool: &'static str, arguments: Value) -> String {
        self.ok(tool, arguments).await["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{tool} returned no id"))
            .to_string()
    }

    fn instructions(&self) -> String {
        self.client
            .peer_info()
            .and_then(|info| info.instructions.clone())
            .expect("the server sends instructions")
    }
}

#[tokio::test]
async fn a_read_only_session_offers_no_write_tool() {
    let harness = setup().await;
    let tools = harness.client.list_all_tools().await.unwrap();
    for name in DESTRUCTIVE_TOOLS.iter().chain(&ADDITIVE_WRITE_TOOLS) {
        assert!(
            tools.iter().all(|tool| tool.name != *name),
            "{name} is listed in a read-only session"
        );
    }
    // Not listed, and not callable either: an unknown tool is a protocol error.
    let tree_id = harness.tree("Locked tree").await;
    let Value::Object(arguments) = json!({ "tree_id": tree_id, "sex": "female" }) else {
        unreachable!()
    };
    assert!(
        harness
            .client
            .call_tool(CallToolRequestParams::new("create_person").with_arguments(arguments))
            .await
            .is_err()
    );
    assert_eq!(
        harness
            .ok("search_persons", json!({ "tree_id": tree_id }))
            .await["total_count"],
        0
    );
    assert!(harness.instructions().contains("read-only"));
}

#[tokio::test]
async fn a_write_session_annotates_every_tool() {
    let harness = setup_writes().await;
    let tools = harness.client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    for name in DESTRUCTIVE_TOOLS.iter().chain(&ADDITIVE_WRITE_TOOLS) {
        assert!(names.contains(name), "{name} is missing");
    }

    for tool in &tools {
        let name = tool.name.as_ref();
        let annotations = tool.annotations.as_ref().expect("annotated");
        assert_eq!(annotations.open_world_hint, Some(false), "{name}");
        let destructive = DESTRUCTIVE_TOOLS.contains(&name);
        let writes = destructive || ADDITIVE_WRITE_TOOLS.contains(&name);
        assert_eq!(annotations.read_only_hint, Some(!writes), "{name}");
        if writes {
            assert_eq!(annotations.destructive_hint, Some(destructive), "{name}");
            assert!(annotations.idempotent_hint.is_some(), "{name}");
        } else {
            assert_eq!(annotations.idempotent_hint, Some(true), "{name}");
        }
        if destructive {
            assert_eq!(annotations.idempotent_hint, Some(true), "{name}");
        }

        let required = tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            required.contains(&json!("tree_id")),
            name != "list_trees",
            "{name}"
        );
        // An update requires the IDs naming its record, never a field: an
        // absent field is kept.
        if name.starts_with("update_") {
            assert!(
                required
                    .iter()
                    .all(|field| field.as_str().is_some_and(|f| f.ends_with("_id"))),
                "{name}: {required:?}"
            );
        }
    }

    let instructions = harness.instructions();
    assert!(!instructions.contains("read-only"));
    assert!(instructions.contains("history"));
}

#[tokio::test]
async fn person_writes_refresh_the_profile_and_the_history() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Write tree").await;

    let person = harness
        .ok(
            "create_person",
            json!({ "tree_id": tree_id, "sex": "unknown" }),
        )
        .await;
    assert_eq!(person["sex"], "unknown");
    let person_id = person["id"].as_str().unwrap().to_string();

    let name = harness
        .ok(
            "add_person_name",
            json!({
                "tree_id": tree_id,
                "person_id": person_id,
                "name_type": "birth",
                "given_names": "Alpha",
                "surname": "Sample",
                "nickname": "Al",
                "is_primary": true
            }),
        )
        .await;
    assert_eq!(name["surname"], "Sample");
    let name_id = name["id"].as_str().unwrap().to_string();

    let updated = harness
        .ok(
            "update_person",
            json!({ "tree_id": tree_id, "person_id": person_id, "sex": "female", "privacy": "private" }),
        )
        .await;
    assert_eq!(updated["sex"], "female");
    assert_eq!(updated["privacy"], "private");

    // A null clears an optional field; an absent one is kept.
    let renamed = harness
        .ok(
            "update_person_name",
            json!({
                "tree_id": tree_id,
                "person_id": person_id,
                "name_id": name_id,
                "given_names": "Beta",
                "nickname": null
            }),
        )
        .await;
    assert_eq!(renamed["given_names"], "Beta");
    assert_eq!(renamed["surname"], "Sample");
    assert_eq!(renamed["nickname"], Value::Null);

    // The profile and the search index show the writes.
    let profile = harness.profile(&tree_id, &person_id).await;
    assert_eq!(profile["sex"], "female");
    assert_eq!(profile["primary_name"]["given_names"], "Beta");
    let found = harness
        .ok("search_persons", json!({ "tree_id": tree_id, "q": "beta" }))
        .await;
    assert_eq!(found["total_count"], 1);

    // Recorded in the history like a write from any other surface.
    harness
        .recorded(
            &tree_id,
            &[
                ("person", "create"),
                ("person", "update"),
                ("person_name", "create"),
                ("person_name", "update"),
            ],
        )
        .await;

    harness
        .ok(
            "delete_person_name",
            json!({ "tree_id": tree_id, "person_id": person_id, "name_id": name_id }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &person_id).await["primary_name"],
        Value::Null
    );

    // A write REST answers with no body returns an empty object.
    assert_eq!(
        harness
            .ok(
                "delete_person",
                json!({ "tree_id": tree_id, "person_id": person_id }),
            )
            .await,
        json!({})
    );
    assert_eq!(
        harness
            .error_code(
                "get_person_profile",
                json!({ "tree_id": tree_id, "person_id": person_id }),
            )
            .await,
        "not_found"
    );
    harness
        .recorded(&tree_id, &[("person_name", "delete"), ("person", "delete")])
        .await;
}

#[tokio::test]
async fn event_writes_carry_the_date_precision_and_witnesses() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Event tree").await;
    let person_id = harness.person(&tree_id, "male", "Gamma", "Sample").await;
    let witness_id = harness.person(&tree_id, "female", "Delta", "Example").await;
    let place_id = harness
        .id_of(
            "create_place",
            json!({ "tree_id": tree_id, "name": "Sampleville" }),
        )
        .await;
    let place = harness
        .ok(
            "update_place",
            json!({ "tree_id": tree_id, "place_id": place_id, "latitude": 45.5, "longitude": -1.25 }),
        )
        .await;
    assert_eq!(place["name"], "Sampleville");
    assert_eq!(place["latitude"], 45.5);

    let event = harness
        .ok(
            "create_event",
            json!({
                "tree_id": tree_id,
                "person_id": person_id,
                "event_type": "birth",
                "date_value": "1850",
                "date_qualifier": "about",
                "place_id": place_id,
                "description": "Sample description"
            }),
        )
        .await;
    assert_eq!(event["date_qualifier"], "about");
    let event_id = event["id"].as_str().unwrap().to_string();
    let birth = &harness.profile(&tree_id, &person_id).await["birth"];
    assert_eq!(birth["date_value"], "1850");
    assert_eq!(birth["date_qualifier"], "about");
    assert_eq!(birth["place_id"], place_id);

    let updated = harness
        .ok(
            "update_event",
            json!({
                "tree_id": tree_id,
                "event_id": event_id,
                "date_value": "12 MAR 1849",
                "date_qualifier": "exact",
                "calendar": "julian"
            }),
        )
        .await;
    assert_eq!(updated["calendar"], "julian");
    assert_eq!(updated["description"], "Sample description");
    let birth = &harness.profile(&tree_id, &person_id).await["birth"];
    assert_eq!(birth["date_value"], "12 MAR 1849");
    assert_eq!(birth["date_qualifier"], "exact");

    let witness = harness
        .ok(
            "add_event_witness",
            json!({ "tree_id": tree_id, "event_id": event_id, "person_id": witness_id, "relation": "godmother" }),
        )
        .await;
    let link_id = witness["id"].as_str().unwrap().to_string();
    let witnesses = harness
        .ok(
            "list_event_witnesses",
            json!({ "tree_id": tree_id, "event_id": event_id }),
        )
        .await;
    assert_eq!(witnesses["items"][0]["person_id"], witness_id);
    assert_eq!(witnesses["items"][0]["relation"], "godmother");
    harness
        .ok(
            "remove_event_witness",
            json!({ "tree_id": tree_id, "event_id": event_id, "witness_id": link_id }),
        )
        .await;
    assert_eq!(
        harness
            .ok(
                "list_event_witnesses",
                json!({ "tree_id": tree_id, "event_id": event_id }),
            )
            .await["items"],
        json!([])
    );

    harness
        .ok(
            "delete_event",
            json!({ "tree_id": tree_id, "event_id": event_id }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &person_id).await["birth"],
        Value::Null
    );
    harness
        .ok(
            "delete_place",
            json!({ "tree_id": tree_id, "place_id": place_id }),
        )
        .await;
    assert_eq!(
        harness
            .error_code(
                "get_place",
                json!({ "tree_id": tree_id, "place_id": place_id }),
            )
            .await,
        "not_found"
    );

    harness
        .recorded(
            &tree_id,
            &[
                ("event", "create"),
                ("event", "update"),
                ("event", "delete"),
                ("event_witness", "create"),
                ("event_witness", "delete"),
                ("place", "create"),
                ("place", "update"),
                ("place", "delete"),
            ],
        )
        .await;
}

#[tokio::test]
async fn family_writes_link_persons_and_kinship_reads_them() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Family tree").await;
    let father = harness.person(&tree_id, "male", "Alpha", "Sample").await;
    let mother = harness.person(&tree_id, "female", "Beta", "Example").await;
    let child = harness.person(&tree_id, "female", "Gamma", "Sample").await;

    let family_id = harness
        .id_of("create_family", json!({ "tree_id": tree_id }))
        .await;
    let mut spouse_links = Vec::new();
    for (person_id, role) in [(&father, "husband"), (&mother, "wife")] {
        let link = harness
            .ok(
                "add_spouse",
                json!({ "tree_id": tree_id, "family_id": family_id, "person_id": person_id, "role": role }),
            )
            .await;
        assert_eq!(link["role"], role);
        spouse_links.push(link["id"].as_str().unwrap().to_string());
    }
    let child_link = harness
        .ok(
            "add_child",
            json!({ "tree_id": tree_id, "family_id": family_id, "person_id": child, "child_type": "adopted" }),
        )
        .await;
    assert_eq!(child_link["child_type"], "adopted");
    let family = harness
        .ok(
            "update_family",
            json!({ "tree_id": tree_id, "family_id": family_id, "privacy": "public" }),
        )
        .await;
    assert_eq!(family["privacy"], "public");

    let family = harness
        .ok(
            "get_family",
            json!({ "tree_id": tree_id, "family_id": family_id }),
        )
        .await;
    assert_eq!(family["privacy"], "public");
    assert_eq!(family["spouses"].as_array().unwrap().len(), 2);
    assert_eq!(family["children"][0]["id"], child_link["id"]);

    let profile = harness.profile(&tree_id, &child).await;
    assert_eq!(profile["family_as_child"]["father_id"], father);
    assert_eq!(profile["family_as_child"]["child_type"], "adopted");

    let kinship = harness
        .ok(
            "get_kinship",
            json!({ "tree_id": tree_id, "person_id": child, "other_person_id": father }),
        )
        .await;
    assert_eq!(kinship["from_person_id"], child);
    assert!(!kinship["paths"].as_array().unwrap().is_empty());
    assert_eq!(
        harness
            .error_code(
                "get_kinship",
                json!({ "tree_id": tree_id, "person_id": child, "other_person_id": child }),
            )
            .await,
        "validation_error"
    );

    harness
        .ok(
            "remove_child",
            json!({ "tree_id": tree_id, "family_id": family_id, "child_id": child_link["id"] }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &child).await["family_as_child"],
        Value::Null
    );
    // A child link is not a spouse link.
    assert_eq!(
        harness
            .error_code(
                "remove_spouse",
                json!({ "tree_id": tree_id, "family_id": family_id, "spouse_id": child_link["id"] }),
            )
            .await,
        "not_found"
    );
    harness
        .ok(
            "remove_spouse",
            json!({ "tree_id": tree_id, "family_id": family_id, "spouse_id": spouse_links[0] }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &father).await["families_as_spouse"],
        json!([])
    );

    harness
        .ok(
            "delete_family",
            json!({ "tree_id": tree_id, "family_id": family_id }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &mother).await["families_as_spouse"],
        json!([])
    );

    harness
        .recorded(
            &tree_id,
            &[
                ("family", "create"),
                ("family", "update"),
                ("family", "delete"),
                ("family_spouse", "create"),
                ("family_spouse", "delete"),
                ("family_child", "create"),
                ("family_child", "delete"),
            ],
        )
        .await;
}

#[tokio::test]
async fn sources_citations_notes_and_tags_are_written() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Evidence tree").await;
    let person_id = harness.person(&tree_id, "male", "Alpha", "Sample").await;

    let source_id = harness
        .id_of(
            "create_source",
            json!({ "tree_id": tree_id, "title": "Sample register", "author": "Sample clerk" }),
        )
        .await;
    let source = harness
        .ok(
            "update_source",
            json!({ "tree_id": tree_id, "source_id": source_id, "author": null, "publisher": "Sample press" }),
        )
        .await;
    assert_eq!(source["title"], "Sample register");
    assert_eq!(source["author"], Value::Null);
    assert_eq!(source["publisher"], "Sample press");

    let citation = harness
        .ok(
            "create_citation",
            json!({ "tree_id": tree_id, "source_id": source_id, "person_id": person_id, "page": "f. 12", "confidence": "high" }),
        )
        .await;
    let citation_id = citation["id"].as_str().unwrap().to_string();
    let citation = harness
        .ok(
            "update_citation",
            json!({ "tree_id": tree_id, "citation_id": citation_id, "confidence": null }),
        )
        .await;
    assert_eq!(citation["confidence"], Value::Null);
    assert_eq!(citation["page"], "f. 12");
    assert_eq!(
        harness.profile(&tree_id, &person_id).await["citation_count"],
        1
    );

    // Still cited: an `only_if_unused` delete keeps it, and says so.
    assert_eq!(
        harness
            .ok(
                "delete_source",
                json!({ "tree_id": tree_id, "source_id": source_id, "only_if_unused": true }),
            )
            .await,
        json!({ "deleted": false })
    );

    let note_id = harness
        .id_of(
            "create_note",
            json!({ "tree_id": tree_id, "person_id": person_id, "text": "Sample remark" }),
        )
        .await;
    let note = harness
        .ok(
            "update_note",
            json!({ "tree_id": tree_id, "note_id": note_id, "text": "Revised remark" }),
        )
        .await;
    assert_eq!(note["text"], "Revised remark");
    assert_eq!(harness.profile(&tree_id, &person_id).await["note_count"], 1);

    let tag = harness
        .ok(
            "create_tag",
            json!({ "tree_id": tree_id, "label": "Checked" }),
        )
        .await;
    assert_eq!(tag["created"], true);
    let tag_id = tag["id"].as_str().unwrap().to_string();
    let again = harness
        .ok(
            "create_tag",
            json!({ "tree_id": tree_id, "label": "checked" }),
        )
        .await;
    assert_eq!(again["created"], false);
    assert_eq!(again["id"], tag_id);
    for _ in 0..2 {
        assert_eq!(
            harness
                .ok(
                    "tag_record",
                    json!({ "tree_id": tree_id, "tag_id": tag_id, "kind": "person", "record_id": person_id }),
                )
                .await,
            json!({})
        );
    }
    assert_eq!(
        harness.profile(&tree_id, &person_id).await["tags"][0]["label"],
        "Checked"
    );
    harness
        .ok(
            "tag_record",
            json!({ "tree_id": tree_id, "tag_id": tag_id, "kind": "note", "record_id": note_id }),
        )
        .await;
    harness
        .ok(
            "untag_record",
            json!({ "tree_id": tree_id, "tag_id": tag_id, "kind": "person", "record_id": person_id }),
        )
        .await;
    assert_eq!(
        harness.profile(&tree_id, &person_id).await["tags"],
        json!([])
    );
    let tags = harness.ok("list_tags", json!({ "tree_id": tree_id })).await;
    assert_eq!(tags["items"][0]["counts"]["notes"], 1);
    assert_eq!(tags["items"][0]["counts"]["persons"], 0);
    harness
        .ok(
            "delete_tag",
            json!({ "tree_id": tree_id, "tag_id": tag_id }),
        )
        .await;
    assert_eq!(
        harness.ok("list_tags", json!({ "tree_id": tree_id })).await["items"],
        json!([])
    );

    harness
        .ok(
            "delete_note",
            json!({ "tree_id": tree_id, "note_id": note_id }),
        )
        .await;
    harness
        .ok(
            "delete_citation",
            json!({ "tree_id": tree_id, "citation_id": citation_id }),
        )
        .await;
    let profile = harness.profile(&tree_id, &person_id).await;
    assert_eq!(profile["note_count"], 0);
    assert_eq!(profile["citation_count"], 0);
    assert_eq!(
        harness
            .ok(
                "delete_source",
                json!({ "tree_id": tree_id, "source_id": source_id, "only_if_unused": true }),
            )
            .await,
        json!({ "deleted": true })
    );

    harness
        .recorded(
            &tree_id,
            &[
                ("source", "create"),
                ("source", "update"),
                ("source", "delete"),
                ("citation", "create"),
                ("citation", "update"),
                ("citation", "delete"),
                ("note", "create"),
                ("note", "update"),
                ("note", "delete"),
                ("tag", "create"),
                ("tag", "delete"),
                ("record_tag", "create"),
                ("record_tag", "delete"),
            ],
        )
        .await;
}

#[tokio::test]
async fn invalid_writes_are_refused_and_change_nothing() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Strict tree").await;
    let person_id = harness.person(&tree_id, "male", "Alpha", "Sample").await;
    let entries = harness.audit(&tree_id).await.len();

    for (tool, arguments) in [
        (
            "create_note",
            json!({ "tree_id": tree_id, "person_id": person_id, "text": "   " }),
        ),
        (
            "create_event",
            json!({ "tree_id": tree_id, "person_id": person_id, "event_type": "birth", "age": "not an age" }),
        ),
        (
            "create_tag",
            json!({ "tree_id": tree_id, "label": "Out of range", "color": 999 }),
        ),
    ] {
        assert_eq!(
            harness.error_code(tool, arguments).await,
            "validation_error",
            "{tool}"
        );
    }

    // Arguments that do not fit the input schema never reach the service.
    for (tool, arguments) in [
        (
            "add_person_name",
            json!({ "tree_id": tree_id, "person_id": person_id, "given_names": "Alpha" }),
        ),
        (
            "create_person",
            json!({ "tree_id": tree_id, "sex": "neither" }),
        ),
        ("create_family", json!({})),
    ] {
        let (is_error, _) = harness.call(tool, arguments).await;
        assert!(is_error, "{tool}");
    }

    assert_eq!(harness.audit(&tree_id).await.len(), entries);
    assert_eq!(
        harness
            .ok("search_persons", json!({ "tree_id": tree_id }))
            .await["total_count"],
        1
    );
}

#[tokio::test]
async fn a_write_naming_an_id_from_another_tree_is_not_found() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Scope tree").await;
    let other_tree_id = harness.tree("Other tree").await;
    let local = harness.person(&tree_id, "male", "Alpha", "Sample").await;
    let foreign = harness
        .person(&other_tree_id, "female", "Delta", "Sample")
        .await;
    let foreign_source = harness
        .created(
            &format!("/api/v1/trees/{other_tree_id}/sources"),
            json!({ "title": "Sample register" }),
        )
        .await;
    let foreign_tag = harness
        .created(
            &format!("/api/v1/trees/{other_tree_id}/tags"),
            json!({ "label": "Foreign" }),
        )
        .await;
    let family_id = harness
        .id_of("create_family", json!({ "tree_id": tree_id }))
        .await;
    let entries = harness.audit(&tree_id).await.len();
    let other_entries = harness.audit(&other_tree_id).await.len();

    for (tool, arguments) in [
        (
            "update_person",
            json!({ "tree_id": tree_id, "person_id": foreign, "sex": "male" }),
        ),
        (
            "add_person_name",
            json!({ "tree_id": tree_id, "person_id": foreign, "name_type": "birth", "is_primary": false }),
        ),
        (
            "add_spouse",
            json!({ "tree_id": tree_id, "family_id": family_id, "person_id": foreign, "role": "wife" }),
        ),
        (
            "add_child",
            json!({ "tree_id": tree_id, "family_id": family_id, "person_id": foreign, "child_type": "biological" }),
        ),
        (
            "create_event",
            json!({ "tree_id": tree_id, "person_id": foreign, "event_type": "death" }),
        ),
        (
            "create_citation",
            json!({ "tree_id": tree_id, "source_id": foreign_source, "person_id": local }),
        ),
        (
            "create_note",
            json!({ "tree_id": tree_id, "person_id": foreign, "text": "Remark" }),
        ),
        (
            "tag_record",
            json!({ "tree_id": tree_id, "tag_id": foreign_tag, "kind": "person", "record_id": local }),
        ),
        (
            "delete_source",
            json!({ "tree_id": tree_id, "source_id": foreign_source }),
        ),
        (
            "delete_person",
            json!({ "tree_id": tree_id, "person_id": foreign }),
        ),
        (
            "get_family",
            json!({ "tree_id": other_tree_id, "family_id": family_id }),
        ),
        (
            "get_kinship",
            json!({ "tree_id": tree_id, "person_id": local, "other_person_id": foreign }),
        ),
    ] {
        assert_eq!(
            harness.error_code(tool, arguments).await,
            "not_found",
            "{tool}"
        );
    }

    // Nothing was written to either tree.
    assert_eq!(harness.audit(&tree_id).await.len(), entries);
    assert_eq!(harness.audit(&other_tree_id).await.len(), other_entries);
    assert_eq!(
        harness.profile(&other_tree_id, &foreign).await["sex"],
        "female"
    );
}

#[tokio::test]
async fn a_write_to_a_tree_deleted_mid_session_is_not_found() {
    let harness = setup_writes().await;
    let tree_id = harness.tree("Doomed tree").await;
    let person_id = harness.person(&tree_id, "male", "Alpha", "Sample").await;

    let (status, _) = harness
        .rest(Method::DELETE, &format!("/api/v1/trees/{tree_id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    for (tool, arguments) in [
        (
            "create_person",
            json!({ "tree_id": tree_id, "sex": "male" }),
        ),
        ("create_family", json!({ "tree_id": tree_id })),
        (
            "create_place",
            json!({ "tree_id": tree_id, "name": "Sampleville" }),
        ),
        ("create_tag", json!({ "tree_id": tree_id, "label": "Late" })),
        (
            "update_person",
            json!({ "tree_id": tree_id, "person_id": person_id, "sex": "female" }),
        ),
        (
            "delete_person",
            json!({ "tree_id": tree_id, "person_id": person_id }),
        ),
    ] {
        assert_eq!(
            harness.error_code(tool, arguments).await,
            "not_found",
            "{tool}"
        );
    }
}
