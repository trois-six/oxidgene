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
    // Media lands in a throwaway directory: these tests never upload,
    // but `AppState` needs a root and it must not be the developer's.
    let state = AppState::new(db, std::env::temp_dir().join("oxidgene-test-media"));
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
    assert_eq!(response.status(), StatusCode::OK, "GraphQL query: {query}");
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
async fn graphql_errors_use_safe_messages_and_stable_codes() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Error Contract" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"].as_str().unwrap();

    let resp = graphql(
        app,
        &format!(r#"mutation {{ updateTree(id: "{tree_id}", input: {{ name: "" }}) {{ id }} }}"#),
        None,
    )
    .await;
    let error = &resp["errors"][0];

    assert_eq!(error["message"], "The request is invalid");
    assert_eq!(error["extensions"]["code"], "VALIDATION_ERROR");
    assert!(error["extensions"].get("requestId").is_none());
}

#[tokio::test]
async fn test_tree_duplicate_preserves_genealogy() {
    let app = setup_app().await;
    let source_tree_id = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Original" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let person_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{source_tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(treeId: "{source_tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "Ada", surname: "Lovelace", isPrimary: true }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    let duplicate = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ duplicateTree(treeId: "{source_tree_id}", name: "Copy") {{ id name personCount }} }}"#
            ),
            None,
        )
        .await,
    )["duplicateTree"]
        .clone();
    assert_eq!(duplicate["name"], "Copy");
    assert_eq!(duplicate["personCount"], 1);

    let copied_tree_id = duplicate["id"].as_str().unwrap();
    let people = data(
        &graphql(
            app,
            &format!(
                r#"{{ persons(treeId: "{copied_tree_id}") {{ edges {{ node {{ primaryName {{ givenNames surname }} }} }} }} }}"#
            ),
            None,
        )
        .await,
    )["persons"]["edges"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(people.len(), 1);
    assert_eq!(people[0]["node"]["primaryName"]["givenNames"], "Ada");
    assert_eq!(people[0]["node"]["primaryName"]["surname"], "Lovelace");
}

#[tokio::test]
async fn test_sosa_and_portraits_are_available_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let person_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateTree(id: "{tree_id}", input: {{ sosaRootPersonId: "{person_id}" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    let sosa = data(
        &graphql(
            app.clone(),
            &format!(r#"{{ personBySosa(treeId: "{tree_id}", number: 1) {{ id }} }}"#),
            None,
        )
        .await,
    )["personBySosa"]
        .clone();
    assert_eq!(sosa["id"], person_id);

    let media_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{ fileName: "portrait.png", contentBase64: "{}" }}) {{ id }} }}"#,
                png_base64(20, 20)
            ),
            None,
        )
        .await,
    )["uploadMediaFile"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ setPersonPortrait(treeId: "{tree_id}", personId: "{person_id}", mediaId: "{media_id}") {{ id }} }}"#
        ),
        None,
    )
    .await;

    let portraits = data(
        &graphql(
            app,
            &format!(
                r#"{{ portraits(treeId: "{tree_id}") {{ personId mediaId vignetteId hasThumbnail }} }}"#
            ),
            None,
        )
        .await,
    )["portraits"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(portraits.len(), 1);
    assert_eq!(portraits[0]["personId"], person_id);
    assert_eq!(portraits[0]["mediaId"], media_id);
    assert!(portraits[0]["vignetteId"].is_null());
    assert_eq!(portraits[0]["hasThumbnail"], true);
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
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "John", surname: "Doe", isPrimary: true }}) {{ id givenNames surname isPrimary }} }}"#
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
            r#"mutation {{ updatePerson(treeId: "{tree_id}", id: "{person_id}", input: {{ sex: FEMALE }}) {{ id sex }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updatePerson"]["sex"], "FEMALE");

    // Delete person
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deletePerson(treeId: "{tree_id}", id: "{person_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deletePerson"], true);
}

