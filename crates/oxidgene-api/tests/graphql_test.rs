//! Integration tests for GraphQL API.
//!
//! All tests run against an in-memory SQLite database. Requests are sent
//! to `POST /graphql` via Axum's tower `ServiceExt::oneshot`.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
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

/// Helper: send a GraphQL query/mutation and return the full JSON response.
async fn graphql(app: axum::Router, query: &str, variables: Option<Value>) -> Value {
    let body = match variables {
        Some(vars) => json!({ "query": query, "variables": vars }),
        None => json!({ "query": query }),
    };

    let request = Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Helper: extract `data` field from a GraphQL response, panicking on errors.
fn data(resp: &Value) -> &Value {
    if let Some(errors) = resp.get("errors") {
        panic!("GraphQL errors: {errors}");
    }
    resp.get("data").expect("missing 'data' in response")
}

// ── Tree CRUD ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_tree_create_and_query() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "My Tree", description: "A test tree" }) { id name description } }"#,
        None,
    )
    .await;
    let tree = &data(&resp)["createTree"];
    assert_eq!(tree["name"], "My Tree");
    assert_eq!(tree["description"], "A test tree");
    let tree_id = tree["id"].as_str().unwrap();

    // Query single tree
    let resp = graphql(
        app.clone(),
        &format!(r#"{{ tree(id: "{tree_id}") {{ id name description }} }}"#),
        None,
    )
    .await;
    let fetched = &data(&resp)["tree"];
    assert_eq!(fetched["name"], "My Tree");
}

#[tokio::test]
async fn test_tree_update_and_delete() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Old Name" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Update
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateTree(id: "{tree_id}", input: {{ name: "New Name", description: "Updated" }}) {{ id name description }} }}"#
        ),
        None,
    )
    .await;
    let updated = &data(&resp)["updateTree"];
    assert_eq!(updated["name"], "New Name");
    assert_eq!(updated["description"], "Updated");

    // Delete
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ deleteTree(id: "{tree_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteTree"], true);

    // Verify gone from list
    let resp = graphql(app, "{ trees { totalCount } }", None).await;
    assert_eq!(data(&resp)["trees"]["totalCount"], 0);
}

#[tokio::test]
async fn test_tree_pagination() {
    let app = setup_app().await;

    // Create 3 trees
    for i in 1..=3 {
        graphql(
            app.clone(),
            &format!(r#"mutation {{ createTree(input: {{ name: "Tree {i}" }}) {{ id }} }}"#),
            None,
        )
        .await;
    }

    // Page of 2
    let resp = graphql(
        app.clone(),
        "{ trees(first: 2) { edges { cursor node { name } } pageInfo { hasNextPage endCursor } totalCount } }",
        None,
    )
    .await;
    let conn = &data(&resp)["trees"];
    assert_eq!(conn["totalCount"], 3);
    assert_eq!(conn["edges"].as_array().unwrap().len(), 2);
    assert_eq!(conn["pageInfo"]["hasNextPage"], true);

    // Next page
    let cursor = conn["pageInfo"]["endCursor"].as_str().unwrap();
    let resp = graphql(
        app,
        &format!(
            r#"{{ trees(first: 2, after: "{cursor}") {{ edges {{ node {{ name }} }} pageInfo {{ hasNextPage }} totalCount }} }}"#
        ),
        None,
    )
    .await;
    let conn2 = &data(&resp)["trees"];
    assert_eq!(conn2["edges"].as_array().unwrap().len(), 1);
    assert_eq!(conn2["pageInfo"]["hasNextPage"], false);
}

// ── Person CRUD with nested names ────────────────────────────────────

#[tokio::test]
async fn test_person_crud_with_names() {
    let app = setup_app().await;

    // Create tree
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "T" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create person
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id sex }} }}"#
        ),
        None,
    )
    .await;
    let person = &data(&resp)["createPerson"];
    assert_eq!(person["sex"], "MALE");
    let person_id = person["id"].as_str().unwrap().to_string();

    // Add name
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "John", surname: "Doe", isPrimary: true }}) {{ id givenNames surname isPrimary }} }}"#
        ),
        None,
    )
    .await;
    let name = &data(&resp)["addPersonName"];
    assert_eq!(name["givenNames"], "John");
    assert_eq!(name["surname"], "Doe");
    assert_eq!(name["isPrimary"], true);

    // Query person with nested names via primaryName
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ persons(treeId: "{tree_id}") {{ edges {{ node {{ id sex primaryName {{ givenNames surname }} names {{ id nameType }} }} }} }} }}"#
        ),
        None,
    )
    .await;
    let edges = data(&resp)["persons"]["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    let p = &edges[0]["node"];
    assert_eq!(p["primaryName"]["givenNames"], "John");
    assert_eq!(p["names"].as_array().unwrap().len(), 1);

    // Update person sex
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updatePerson(id: "{person_id}", input: {{ sex: FEMALE }}) {{ id sex }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updatePerson"]["sex"], "FEMALE");

    // Delete person
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deletePerson(id: "{person_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deletePerson"], true);
}