#[tokio::test]
async fn person_from_another_tree_is_not_exposed_by_graphql() {
    let app = setup_app().await;
    let first_tree = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "First" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let second_tree = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Second" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let person_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{second_tree}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = graphql(
        app.clone(),
        &format!(r#"{{ person(treeId: "{first_tree}", id: "{person_id}") {{ id }} }}"#),
        None,
    )
    .await;
    assert!(data(&response)["person"].is_null());

    let response = graphql(
        app,
        &format!(
            r#"mutation {{ updatePerson(treeId: "{first_tree}", id: "{person_id}", input: {{ sex: MALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    assert!(response.get("errors").is_some());
}

// ── Family with spouses and children ─────────────────────────────────

#[tokio::test]
async fn test_search_persons_filters_by_spouse() {
    let app = setup_app().await;
    let tree_id = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Search tree" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mut person_ids = Vec::new();
    for (sex, given_names, surname) in [
        ("MALE", "SearchSubject", "Subject"),
        ("FEMALE", "RelatedPerson", "RelativeMatch"),
    ] {
        let person_id = data(
            &graphql(
                app.clone(),
                &format!(
                    r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: {sex} }}) {{ id }} }}"#
                ),
                None,
            )
            .await,
        )["createPerson"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        data(
            &graphql(
                app.clone(),
                &format!(
                    r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "{given_names}", surname: "{surname}", isPrimary: true }}) {{ id }} }}"#
                ),
                None,
            )
            .await,
        );
        person_ids.push(person_id);
    }

    let family_id = data(
        &graphql(
            app.clone(),
            &format!(r#"mutation {{ createFamily(treeId: "{tree_id}") {{ id }} }}"#),
            None,
        )
        .await,
    )["createFamily"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (person_id, role) in person_ids.iter().zip(["HUSBAND", "WIFE"]) {
        data(
            &graphql(
                app.clone(),
                &format!(
                    r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{person_id}", role: {role} }}) {{ id }} }}"#
                ),
                None,
            )
            .await,
        );
    }

    let response = graphql(
        app,
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "", surname: "subject", spouseSurname: "relative") {{ totalCount entries {{ displayName }} }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["searchPersons"];
    assert_eq!(result["totalCount"], 1);
    assert_eq!(result["entries"][0]["displayName"], "SearchSubject Subject");
}

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

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateFamily(treeId: "{tree_id}", id: "{family_id}", input: {{ privacy: PRIVATE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateFamily"]["id"], family_id);

    // Add spouses
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{husband_id}", role: HUSBAND }}) {{ id role }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["addSpouse"]["role"], "HUSBAND");
    let spouse_link_id = data(&resp)["addSpouse"]["id"].as_str().unwrap().to_string();

    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{wife_id}", role: WIFE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    // Add child
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addChild(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{child_id}", childType: BIOLOGICAL }}) {{ id childType }} }}"#
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
        &format!(r#"mutation {{ removeSpouse(treeId: "{tree_id}", familyId: "{family_id}", id: "{spouse_link_id}") }}"#),
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
            r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "Paris", latitude: 42.4242, longitude: 2.4242 }}) {{ id name latitude longitude }} }}"#
        ),
        None,
    )
    .await;
    let place = &data(&resp)["createPlace"];
    assert_eq!(place["name"], "Paris");
    let place_id = place["id"].as_str().unwrap().to_string();

    // Create event linked to person and place. `dateSort` is not an input:
    // the server derives it from the date value and its calendar.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: BIRTH, dateValue: "1 Jan 1900", placeId: "{place_id}", personId: "{person_id}" }}) {{ id eventType dateValue dateSort }} }}"#
        ),
        None,
    )
    .await;
    let event = &data(&resp)["createEvent"];
    assert_eq!(event["eventType"], "BIRTH");
    assert_eq!(event["dateValue"], "1 Jan 1900");
    assert_eq!(event["dateSort"], "1900-01-01");
    let event_id = event["id"].as_str().unwrap().to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ birth {{ placeName }} }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["birth"]["placeName"], "Paris");

    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updatePlace(treeId: "{tree_id}", id: "{place_id}", input: {{ name: "Lyon" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ birth {{ placeName }} }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["birth"]["placeName"], "Lyon");

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
    assert_eq!(ev["place"]["name"], "Lyon");
    assert!(ev["person"]["id"].as_str().is_some());

    // Update event
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateEvent(treeId: "{tree_id}", id: "{event_id}", input: {{ description: "Updated birth" }}) {{ id description }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateEvent"]["description"], "Updated birth");

    // Delete event
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteEvent(treeId: "{tree_id}", id: "{event_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteEvent"], true);
}