// ── Family with spouses and children ─────────────────────────────────

#[tokio::test]
async fn test_family_with_members() {
    let app = setup_app().await;

    // Setup tree + persons
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Fam" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let husband_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let wife_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let child_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create family
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ createFamily(treeId: "{tree_id}") {{ id }} }}"#),
        None,
    )
    .await;
    let family_id = data(&resp)["createFamily"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Add spouses
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addSpouse(familyId: "{family_id}", input: {{ personId: "{husband_id}", role: HUSBAND }}) {{ id role }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["addSpouse"]["role"], "HUSBAND");
    let spouse_link_id = data(&resp)["addSpouse"]["id"].as_str().unwrap().to_string();

    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addSpouse(familyId: "{family_id}", input: {{ personId: "{wife_id}", role: WIFE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    // Add child
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addChild(familyId: "{family_id}", input: {{ personId: "{child_id}", childType: BIOLOGICAL }}) {{ id childType }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["addChild"]["childType"], "BIOLOGICAL");

    // Query family with resolved members
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ family(treeId: "{tree_id}", id: "{family_id}") {{ id spouses {{ person {{ id sex }} role }} children {{ person {{ id }} childType }} }} }}"#
        ),
        None,
    )
    .await;
    let fam = &data(&resp)["family"];
    assert_eq!(fam["spouses"].as_array().unwrap().len(), 2);
    assert_eq!(fam["children"].as_array().unwrap().len(), 1);

    // Remove spouse
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ removeSpouse(id: "{spouse_link_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["removeSpouse"], true);
}

// ── Event with place resolution ──────────────────────────────────────

#[tokio::test]
async fn test_event_with_place() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "E" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create place
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "Paris", latitude: 48.8566, longitude: 2.3522 }}) {{ id name latitude longitude }} }}"#
        ),
        None,
    )
    .await;
    let place = &data(&resp)["createPlace"];
    assert_eq!(place["name"], "Paris");
    let place_id = place["id"].as_str().unwrap().to_string();

    // Create event linked to person and place
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: BIRTH, dateValue: "1 Jan 1900", dateSort: "1900-01-01", placeId: "{place_id}", personId: "{person_id}" }}) {{ id eventType dateValue dateSort }} }}"#
        ),
        None,
    )
    .await;
    let event = &data(&resp)["createEvent"];
    assert_eq!(event["eventType"], "BIRTH");
    assert_eq!(event["dateValue"], "1 Jan 1900");
    assert_eq!(event["dateSort"], "1900-01-01");
    let event_id = event["id"].as_str().unwrap().to_string();

    // Query event with resolved place
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ event(treeId: "{tree_id}", id: "{event_id}") {{ id eventType place {{ name latitude }} person {{ id }} }} }}"#
        ),
        None,
    )
    .await;
    let ev = &data(&resp)["event"];
    assert_eq!(ev["place"]["name"], "Paris");
    assert!(ev["person"]["id"].as_str().is_some());

    // Update event
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateEvent(id: "{event_id}", input: {{ description: "Updated birth" }}) {{ id description }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateEvent"]["description"], "Updated birth");

    // Delete event
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteEvent(id: "{event_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteEvent"], true);
}

// ── Source + Citation CRUD ────────────────────────────────────────────

#[tokio::test]
async fn test_source_and_citation() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "S" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create source
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createSource(treeId: "{tree_id}", input: {{ title: "Census 1900", author: "Govt" }}) {{ id title author }} }}"#
        ),
        None,
    )
    .await;
    let src = &data(&resp)["createSource"];
    assert_eq!(src["title"], "Census 1900");
    assert_eq!(src["author"], "Govt");
    let source_id = src["id"].as_str().unwrap().to_string();

    // Create citation
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createCitation(input: {{ sourceId: "{source_id}", page: "42", confidence: HIGH, text: "entry text" }}) {{ id page confidence text }} }}"#
        ),
        None,
    )
    .await;
    let cit = &data(&resp)["createCitation"];
    assert_eq!(cit["page"], "42");
    assert_eq!(cit["confidence"], "HIGH");
    let citation_id = cit["id"].as_str().unwrap().to_string();

    // Query source with nested citations
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ source(treeId: "{tree_id}", id: "{source_id}") {{ title citations {{ id page confidence }} }} }}"#
        ),
        None,
    )
    .await;
    let fetched = &data(&resp)["source"];
    assert_eq!(fetched["citations"].as_array().unwrap().len(), 1);

    // Update citation
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateCitation(id: "{citation_id}", input: {{ page: "43" }}) {{ id page }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateCitation"]["page"], "43");

    // Delete citation
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteCitation(id: "{citation_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteCitation"], true);
}