#[tokio::test]
async fn test_dictionary_snapshot_and_reference_over_graphql() {
    let app = setup_app().await;
    let tree_id = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Dictionary" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let person_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "Marie", surname: "Durand", isPrimary: true }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let place_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "Lyon" }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createPlace"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: OCCUPATION, personId: "{person_id}", placeId: "{place_id}", description: "Agriculteur" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let source_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createSource(treeId: "{tree_id}", input: {{ title: "Lyon register" }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createSource"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createCitation(treeId: "{tree_id}", input: {{ sourceId: "{source_id}", personId: "{person_id}", confidence: HIGH }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    let response = graphql(
        app.clone(),
        &format!(
            r#"{{
                dictionaryFamilyNames(treeId: "{tree_id}") {{ value count }}
                dictionaryOccupations(treeId: "{tree_id}") {{ value count }}
                dictionarySources(treeId: "{tree_id}") {{ source {{ id title }} count }}
                dictionaryPlaces(treeId: "{tree_id}") {{ place {{ id name }} count }}
                familyNameUsage(treeId: "{tree_id}", value: "Durand") {{ personId }}
                occupationUsage(treeId: "{tree_id}", value: "Agriculteur") {{ personId }}
                sourceUsage(sourceId: "{source_id}") {{ personId }}
                placeUsage(placeId: "{place_id}") {{ personId }}
                treeSnapshot(treeId: "{tree_id}") {{ persons {{ id }} names {{ surname }} events {{ eventType }} places {{ name }} }}
                occupationReference(language: "fr", term: "Agriculteur") {{ label }}
                givenNameReference(language: "fr", term: "Marie") {{ label }}
            }}"#
        ),
        None,
    )
    .await;
    let response = data(&response);

    assert_eq!(response["dictionaryFamilyNames"][0]["value"], "Durand");
    assert_eq!(response["dictionaryOccupations"][0]["value"], "Agriculteur");
    assert_eq!(response["dictionarySources"][0]["source"]["id"], source_id);
    assert_eq!(response["dictionarySources"][0]["count"], 1);
    assert_eq!(response["dictionaryPlaces"][0]["place"]["id"], place_id);
    assert_eq!(response["dictionaryPlaces"][0]["count"], 1);
    for key in [
        "familyNameUsage",
        "occupationUsage",
        "sourceUsage",
        "placeUsage",
    ] {
        assert_eq!(response[key][0]["personId"], person_id, "{key}: {response}");
    }
    assert_eq!(response["treeSnapshot"]["persons"][0]["id"], person_id);
    assert_eq!(response["treeSnapshot"]["names"][0]["surname"], "Durand");
    assert_eq!(
        response["treeSnapshot"]["events"][0]["eventType"],
        "OCCUPATION"
    );
    assert_eq!(response["treeSnapshot"]["places"][0]["name"], "Lyon");
    assert_eq!(response["occupationReference"]["label"], "Agriculteur");
    assert_eq!(response["givenNameReference"]["label"], "Marie");
}

// ── date_sort is the server's to derive ───────────────────────────────

/// A date written in another calendar has to be normalised to Gregorian
/// before it can be sorted against the rest, and only the server can do that.
/// A Republican `2 BRUM 14` read at face value files under year 14 — thirteen
/// centuries adrift — which is what the frontend used to send.
#[tokio::test]
async fn a_republican_date_is_sorted_where_it_belongs() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "R" }) { id } }"#,
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
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: BIRTH, dateValue: "2 BRUM 14", calendar: FRENCH_REPUBLICAN }}) {{ id dateValue dateSort }} }}"#
        ),
        None,
    )
    .await;
    let event = &data(&resp)["createEvent"];
    // The value is stored as written; only the sort key is converted.
    assert_eq!(event["dateValue"], "2 BRUM 14");
    let sort = event["dateSort"].as_str().expect("a sort key was derived");
    assert!(sort.starts_with("1805-10"), "sorted as {sort}");
    let event_id = event["id"].as_str().unwrap().to_string();

    // Re-deriving on update: the patch touches only the calendar, so the
    // stored value has to be read back to make sense of it. The same digits
    // now mean an ordinary Gregorian day in year 14.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateEvent(treeId: "{tree_id}", id: "{event_id}", input: {{ calendar: GREGORIAN }}) {{ dateSort }} }}"#
        ),
        None,
    )
    .await;
    let sort = data(&resp)["updateEvent"]["dateSort"].as_str();
    assert_ne!(sort, Some("1805-10-23"), "the sort key was not re-derived");

    // And clearing the date clears the key with it.
    let resp = graphql(
        app,
        &format!(
            r#"mutation {{ updateEvent(treeId: "{tree_id}", id: "{event_id}", input: {{ dateValue: null }}) {{ dateValue dateSort }} }}"#
        ),
        None,
    )
    .await;
    let event = &data(&resp)["updateEvent"];
    assert!(event["dateValue"].is_null());
    assert!(event["dateSort"].is_null());
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

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&resp)["createPerson"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ citationCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["citationCount"], 0);

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
            r#"mutation {{ createCitation(treeId: "{tree_id}", input: {{ sourceId: "{source_id}", personId: "{person_id}", page: "42", confidence: HIGH, text: "entry text" }}) {{ id page confidence text }} }}"#
        ),
        None,
    )
    .await;
    let cit = &data(&resp)["createCitation"];
    assert_eq!(cit["page"], "42");
    assert_eq!(cit["confidence"], "HIGH");
    let citation_id = cit["id"].as_str().unwrap().to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ citationCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["citationCount"], 1);

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
            r#"mutation {{ updateCitation(treeId: "{tree_id}", id: "{citation_id}", input: {{ page: "43" }}) {{ id page }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateCitation"]["page"], "43");

    // Delete citation
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ deleteCitation(treeId: "{tree_id}", id: "{citation_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteCitation"], true);
    let resp = graphql(
        app,
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ citationCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["citationCount"], 0);
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

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ noteCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["noteCount"], 0);

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
            r#"mutation {{ createMediaLink(treeId: "{tree_id}", input: {{ mediaId: "{media_id}", personId: "{person_id}" }}) {{ id mediaId personId }} }}"#
        ),
        None,
    )
    .await;
    let link = &data(&resp)["createMediaLink"];
    assert_eq!(link["mediaId"], media_id);
    let link_id = link["id"].as_str().unwrap().to_string();

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ treeMediaLinks(treeId: "{tree_id}") {{ linkId entityId entityType mediaId fileName mimeType hasThumbnail }} }}"#
        ),
        None,
    )
    .await;
    let tree_links = data(&resp)["treeMediaLinks"].as_array().unwrap();
    assert_eq!(tree_links.len(), 1);
    assert_eq!(tree_links[0]["linkId"], link_id);
    assert_eq!(tree_links[0]["entityId"], person_id);
    assert_eq!(tree_links[0]["entityType"], "person");
    assert_eq!(tree_links[0]["mediaId"], media_id);

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ mediaLinks(treeId: "{tree_id}", mediaId: "{media_id}") {{ id personId }} }}"#
        ),
        None,
    )
    .await;
    let media_links = data(&resp)["mediaLinks"].as_array().unwrap();
    assert_eq!(media_links.len(), 1);
    assert_eq!(media_links[0]["id"], link_id);
    assert_eq!(media_links[0]["personId"], person_id);

    // Update media
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateMedia(treeId: "{tree_id}", id: "{media_id}", input: {{ title: "New Portrait", privacy: PRIVATE }}) {{ id title privacy }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateMedia"]["title"], "New Portrait");
    assert_eq!(data(&resp)["updateMedia"]["privacy"], "PRIVATE");

    // Delete media link
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ deleteMediaLink(treeId: "{tree_id}", id: "{link_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteMediaLink"], true);

    // Delete media
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deleteMedia(treeId: "{tree_id}", id: "{media_id}") }}"#),
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

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ noteCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["noteCount"], 1);

    // Update note
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateNote(treeId: "{tree_id}", id: "{note_id}", input: {{ text: "Updated note" }}) {{ id text }} }}"#
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
        app.clone(),
        &format!(r#"mutation {{ deleteNote(treeId: "{tree_id}", id: "{note_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteNote"], true);
    let resp = graphql(
        app,
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{ noteCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfile"]["noteCount"], 0);
}

#[tokio::test]
async fn notes_and_citations_use_cursor_pagination() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Pagination" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let mut person_ids = Vec::new();
    for _ in 0..2 {
        let resp = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        person_ids.push(
            data(&resp)["createPerson"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let person_id = &person_ids[0];
    let other_person_id = &person_ids[1];

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createSource(treeId: "{tree_id}", input: {{ title: "Pagination source" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let source_id = data(&resp)["createSource"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    for (text, linked_person_id) in [
        ("First note", person_id),
        ("Second note", person_id),
        ("Other note", other_person_id),
    ] {
        graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createNote(treeId: "{tree_id}", input: {{ text: "{text}", personId: "{linked_person_id}" }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
    }
    for (page, linked_person_id) in [("1", person_id), ("2", person_id), ("3", other_person_id)] {
        graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createCitation(treeId: "{tree_id}", input: {{ sourceId: "{source_id}", personId: "{linked_person_id}", page: "{page}", confidence: HIGH }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
    }

    for resource in ["notes", "citations"] {
        let resp = graphql(
            app.clone(),
            &format!(
                r#"{{ {resource}(treeId: "{tree_id}", personId: "{person_id}", first: 1) {{ totalCount edges {{ node {{ id }} }} pageInfo {{ hasNextPage endCursor }} }} }}"#
            ),
            None,
        )
        .await;
        let first_page = &data(&resp)[resource];
        assert_eq!(first_page["totalCount"], 2);
        assert_eq!(first_page["edges"].as_array().unwrap().len(), 1);
        assert_eq!(first_page["pageInfo"]["hasNextPage"], true);
        let first_id = first_page["edges"][0]["node"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let cursor = first_page["pageInfo"]["endCursor"]
            .as_str()
            .unwrap()
            .to_string();

        let resp = graphql(
            app.clone(),
            &format!(
                r#"{{ {resource}(treeId: "{tree_id}", personId: "{person_id}", first: 1, after: "{cursor}") {{ totalCount edges {{ node {{ id }} }} pageInfo {{ hasNextPage }} }} }}"#
            ),
            None,
        )
        .await;
        let second_page = &data(&resp)[resource];
        assert_eq!(second_page["totalCount"], 2);
        assert_eq!(second_page["edges"].as_array().unwrap().len(), 1);
        assert_eq!(second_page["pageInfo"]["hasNextPage"], false);
        assert_ne!(second_page["edges"][0]["node"]["id"], first_id);
    }
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
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "John", surname: "Smith", isPrimary: true }}) {{ id }} }}"#
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
            r#"mutation {{ updatePersonName(treeId: "{tree_id}", personId: "{person_id}", id: "{name_id}", input: {{ surname: "Jones" }}) {{ id surname }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updatePersonName"]["surname"], "Jones");

    // Delete name
    let resp = graphql(
        app,
        &format!(r#"mutation {{ deletePersonName(treeId: "{tree_id}", personId: "{person_id}", id: "{name_id}") }}"#),
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

// ── Geneanet and exports ─────────────────────────────────────────────

/// A one-couple-one-child genealogy in GeneWeb's `.gw` syntax.
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

#[tokio::test]
async fn test_geneanet_wizard_operations_over_graphql() {
    use base64::Engine as _;

    let app = setup_app().await;
    let gw_base64 = base64::engine::general_purpose::STANDARD.encode(minimal_geneweb());
    let collection = r#"{"deposits":[],"references":[],"view_references":{}}"#;
    let collection_graphql = collection.replace('"', "\\\"");
    let inspection = graphql(
        app.clone(),
        &format!(
            r#"{{ inspectGeneweb(gwBase64: "{gw_base64}", fileName: "family.gw") {{ personCount familyCount skippedBlocks }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&inspection)["inspectGeneweb"]["personCount"], 3);
    assert_eq!(data(&inspection)["inspectGeneweb"]["familyCount"], 1);

    let encoded = graphql(
        app.clone(),
        &format!(
            r#"mutation {{
            encodeGeneanetSession(input: {{ collection: "{collection_graphql}", account: "test-account" }}) {{
                archiveBase64
            }}
        }}"#
        ),
        None,
    )
    .await;
    let archive_base64 = data(&encoded)["encodeGeneanetSession"]["archiveBase64"]
        .as_str()
        .unwrap()
        .to_string();
    let decoded = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ decodeGeneanetSession(archiveBase64: "{archive_base64}") {{ collection account photoCount media {{ url path }} }} }}"#
        ),
        None,
    )
    .await;
    let session = &data(&decoded)["decodeGeneanetSession"];
    let restored_collection: Value =
        serde_json::from_str(session["collection"].as_str().unwrap()).unwrap();
    assert_eq!(restored_collection["deposits"], serde_json::json!([]));
    assert_eq!(restored_collection["references"], serde_json::json!([]));
    assert_eq!(
        restored_collection["view_references"],
        serde_json::json!({})
    );
    assert_eq!(session["account"], "test-account");
    assert_eq!(session["photoCount"], 0);
    assert!(session["media"].as_array().unwrap().is_empty());

    let indexed = graphql(
        app.clone(),
        r#"{ indexGeneanetArchives(paths: []) { fileCount archives { path } } }"#,
        None,
    )
    .await;
    assert_eq!(data(&indexed)["indexGeneanetArchives"]["fileCount"], 0);
    assert!(
        data(&indexed)["indexGeneanetArchives"]["archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let rejected = graphql(
        app,
        &format!(
            r#"{{ geneanetPreview(input: {{ gwBase64: "{gw_base64}", fileName: "family.gw", collection: "{collection_graphql}", depositSizes: [{{ depositId: 1, size: -1 }}] }}) {{ personCount }} }}"#
        ),
        None,
    )
    .await;
    assert!(rejected.get("errors").is_some());
}

/// A note whose two lines are one break apart, in each format's own spelling:
/// GEDCOM continues the line with `CONT`, GeneWeb ends it with `<br/>` *and*
/// the newline that follows in the file. The sample Geneanet exports in `samples/`
/// hold the same real note both ways.
///
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
async fn test_graphql_export_gedzip() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join(format!("oxidgene-gql-export-{}", uuid::Uuid::now_v7())),
    );
    let app = build_router(state.clone());
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "GQL GEDZIP Export" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let response = graphql(
        app.clone(),
        &format!(r#"mutation {{ startExportJob(treeId: "{tree_id}") {{ jobId }} }}"#),
        None,
    )
    .await;
    let job_id = data(&response)["startExportJob"]["jobId"].as_str().unwrap();
    let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
        state.db.clone(),
        state.profiles.clone(),
        state.media.clone(),
        "graphql-test",
    );
    assert!(worker.run_once().await.unwrap());

    let response = graphql(
        app,
        &format!(
            r#"{{ exportJobStatus(treeId: "{tree_id}", jobId: "{job_id}") {{ phase downloadUrl warnings error }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["exportJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert_eq!(
        result["downloadUrl"],
        format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download")
    );
    assert!(result["warnings"].as_array().unwrap().is_empty());
    assert!(result["error"].is_null());
}

#[tokio::test]
async fn test_graphql_file_import_job() {
    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join(format!("oxidgene-gql-import-{}", uuid::Uuid::now_v7())),
    );
    let app = build_router(state.clone());
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "GQL Import Job" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v1/trees/{tree_id}/import-jobs?format=gedcom&filename=tree.ged"
        ))
        .body(Body::from(
            "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n0 @I1@ INDI\n1 NAME Alex /Example/\n0 TRLR\n",
        ))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let started: Value = serde_json::from_slice(&body).unwrap();
    let job_id = started["job_id"].as_str().unwrap();
    let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
        state.db.clone(),
        state.profiles.clone(),
        state.media.clone(),
        "graphql-test",
    );
    assert!(worker.run_once().await.unwrap());

    let response = graphql(
        app,
        &format!(
            r#"{{ importJobStatus(treeId: "{tree_id}", jobId: "{job_id}") {{ phase result {{ personsCount }} error }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["importJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert_eq!(result["result"]["personsCount"], 1);
    assert!(result["error"].is_null());
}

#[tokio::test]
async fn test_graphql_geneanet_import_job() {
    use base64::Engine as _;

    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join(format!(
            "oxidgene-gql-geneanet-import-{}",
            uuid::Uuid::now_v7()
        )),
    );
    let app = build_router(state.clone());
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Geneanet Import Job" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let geneweb = "encoding: utf-8\n\nfam BRANCH_A person_a.0 + BRANCH_B person_b.0\n";
    let gw_base64 = base64::engine::general_purpose::STANDARD.encode(geneweb);
    let collection = r#"{\"deposits\":[],\"references\":[],\"details\":[],\"view_references\":{}}"#;
    let response = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ importGeneanet(treeId: "{tree_id}", input: {{ gwBase64: "{gw_base64}", fileName: "family.gw", collection: "{collection}" }}) {{ jobId }} }}"#
        ),
        None,
    )
    .await;
    let job_id = data(&response)["importGeneanet"]["jobId"]
        .as_str()
        .expect("job id");
    let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
        state.db.clone(),
        state.profiles.clone(),
        state.media.clone(),
        "graphql-geneanet-test",
    );
    assert!(worker.run_once().await.expect("run Geneanet import job"));

    let response = graphql(
        app,
        &format!(
            r#"{{ importJobStatus(treeId: "{tree_id}", jobId: "{job_id}") {{ phase result {{ personsCount }} geneanetResult {{ personsCount familiesCount mediaCount }} error }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["importJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert!(result["result"].is_null());
    assert_eq!(result["geneanetResult"]["personsCount"], 2);
    assert_eq!(result["geneanetResult"]["familiesCount"], 1);
    assert_eq!(result["geneanetResult"]["mediaCount"], 0);
    assert!(result["error"].is_null());
}

// ── Projection queries & mutations ───────────────────────────────────

/// GraphQL must expose the same vocabulary as REST (Sprint E.9): `profiles`
/// and `pedigree`, never `cache`. This resolves the whole renamed surface.
#[tokio::test]
async fn test_projection_graphql_surface() {
    let app = setup_app().await;

    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Projection Tree" }) { id } }"#,
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

    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{
                nameType: BIRTH, givenNames: "Jean", surname: "Dupont", isPrimary: true
            }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    // personProfile / personProfiles (were cachedPerson / cachedPersons).
    let resp = graphql(
        app.clone(),
        &format!(
            r#"query {{ personProfile(treeId: "{tree_id}", personId: "{person_id}") {{
                personId primaryName {{ displayName }} builtAt
            }} }}"#
        ),
        None,
    )
    .await;
    let profile = &data(&resp)["personProfile"];
    assert_eq!(profile["personId"], person_id);
    assert_eq!(profile["primaryName"]["displayName"], "Jean Dupont");
    assert!(profile["builtAt"].is_string(), "builtAt (was cachedAt)");

    let resp = graphql(
        app.clone(),
        &format!(r#"query {{ personProfiles(treeId: "{tree_id}") {{ personId }} }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["personProfiles"].as_array().unwrap().len(), 1);

    // pedigree — unchanged name, but must still resolve after the type rename.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"query {{ pedigree(treeId: "{tree_id}", rootPersonId: "{person_id}",
                ancestorDepth: 2, descendantDepth: 1) {{
                rootPersonId ancestorDepthLoaded nodes {{ displayName }}
            }} }}"#
        ),
        None,
    )
    .await;
    let pedigree = &data(&resp)["pedigree"];
    assert_eq!(pedigree["rootPersonId"], person_id);
    assert_eq!(pedigree["ancestorDepthLoaded"], 2);

    // rebuildTreeProfiles / rebuildPersonProfile / dropTreeProfiles.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ rebuildTreeProfiles(treeId: "{tree_id}") {{ rebuilt personsCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["rebuildTreeProfiles"]["personsCount"], 1);

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ rebuildPersonProfile(treeId: "{tree_id}", personId: "{person_id}") {{ rebuilt }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["rebuildPersonProfile"]["rebuilt"], true);

    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ dropTreeProfiles(treeId: "{tree_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["dropTreeProfiles"], true);

    // The old cache-flavoured fields must be gone from the schema.
    for field in [
        format!(
            r#"query {{ cachedPerson(treeId: "{tree_id}", personId: "{person_id}") {{ personId }} }}"#
        ),
        format!(r#"query {{ cachedPersons(treeId: "{tree_id}") {{ personId }} }}"#),
        format!(r#"mutation {{ rebuildTreeCache(treeId: "{tree_id}") {{ rebuilt }} }}"#),
        format!(r#"mutation {{ invalidateTreeCache(treeId: "{tree_id}") }}"#),
    ] {
        let resp = graphql(app.clone(), &field, None).await;
        assert!(
            resp.get("errors").is_some(),
            "field still in schema: {field}"
        );
    }
}

/// GraphQL must be able to clear a nullable field, like REST can.
///
/// Its inputs used to be plain `Option<T>`, which collapses an omitted field
/// and an explicit `null` into the same `None` — so a field could be set but
/// never cleared, and the mutation reported success either way.
#[tokio::test]
async fn test_update_can_clear_a_nullable_field() {
    let app = setup_app().await;

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

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "Jean", surname: "MARTIN", surnamePrefix: "de", nickname: "Jeannot", isPrimary: true }}) {{ id surnamePrefix }} }}"#
        ),
        None,
    )
    .await;
    let name = &data(&resp)["addPersonName"];
    assert_eq!(name["surnamePrefix"], "de");
    let name_id = name["id"].as_str().unwrap().to_string();

    // An explicit null clears the particle...
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updatePersonName(treeId: "{tree_id}", personId: "{person_id}", id: "{name_id}", input: {{ surname: "MARTIN", surnamePrefix: null }}) {{ surname surnamePrefix nickname }} }}"#
        ),
        None,
    )
    .await;
    let name = &data(&resp)["updatePersonName"];
    assert!(
        name["surnamePrefix"].is_null(),
        "an explicit null must clear the particle, got {}",
        name["surnamePrefix"]
    );
    // ...while an omitted field still means "leave unchanged".
    assert_eq!(name["nickname"], "Jeannot");
    assert_eq!(name["surname"], "MARTIN");
}