// ── Media + MediaLink CRUD ───────────────────────────────────────────

#[tokio::test]
async fn test_media_and_media_link() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "M" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Upload media
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMedia(treeId: "{tree_id}", input: {{ fileName: "photo.jpg", mimeType: "image/jpeg", filePath: "/uploads/photo.jpg", fileSize: 1024, title: "Portrait" }}) {{ id fileName title }} }}"#
        ),
        None,
    )
    .await;
    let media = &data(&resp)["uploadMedia"];
    assert_eq!(media["fileName"], "photo.jpg");
    assert_eq!(media["title"], "Portrait");
    let media_id = media["id"].as_str().unwrap().to_string();

    // Create media link
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createMediaLink(input: {{ mediaId: "{media_id}", personId: "{person_id}" }}) {{ id mediaId personId }} }}"#
        ),
        None,
    )
    .await;
    let link = &data(&resp)["createMediaLink"];
    assert_eq!(link["mediaId"], media_id);
    let link_id = link["id"].as_str().unwrap().to_string();

    // Update media
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateMedia(id: "{media_id}", input: {{ title: "New Portrait" }}) {{ id title }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateMedia"]["title"], "New Portrait");

    // Delete media link
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ deleteMediaLink(id: "{link_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteMediaLink"], true);

    // Delete media
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteMedia(id: "{media_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteMedia"], true);
}

// ── Note CRUD ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_note_crud() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "N" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create note
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createNote(treeId: "{tree_id}", input: {{ text: "Important note", personId: "{person_id}" }}) {{ id text personId }} }}"#
        ),
        None,
    )
    .await;
    let note = &data(&resp)["createNote"];
    assert_eq!(note["text"], "Important note");
    let note_id = note["id"].as_str().unwrap().to_string();

    // Update note
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateNote(id: "{note_id}", input: {{ text: "Updated note" }}) {{ id text }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateNote"]["text"], "Updated note");

    // Query person's notes via nested resolver
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ persons(treeId: "{tree_id}") {{ edges {{ node {{ notes {{ id text }} }} }} }} }}"#
        ),
        None,
    )
    .await;
    let nodes = data(&resp)["persons"]["edges"].as_array().unwrap();
    assert_eq!(nodes[0]["node"]["notes"].as_array().unwrap().len(), 1);

    // Delete note
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteNote(id: "{note_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteNote"], true);
}

// ── Ancestors / Descendants (empty) ──────────────────────────────────

#[tokio::test]
async fn test_ancestors_descendants_empty() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Anc" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Ancestors (empty)
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ ancestors(treeId: "{tree_id}", personId: "{person_id}") {{ person {{ id }} depth }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["ancestors"].as_array().unwrap().len(), 0);

    // Descendants (empty)
    let resp = graphql(
        app,
        &format!(
            r#"{{ descendants(treeId: "{tree_id}", personId: "{person_id}") {{ person {{ id }} depth }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["descendants"].as_array().unwrap().len(), 0);
}

// ── Error handling: not found ────────────────────────────────────────

#[tokio::test]
async fn test_query_not_found_returns_null() {
    let app = setup_app().await;

    let resp = graphql(
        app,
        r#"{ tree(id: "00000000-0000-0000-0000-000000000000") { id name } }"#,
        None,
    )
    .await;
    // Should return null, not an error
    assert!(data(&resp)["tree"].is_null());
}

// ── Error handling: invalid UUID ─────────────────────────────────────

#[tokio::test]
async fn test_mutation_invalid_uuid() {
    let app = setup_app().await;

    let resp = graphql(
        app,
        r#"mutation { updateTree(id: "not-a-uuid", input: { name: "X" }) { id } }"#,
        None,
    )
    .await;
    // Should have errors
    assert!(resp.get("errors").is_some());
}

// ── Place search ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_place_search() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "P" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create places
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "Paris, France" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "London, UK" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    // Search
    let resp = graphql(
        app,
        &format!(
            r#"{{ places(treeId: "{tree_id}", search: "Paris") {{ edges {{ node {{ name }} }} totalCount }} }}"#
        ),
        None,
    )
    .await;
    let places = &data(&resp)["places"];
    assert_eq!(places["totalCount"], 1);
    assert_eq!(places["edges"][0]["node"]["name"], "Paris, France");
}