// ── Media & vignettes (Sprint F.1) ───────────────────────────────────

/// A media directory that removes itself when the test ends.
struct TempMediaRoot(std::path::PathBuf);

impl Drop for TempMediaRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A router whose media store is its own throwaway directory.
///
/// The shared `setup_app` root is fine for the tests that never write; these
/// do, and one test's uploads must not be another's.
async fn setup_app_with_media() -> (axum::Router, TempMediaRoot) {
    let db = setup_db().await;
    let root = TempMediaRoot(
        std::env::temp_dir().join(format!("oxidgene-gql-media-{}", uuid::Uuid::now_v7())),
    );
    std::fs::create_dir_all(&root.0).expect("create media root");
    (build_router(AppState::new(db, &root.0)), root)
}

/// A small PNG, base64-encoded, as `uploadMediaFile` wants it.
fn png_base64(width: u32, height: u32) -> String {
    use base64::Engine as _;
    let img = image::RgbImage::new(width, height);
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(out.into_inner())
}

async fn tree_id_for(app: &axum::Router) -> String {
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Media tree" }) { id } }"#,
        None,
    )
    .await;
    data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_upload_media_file_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 fileName: "portrait.png",
                 contentBase64: "{}",
                 title: "A portrait"
               }}) {{ id fileName mimeType width height pageCount sha256 storageKey thumbnailKey title }} }}"#,
            png_base64(640, 480)
        ),
        None,
    )
    .await;

    let media = &data(&resp)["uploadMediaFile"];
    assert_eq!(media["fileName"], "portrait.png");
    assert_eq!(media["mimeType"], "image/png");
    assert_eq!(media["width"], 640);
    assert_eq!(media["height"], 480);
    assert_eq!(media["pageCount"], 1);
    assert_eq!(media["title"], "A portrait");
    assert_eq!(media["sha256"].as_str().unwrap().len(), 64);
    assert!(media["storageKey"].is_string());
    assert!(media["thumbnailKey"].is_string());
}

#[tokio::test]
async fn detaching_a_page_over_graphql_removes_its_relations() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;

    let person = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&person)["createPerson"]["id"].as_str().unwrap();
    let document = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createMediaDocument(treeId: "{tree_id}", title: "Register") {{ id }} }}"#
        ),
        None,
    )
    .await;
    let document_id = data(&document)["createMediaDocument"]["id"]
        .as_str()
        .unwrap();
    let page = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 fileName: "page.png", contentBase64: "{}"
               }}) {{ id }} }}"#,
            png_base64(300, 400)
        ),
        None,
    )
    .await;
    let page_id = data(&page)["uploadMediaFile"]["id"].as_str().unwrap();
    data(&graphql(
        app.clone(),
        &format!(
            r#"mutation {{ appendMediaPage(treeId: "{tree_id}", documentId: "{document_id}", mediaId: "{page_id}") {{ id }} }}"#
        ),
        None,
    )
    .await);
    data(&graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createMediaLink(treeId: "{tree_id}", input: {{ mediaId: "{page_id}", personId: "{person_id}" }}) {{ id }} }}"#
        ),
        None,
    )
    .await);
    let vignette = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createVignette(treeId: "{tree_id}", input: {{
                 mediaId: "{page_id}", personId: "{person_id}", x: 10, y: 10, width: 50, height: 60
               }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let vignette_id = data(&vignette)["createVignette"]["id"].as_str().unwrap();
    data(&graphql(
        app.clone(),
        &format!(
            r#"mutation {{ setPersonPortrait(treeId: "{tree_id}", personId: "{person_id}", vignetteId: "{vignette_id}") {{ id }} }}"#
        ),
        None,
    )
    .await);

    let detached = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ detachMediaPage(treeId: "{tree_id}", documentId: "{document_id}", pageId: "{page_id}") {{ id parentMediaId }} }}"#
        ),
        None,
    )
    .await;
    assert!(data(&detached)["detachMediaPage"]["parentMediaId"].is_null());

    let result = graphql(
        app,
        &format!(
            r#"{{
                mediaLinks(treeId: "{tree_id}", mediaId: "{page_id}") {{ id }}
                vignettes(treeId: "{tree_id}", personId: "{person_id}") {{ id }}
                person(treeId: "{tree_id}", id: "{person_id}") {{ portraitMediaId portraitVignetteId }}
            }}"#
        ),
        None,
    )
    .await;
    let result = data(&result);
    assert!(result["mediaLinks"].as_array().unwrap().is_empty());
    assert!(result["vignettes"].as_array().unwrap().is_empty());
    assert!(result["person"]["portraitMediaId"].is_null());
    assert!(result["person"]["portraitVignetteId"].is_null());
}