// ── PersonName update and delete ─────────────────────────────────────

#[tokio::test]
async fn test_person_name_update_delete() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "PN" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Add name
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "John", surname: "Smith", isPrimary: true }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let name_id = data(&resp)["addPersonName"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Update name
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updatePersonName(id: "{name_id}", input: {{ surname: "Jones" }}) {{ id surname }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updatePersonName"]["surname"], "Jones");

    // Delete name
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deletePersonName(id: "{name_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deletePersonName"], true);
}

// ── GraphiQL playground ──────────────────────────────────────────────

#[tokio::test]
async fn test_graphiql_playground() {
    let app = setup_app().await;

    let request = Request::builder()
        .method(Method::GET)
        .uri("/graphql")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    // Should contain GraphiQL HTML
    assert!(body.contains("graphiql"));
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
async fn test_graphql_import_gedcom() {
    let app = setup_app().await;

    // Create tree
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "GQL GEDCOM Tree" }) { id name } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Import GEDCOM
    let query = format!(
        r#"mutation {{
            importGedcom(treeId: "{tree_id}", input: {{ gedcom: "{}" }}) {{
                personsCount
                familiesCount
                eventsCount
                sourcesCount
                mediaCount
                placesCount
                notesCount
                warnings
            }}
        }}"#,
        minimal_gedcom().replace('\n', "\\n").replace('"', "\\\"")
    );
    let resp = graphql(app.clone(), &query, None).await;
    let result = &data(&resp)["importGedcom"];
    assert_eq!(result["personsCount"], 2);
    assert_eq!(result["familiesCount"], 1);
    assert!(result["eventsCount"].as_i64().unwrap() >= 2);
    assert!(result["placesCount"].as_i64().unwrap() >= 1);

    // Verify persons are in the DB via GraphQL
    let query = format!(
        r#"{{ persons(treeId: "{tree_id}") {{ edges {{ node {{ id sex }} }} totalCount }} }}"#
    );
    let resp = graphql(app.clone(), &query, None).await;
    let persons = &data(&resp)["persons"];
    assert_eq!(persons["totalCount"], 2);
}

#[tokio::test]
async fn test_graphql_export_gedcom() {
    let app = setup_app().await;

    // Create tree
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "GQL Export Tree" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Export empty tree
    let query = format!(r#"{{ exportGedcom(treeId: "{tree_id}") {{ gedcom warnings }} }}"#);
    let resp = graphql(app.clone(), &query, None).await;
    let result = &data(&resp)["exportGedcom"];
    assert!(result["gedcom"].as_str().unwrap().contains("HEAD"));
    assert!(result["warnings"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_graphql_gedcom_roundtrip() {
    let app = setup_app().await;

    // Create tree
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "GQL Roundtrip" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Import
    let import_query = format!(
        r#"mutation {{
            importGedcom(treeId: "{tree_id}", input: {{ gedcom: "{}" }}) {{
                personsCount familiesCount eventsCount
            }}
        }}"#,
        minimal_gedcom().replace('\n', "\\n").replace('"', "\\\"")
    );
    let resp = graphql(app.clone(), &import_query, None).await;
    let import_result = &data(&resp)["importGedcom"];
    assert_eq!(import_result["personsCount"], 2);
    assert_eq!(import_result["familiesCount"], 1);

    // Export
    let export_query = format!(r#"{{ exportGedcom(treeId: "{tree_id}") {{ gedcom warnings }} }}"#);
    let resp = graphql(app.clone(), &export_query, None).await;
    let export_result = &data(&resp)["exportGedcom"];
    let gedcom = export_result["gedcom"].as_str().unwrap();
    assert!(gedcom.contains("INDI"));
    assert!(gedcom.contains("FAM"));
}

#[tokio::test]
async fn test_graphql_import_gedcom_invalid_tree() {
    let app = setup_app().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";

    let query = format!(
        r#"mutation {{
            importGedcom(treeId: "{fake_id}", input: {{ gedcom: "0 HEAD\n0 TRLR\n" }}) {{
                personsCount
            }}
        }}"#
    );
    let resp = graphql(app.clone(), &query, None).await;
    // Should have errors
    assert!(resp.get("errors").is_some());
}