#[tokio::test]
async fn test_upload_media_file_rejects_content_that_is_not_base64() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 fileName: "x.png", contentBase64: "not base64 at all!!"
               }}) {{ id }} }}"#
        ),
        None,
    )
    .await;

    let error = &resp["errors"][0];
    assert_eq!(error["message"], "The request is invalid");
    assert_eq!(error["extensions"]["code"], "VALIDATION_ERROR");
    assert!(error["extensions"].get("requestId").is_none());
}

#[tokio::test]
async fn test_vignette_lifecycle_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 fileName: "register.png", contentBase64: "{}"
               }}) {{ id }} }}"#,
            png_base64(800, 600)
        ),
        None,
    )
    .await;
    let media_id = data(&resp)["uploadMediaFile"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createVignette(treeId: "{tree_id}", input: {{
                                 mediaId: "{media_id}", x: 10, y: 20, width: 200, height: 150
                             }}) {{ id x y width height page }} }}"#
        ),
        None,
    )
    .await;
    let vignette = &data(&resp)["createVignette"];
    assert_eq!(vignette["x"], 10);
    assert_eq!(vignette["page"], 0);
    let vignette_id = vignette["id"].as_str().unwrap().to_string();

    // Query by media
    let resp = graphql(
        app.clone(),
        &format!(r#"{{ mediaVignettes(treeId: "{tree_id}", mediaId: "{media_id}") {{ id }} }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["mediaVignettes"].as_array().unwrap().len(), 1);

    // Move it
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateVignette(treeId: "{tree_id}", id: "{vignette_id}", input: {{
                 x: 100, y: 100, width: 300, height: 200
             }}) {{ x y width height }} }}"#
        ),
        None,
    )
    .await;
    let moved = &data(&resp)["updateVignette"];
    assert_eq!(moved["x"], 100);

    // Off the edge
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateVignette(treeId: "{tree_id}", id: "{vignette_id}", input: {{
                 x: 700, y: 500, width: 300, height: 200
               }}) {{ x }} }}"#
        ),
        None,
    )
    .await;
    assert!(
        resp.get("errors").is_some(),
        "a crop leaving the scan must be refused: {resp}"
    );

    // Delete
    let resp = graphql(
        app.clone(),
        &format!(r#"mutation {{ deleteVignette(treeId: "{tree_id}", id: "{vignette_id}") }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["deleteVignette"], true);

    let resp = graphql(
        app.clone(),
        &format!(r#"{{ vignette(treeId: "{tree_id}", id: "{vignette_id}") {{ id }} }}"#),
        None,
    )
    .await;
    assert!(data(&resp)["vignette"].is_null());
}
