//! Integration tests for GraphQL API.
//!
//! All tests run against a fresh SQLite database: a file with its read pool
//! (`setup_app`) or an in-memory one (`setup_db`). Requests are sent to
//! `POST /graphql` via Axum's tower `ServiceExt::oneshot`.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use serde_json::{Value, json};
use tower::ServiceExt;

use common::{send, setup_app, setup_db};

/// Helper: send a GraphQL query/mutation and return the full JSON response.
async fn graphql(app: axum::Router, query: &str, variables: Option<Value>) -> Value {
    let body = match variables {
        Some(vars) => json!({ "query": query, "variables": vars }),
        None => json!({ "query": query }),
    };
    let (status, response) = send(&app, Method::POST, "/graphql", Some(body)).await;
    assert_eq!(status, StatusCode::OK, "GraphQL query: {query}");
    response
}

#[tokio::test]
async fn given_name_reference_batch_matches_rest_bounds() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"{
            givenNameReferences(
                language: "fr"
                terms: ["Jean", "Marie", "Jean", "__unknown__"]
            ) { term reference { label } }
        }"#,
        None,
    )
    .await;
    let response = data(&response);
    assert_eq!(response["givenNameReferences"].as_array().unwrap().len(), 2);
    assert_eq!(response["givenNameReferences"][0]["term"], "Jean");
    assert_eq!(response["givenNameReferences"][1]["term"], "Marie");

    let terms = (0..129)
        .map(|index| format!("\"{index}\""))
        .collect::<Vec<_>>()
        .join(",");
    let response = graphql(
        app,
        &format!("{{ givenNameReferences(language: \"fr\", terms: [{terms}]) {{ term }} }}"),
        None,
    )
    .await;
    assert!(
        response["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "oversized batch should be rejected: {response}"
    );
}

#[tokio::test]
async fn place_suggestions_match_rest() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"{
            placeSuggestions(language: "en", query: "paris", limit: 3) {
                label code kind current
            }
        }"#,
        None,
    )
    .await;
    let places = data(&response)["placeSuggestions"].clone();
    assert_eq!(places.as_array().unwrap().len(), 3);
    assert_eq!(
        places[0]["label"],
        "Paris, 75056, Paris, Île-de-France, France"
    );
    assert_eq!(places[0]["kind"], "COMMUNE");
    assert_eq!(places[0]["current"], true);

    let response = graphql(
        app.clone(),
        r#"{ placeSuggestions(language: "pl", query: "paris", limit: 1) { country } }"#,
        None,
    )
    .await;
    assert_eq!(data(&response)["placeSuggestions"][0]["country"], "Francja");

    for query in [
        r#"{ placeSuggestions(language: "fr", query: "paris", limit: 51) { label } }"#,
        r#"{ placeSuggestions(language: "fr", query: "paris", limit: 0) { label } }"#,
        r#"{ placeSuggestions(language: "xx", query: "paris") { label } }"#,
    ] {
        let response = graphql(app.clone(), query, None).await;
        assert!(
            response["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty()),
            "should be rejected: {query}"
        );
    }
}

#[tokio::test]
async fn occupation_reference_batch_matches_rest_bounds() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"{
            occupationReferences(
                language: "fr"
                terms: ["Laboureur", "Forgeron", "Laboureur", "__unknown__"]
            ) { term reference { label } }
        }"#,
        None,
    )
    .await;
    let response = data(&response);
    assert_eq!(
        response["occupationReferences"].as_array().unwrap().len(),
        2
    );
    assert_eq!(response["occupationReferences"][0]["term"], "Laboureur");
    assert_eq!(
        response["occupationReferences"][0]["reference"]["label"],
        "Laboureur"
    );
    assert_eq!(response["occupationReferences"][1]["term"], "Forgeron");

    let terms = (0..129)
        .map(|index| format!("\"{index}\""))
        .collect::<Vec<_>>()
        .join(",");
    let response = graphql(
        app,
        &format!("{{ occupationReferences(language: \"fr\", terms: [{terms}]) {{ term }} }}"),
        None,
    )
    .await;
    assert!(
        response["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "oversized batch should be rejected: {response}"
    );
}

#[tokio::test]
async fn relation_labels_query_is_bounded() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Relation labels" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"].as_str().unwrap();
    let ids = (0..1_025)
        .map(|_| format!("\"{}\"", uuid::Uuid::now_v7()))
        .collect::<Vec<_>>()
        .join(",");

    let response = graphql(
        app,
        &format!(
            "{{ relationLabels(treeId: \"{tree_id}\", personIds: [{ids}], familyIds: []) {{ names {{ id }} }} }}"
        ),
        None,
    )
    .await;
    assert!(
        response["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "oversized batch should be rejected: {response}"
    );
}

/// Helper: extract `data` field from a GraphQL response, panicking on errors.
fn data(resp: &Value) -> &Value {
    if let Some(errors) = resp.get("errors") {
        panic!("GraphQL errors: {errors}");
    }
    resp.get("data").expect("missing 'data' in response")
}

/// The nested fields of a page, read in one batch for all of its records,
/// answer exactly what each record answers read alone — the same records,
/// in the same order, with the same filters — on a tree holding a record of
/// every kind.
#[tokio::test]
async fn a_page_nests_what_each_record_nests_alone() {
    let db = setup_db().await;
    let app = common::app_on(db.clone());
    let tree = common::populated::populated_tree(&app, &db, "Batches", 2).await;
    let cases = [
        (
            "persons",
            "person",
            "id names { id } primaryName { id } events { id place { id } } families { id } \
             citations { id } media { id } notes { id }",
        ),
        (
            "families",
            "family",
            "id spouses { id person { id names { id } } } children { id person { id } } \
             events { id }",
        ),
        (
            "events",
            "event",
            "id place { id } person { id } family { id } citations { id } media { id } \
             notes { id } witnesses { id person { id } }",
        ),
        (
            "sources",
            "source",
            "id citations { id } repositories { id repository { id } source { id } }",
        ),
        (
            "repositories",
            "repository",
            "id sources { id repository { id } source { id } }",
        ),
    ];
    let variables = |id: &Value| json!({ "t": tree.tree_id, "id": id });
    for (list, single, selection) in cases {
        let page = common::gql_ok(
            &app,
            &format!(
                "query($t: ID!) {{ {list}(treeId: $t, first: 100) {{ edges {{ node {{ {selection} }} }} }} }}"
            ),
            variables(&Value::Null),
        )
        .await;
        let edges = page[list]["edges"].as_array().unwrap();
        assert!(!edges.is_empty(), "the fixture holds no {list}");
        for node in edges.iter().map(|edge| &edge["node"]) {
            let alone = common::gql_ok(
                &app,
                &format!(
                    "query($t: ID!, $id: ID!) {{ {single}(treeId: $t, id: $id) {{ {selection} }} }}"
                ),
                variables(&node["id"]),
            )
            .await;
            assert_eq!(&alone[single], node, "{single} read alone");
        }
    }

    // A tree list's counts are read for every tree at once.
    let other = common::new_tree(&app, "Empty").await;
    let trees = common::gql_ok(
        &app,
        "{ trees(first: 100) { edges { node { id personCount familyCount } } } }",
        json!({}),
    )
    .await;
    for node in trees["trees"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| &edge["node"])
    {
        let alone = common::gql_ok(
            &app,
            "query($id: ID!) { tree(id: $id) { id personCount familyCount } }",
            json!({ "id": node["id"] }),
        )
        .await;
        assert_eq!(&alone["tree"], node);
        if node["id"] == other.as_str() {
            assert_eq!(node["personCount"], 0);
        } else {
            assert!(node["personCount"].as_i64().unwrap() > 0, "{node}");
        }
    }
}

#[tokio::test]
async fn person_detail_bundle_query_excludes_unrelated_person_citations() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Scoped detail" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"].as_str().unwrap();

    let mut person_ids = Vec::new();
    for _ in 0..2 {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        person_ids.push(
            data(&response)["createPerson"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }

    let mut source_ids = Vec::new();
    for title in ["Relevant register", "Unrelated register"] {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createSource(treeId: "{tree_id}", input: {{ title: "{title}" }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        source_ids.push(
            data(&response)["createSource"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }

    for index in 0..2 {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createCitation(treeId: "{tree_id}", input: {{ sourceId: "{}", personId: "{}", confidence: HIGH }}) {{ id }} }}"#,
                source_ids[index], person_ids[index]
            ),
            None,
        )
        .await;
        assert!(data(&response)["createCitation"]["id"].is_string());
    }

    let response = graphql(
        app,
        &format!(
            r#"query {{ personDetailBundle(treeId: "{tree_id}", personId: "{}") {{ persons {{ id }} citations {{ sourceId }} sources {{ id }} profileMedia {{ linkId media {{ id }} }} profileVignettes {{ id }} }} }}"#,
            person_ids[0]
        ),
        None,
    )
    .await;
    let bundle = &data(&response)["personDetailBundle"];
    assert_eq!(bundle["persons"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["persons"][0]["id"], person_ids[0]);
    assert_eq!(bundle["citations"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["citations"][0]["sourceId"], source_ids[0]);
    assert_eq!(bundle["sources"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["sources"][0]["id"], source_ids[0]);
    assert_eq!(bundle["profileMedia"], serde_json::json!([]));
    assert_eq!(bundle["profileVignettes"], serde_json::json!([]));
}

#[tokio::test]
async fn person_detail_bundle_query_names_the_couple_a_profile_media_comes_from() {
    let app = setup_app().await;
    let tree_id = tree_id_for(&app).await;
    let mut spouse_ids = Vec::new();
    for sex in ["MALE", "FEMALE"] {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: {sex} }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        spouse_ids.push(
            data(&response)["createPerson"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let response = graphql(
        app.clone(),
        &format!(r#"mutation {{ createFamily(treeId: "{tree_id}") {{ id }} }}"#),
        None,
    )
    .await;
    let family_id = data(&response)["createFamily"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (spouse_id, role) in [(&spouse_ids[0], "HUSBAND"), (&spouse_ids[1], "WIFE")] {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{spouse_id}", role: {role} }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        assert!(data(&response)["addSpouse"]["id"].is_string());
    }

    let own_document = document_id_for(&app, &tree_id).await;
    let couple_document = document_id_for(&app, &tree_id).await;
    for owner in [
        format!(r#"personId: "{}""#, spouse_ids[0]),
        format!(r#"familyId: "{family_id}""#),
    ] {
        let media_id = if owner.starts_with("person") {
            &own_document
        } else {
            &couple_document
        };
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createMediaLink(treeId: "{tree_id}", input: {{ mediaId: "{media_id}", {owner} }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        assert!(data(&response)["createMediaLink"]["id"].is_string());
    }

    let response = graphql(
        app,
        &format!(
            r#"query {{ personDetailBundle(treeId: "{tree_id}", personId: "{}") {{ profileMedia {{ familyId media {{ id }} }} }} }}"#,
            spouse_ids[0]
        ),
        None,
    )
    .await;
    let tiles = data(&response)["personDetailBundle"]["profileMedia"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(tiles.len(), 2);
    for tile in tiles {
        let expected = if tile["media"]["id"] == own_document.as_str() {
            serde_json::Value::Null
        } else {
            serde_json::json!(family_id)
        };
        assert_eq!(tile["familyId"], expected, "{tile}");
    }
}

#[tokio::test]
async fn graphql_geneanet_local_paths_are_refused_by_default() {
    let response = graphql(
        setup_app().await,
        r#"{ indexGeneanetArchives(paths: ["/does/not/exist"]) { fileCount } }"#,
        None,
    )
    .await;

    assert!(response["data"].is_null());
    assert_eq!(
        response["errors"][0]["extensions"]["code"],
        "VALIDATION_ERROR"
    );
    assert_eq!(response["errors"][0]["message"], "The request is invalid");
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

    // Entry suggestions start on, and the tree can turn them off.
    let resp = graphql(
        app.clone(),
        &format!(r#"{{ tree(id: "{tree_id}") {{ entrySuggestions }} }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["tree"]["entrySuggestions"], true);
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateTree(id: "{tree_id}", input: {{ entrySuggestions: false }}) {{ name entrySuggestions }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["updateTree"]["entrySuggestions"], false);
    assert_eq!(data(&resp)["updateTree"]["name"], "New Name");

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
async fn a_stored_file_keeps_its_sniffed_type_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;
    let media_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "scan.png", contentBase64: "{}" }}) {{ id }} }}"#,
                png_base64(4, 4)
            ),
            None,
        )
        .await,
    )["uploadMediaFile"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = graphql(
        app,
        &format!(
            r#"mutation {{ updateMedia(treeId: "{tree_id}", id: "{media_id}", input: {{ mimeType: "text/html" }}) {{ mimeType }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(resp["errors"][0]["extensions"]["code"], "VALIDATION_ERROR");
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
async fn graphql_rejects_queries_over_the_complexity_limit() {
    let app = setup_app().await;
    let selections = (0..1_001)
        .map(|index| format!("trees{index}: trees {{ totalCount }}"))
        .collect::<Vec<_>>()
        .join(" ");
    let response = graphql(app, &format!("query {{ {selections} }}"), None).await;

    assert!(response["data"].is_null());
    let error = &response["errors"][0];
    assert_eq!(error["message"], "The request is invalid");
    assert_eq!(error["extensions"]["code"], "VALIDATION_ERROR");
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
    // A duplicate goes through GEDCOM: what it holds must survive the trip.
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{source_tree_id}", input: {{ eventType: DEATH, personId: "{person_id}", age: "< 1y 6m", agency: "Parish of Northwick" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createSource(treeId: "{source_tree_id}", input: {{ title: "Register", agency: "Sample archives" }}) {{ id }} }}"#
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
    let copied = data(
        &graphql(
            app.clone(),
            &format!(
                r#"{{ events(treeId: "{copied_tree_id}", eventType: DEATH) {{ edges {{ node {{ age agency }} }} }} sources(treeId: "{copied_tree_id}") {{ edges {{ node {{ agency }} }} }} }}"#
            ),
            None,
        )
        .await,
    )
    .clone();
    assert_eq!(copied["events"]["edges"][0]["node"]["age"], "< 1y 6m");
    assert_eq!(
        copied["events"]["edges"][0]["node"]["agency"],
        "Parish of Northwick"
    );
    assert_eq!(
        copied["sources"]["edges"][0]["node"]["agency"],
        "Sample archives"
    );
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

    let document_id = document_id_for(&app, &tree_id).await;
    let media_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "portrait.png", contentBase64: "{}" }}) {{ id }} }}"#,
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
            app.clone(),
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

    let images = data(
        &graphql(
            app.clone(),
            &format!(
                r#"{{ portraitImages(treeId: "{tree_id}", personIds: ["{person_id}"]) {{ personId source {{ kind mediaId }} }} }}"#
            ),
            None,
        )
        .await,
    )["portraitImages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0]["personId"], person_id);
    // A picture we hold names the resource that serves it, never a URL and
    // never the bytes: the client decides how to draw it.
    assert_eq!(images[0]["source"]["kind"], "THUMBNAIL", "{images:?}");
    assert_eq!(images[0]["source"]["mediaId"], media_id, "{images:?}");

    let bundle = data(
        &graphql(
            app,
            &format!(
                r#"{{ galleryBundle(treeId: "{tree_id}", mediaIds: ["{media_id}"], vignetteIds: []) {{ media {{ mediaId source {{ kind mediaId }} eventIds documentPreviews {{ kind }} }} vignettes {{ vignetteId source {{ kind }} }} }} }}"#
            ),
            None,
        )
        .await,
    )["galleryBundle"]
        .clone();
    assert_eq!(bundle["media"].as_array().unwrap().len(), 1);
    assert_eq!(bundle["media"][0]["mediaId"], media_id);
    assert_eq!(
        bundle["media"][0]["source"]["kind"], "THUMBNAIL",
        "{bundle}"
    );
    assert_eq!(
        bundle["media"][0]["source"]["mediaId"], media_id,
        "{bundle}"
    );
}

/// A portrait names a media of the person's own tree; the REST twin is in
/// `media_test.rs`.
#[tokio::test]
async fn a_portrait_is_a_media_of_the_persons_own_tree_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;
    let other_tree = tree_id_for(&app).await;
    let stranger = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createPerson(treeId: "{other_tree}", input: {{ sex: MALE }}) {{ id }} }}"#
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
        &format!(
            r#"mutation {{ setPersonPortrait(treeId: "{other_tree}", personId: "{stranger}", mediaId: "{document_id}") {{ id }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(
        response["errors"][0]["extensions"]["code"], "NOT_FOUND",
        "{response}"
    );
}

#[tokio::test]
async fn a_remote_portrait_is_drawn_and_chosen_through_its_document_over_graphql() {
    // The REST twin of this lives in `media_test.rs`. Both surfaces have to
    // answer the same two questions about a photograph we do not hold: what a
    // gallery tile draws for it, and what it resolves to once somebody makes
    // it a person's portrait.
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
    let document_id = document_id_for(&app, &tree_id).await;
    let url = "https://archives.example.invalid/scan/42.jpg";
    let page_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMedia(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "42.jpg", mimeType: "image/jpeg", filePath: "{url}", fileSize: 0 }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["uploadMedia"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ setPersonPortrait(treeId: "{tree_id}", personId: "{person_id}", mediaId: "{document_id}") {{ id }} }}"#
        ),
        None,
    )
    .await;

    let bundle = data(
        &graphql(
            app.clone(),
            &format!(
                r#"{{ galleryBundle(treeId: "{tree_id}", mediaIds: ["{document_id}"], vignetteIds: []) {{ media {{ mediaId source {{ kind }} documentPreviews {{ kind url }} }} }} }}"#
            ),
            None,
        )
        .await,
    )["galleryBundle"]
        .clone();
    assert_eq!(
        bundle["media"][0]["documentPreviews"],
        serde_json::json!([{ "kind": "REMOTE", "url": url }]),
        "the browser draws it from its own address: {bundle}"
    );
    assert!(bundle["media"][0]["source"].is_null(), "{bundle}");

    let portraits = data(
        &graphql(
            app.clone(),
            &format!(r#"{{ portraits(treeId: "{tree_id}") {{ mediaId filePath hasThumbnail }} }}"#),
            None,
        )
        .await,
    )["portraits"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(portraits.len(), 1);
    assert_eq!(
        portraits[0]["mediaId"], page_id,
        "the chosen document resolves to the page that holds the file"
    );
    assert_eq!(portraits[0]["filePath"], url);
    assert_eq!(portraits[0]["hasThumbnail"], false);

    let images = data(
        &graphql(
            app,
            &format!(
                r#"{{ portraitImages(treeId: "{tree_id}", personIds: ["{person_id}"]) {{ source {{ kind url }} }} }}"#
            ),
            None,
        )
        .await,
    )["portraitImages"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(images.len(), 1);
    assert_eq!(
        images[0]["source"],
        serde_json::json!({ "kind": "REMOTE", "url": url }),
        "we never fetch it: the card is given the address"
    );
}

/// The REST twin lives in `media_test.rs`. Both surfaces resolve a whole
/// screen's pictures in one operation, in request order, and inline only the
/// ones we hold.
#[tokio::test]
async fn image_sources_resolve_to_inline_data_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;
    let media_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "scan.png", contentBase64: "{}" }}) {{ id }} }}"#,
                png_base64(12, 10)
            ),
            None,
        )
        .await,
    )["uploadMediaFile"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let slots = data(
        &graphql(
            app,
            &format!(
                r#"{{ imageData(treeId: "{tree_id}", sources: [
                    {{ kind: THUMBNAIL, mediaId: "{media_id}" }},
                    {{ kind: REMOTE, url: "https://archives.example.invalid/7.jpg" }}
                ]) }}"#
            ),
            None,
        )
        .await,
    )["imageData"]
        .as_array()
        .unwrap()
        .clone();

    assert_eq!(slots.len(), 2, "one slot per source, in order: {slots:?}");
    assert!(
        slots[0]
            .as_str()
            .is_some_and(|s| s.starts_with("data:image/")),
        "a held picture is inlined: {slots:?}"
    );
    assert!(
        slots[1].is_null(),
        "we never proxy somebody else's file: {slots:?}"
    );
}

#[tokio::test]
async fn a_region_of_a_remote_page_carries_its_rectangle_over_graphql() {
    // The REST twin lives in `media_test.rs`. Both surfaces have to hand a
    // client the same two things about a face identified on a photograph we do
    // not hold: the picture's address, and the rectangle to take out of it.
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
    let document_id = document_id_for(&app, &tree_id).await;
    let url = "https://archives.example.invalid/group/7.jpg";
    let page_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ uploadMedia(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "7.jpg", mimeType: "image/jpeg", filePath: "{url}", fileSize: 0 }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["uploadMedia"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // The size only a browser could know, recorded so the region can be placed.
    let sized = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ updateMedia(treeId: "{tree_id}", id: "{page_id}", input: {{ width: 1600, height: 1200 }}) {{ width height }} }}"#
            ),
            None,
        )
        .await,
    )["updateMedia"]
        .clone();
    assert_eq!(sized["width"], 1600);
    assert_eq!(sized["height"], 1200);

    let vignette_id = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{ createVignette(treeId: "{tree_id}", input: {{ mediaId: "{page_id}", personId: "{person_id}", x: 120, y: 40, width: 200, height: 260 }}) {{ id }} }}"#
            ),
            None,
        )
        .await,
    )["createVignette"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let bundle = data(
        &graphql(
            app.clone(),
            &format!(
                r#"{{ galleryBundle(treeId: "{tree_id}", mediaIds: [], vignetteIds: ["{vignette_id}"]) {{ vignettes {{ source {{ kind url }} crop {{ x y width height sourceWidth sourceHeight }} }} }} }}"#
            ),
            None,
        )
        .await,
    )["galleryBundle"]
        .clone();
    assert_eq!(
        bundle["vignettes"][0]["source"],
        serde_json::json!({ "kind": "REMOTE", "url": url }),
        "{bundle}"
    );
    assert_eq!(
        bundle["vignettes"][0]["crop"],
        serde_json::json!({"x": 120, "y": 40, "width": 200, "height": 260,
                           "sourceWidth": 1600, "sourceHeight": 1200}),
        "{bundle}"
    );

    graphql(
        app.clone(),
        &format!(
            r#"mutation {{ setPersonPortrait(treeId: "{tree_id}", personId: "{person_id}", vignetteId: "{vignette_id}") {{ id }} }}"#
        ),
        None,
    )
    .await;
    let images = data(
        &graphql(
            app,
            &format!(
                r#"{{ portraitImages(treeId: "{tree_id}", personIds: ["{person_id}"]) {{ source {{ kind url }} crop {{ width sourceWidth }} }} }}"#
            ),
            None,
        )
        .await,
    )["portraitImages"]
        .clone();
    assert_eq!(
        images[0]["source"],
        serde_json::json!({ "kind": "REMOTE", "url": url }),
        "{images}"
    );
    assert_eq!(images[0]["crop"]["width"], 200, "{images}");
    assert_eq!(images[0]["crop"]["sourceWidth"], 1600, "{images}");
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

/// Mirrors the REST typed-filter test: an enum, a boolean and an ordering
/// select the same persons, and an oversized page is capped rather than
/// refused.
#[tokio::test]
async fn test_search_persons_reads_typed_filters() {
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

    for (sex, given_names) in [("MALE", "Alpha"), ("FEMALE", "Beta"), ("FEMALE", "Gamma")] {
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
                    r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "{given_names}", surname: "Sample", isPrimary: true }}) {{ id }} }}"#
                ),
                None,
            )
            .await,
        );
    }

    let response = graphql(
        app.clone(),
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "sample", sex: FEMALE, hasMedia: false, sort: NAME_DESC, limit: 500) {{ totalCount entries {{ displayName }} }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["searchPersons"];
    assert_eq!(result["totalCount"], 2);
    assert_eq!(result["entries"][0]["displayName"], "Gamma Sample");
    assert_eq!(result["entries"][1]["displayName"], "Beta Sample");

    let response = graphql(
        app,
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "", surname: "sample", hasMedia: true) {{ totalCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&response)["searchPersons"]["totalCount"], 0);
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
        app.clone(),
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "", surname: "subject", spouseSurname: "relative") {{ totalCount entries {{ displayName }} }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["searchPersons"];
    assert_eq!(result["totalCount"], 1);
    assert_eq!(result["entries"][0]["displayName"], "SearchSubject Subject");

    // An accent in the filter must not change the answer: the relative names
    // on the search row are accent-folded, like the subject's own.
    let response = graphql(
        app.clone(),
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "", spouseSurname: "relativemátch") {{ totalCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&response)["searchPersons"]["totalCount"], 1);

    // The relatives ride on the result, so a caller needs no second request.
    let response = graphql(
        app,
        &format!(
            r#"{{ searchPersons(treeId: "{tree_id}", query: "", surname: "subject") {{ entries {{ spouseNames fatherName motherName childrenCount birthQualifier }} }} }}"#
        ),
        None,
    )
    .await;
    let entry = &data(&response)["searchPersons"]["entries"][0];
    assert_eq!(entry["spouseNames"][0], "RelatedPerson RelativeMatch");
    assert!(entry["fatherName"].is_null());
    assert!(entry["motherName"].is_null());
    assert_eq!(entry["childrenCount"], 0);
    assert_eq!(entry["birthQualifier"], "EXACT");
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

// ── Homonyms and merging ─────────────────────────────────────────────

async fn gql_tree(app: &axum::Router) -> String {
    let resp = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Homonyms" }) { id } }"#,
        None,
    )
    .await;
    data(&resp)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Create a person with a primary birth name; returns their ID.
async fn gql_named_person(
    app: &axum::Router,
    tree_id: &str,
    sex: &str,
    given_names: &str,
    surname: &str,
) -> String {
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: {sex} }}) {{ id }} }}"#
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
        r#"mutation($tree: ID!, $person: ID!, $given: String!, $surname: String!) {
            addPersonName(treeId: $tree, personId: $person, input: {
                nameType: BIRTH, givenNames: $given, surname: $surname, isPrimary: true
            }) { id }
        }"#,
        Some(json!({
            "tree": tree_id, "person": person_id, "given": given_names, "surname": surname
        })),
    )
    .await;
    data(&resp);
    person_id
}

/// Create a family and link its members; returns the family ID.
async fn gql_family(
    app: &axum::Router,
    tree_id: &str,
    spouses: &[(&str, &str)],
    children: &[&str],
) -> String {
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
    for (person_id, role) in spouses {
        let resp = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{person_id}", role: {role} }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        data(&resp);
    }
    for person_id in children {
        let resp = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ addChild(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{person_id}", childType: BIOLOGICAL }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        data(&resp);
    }
    family_id
}

async fn gql_homonym_ids(app: &axum::Router, tree_id: &str, person_id: &str) -> Vec<String> {
    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personHomonyms(treeId: "{tree_id}", personId: "{person_id}") {{ personId surname givenNames }} }}"#
        ),
        None,
    )
    .await;
    data(&resp)["personHomonyms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["personId"].as_str().unwrap().to_string())
        .collect()
}

async fn gql_mark_distinct(
    app: &axum::Router,
    tree_id: &str,
    person_id: &str,
    others: &[&str],
) -> Value {
    graphql(
        app.clone(),
        r#"mutation($tree: ID!, $person: ID!, $others: [ID!]!) {
            markPersonsDistinct(treeId: $tree, personId: $person, otherPersonIds: $others)
        }"#,
        Some(json!({ "tree": tree_id, "person": person_id, "others": others })),
    )
    .await
}

async fn gql_merge(app: &axum::Router, tree_id: &str, kept: &str, duplicate: &str) -> Value {
    graphql(
        app.clone(),
        r#"mutation($tree: ID!, $kept: ID!, $duplicate: ID!) {
            mergePersons(treeId: $tree, personId: $kept, duplicateId: $duplicate) { id sex }
        }"#,
        Some(json!({ "tree": tree_id, "kept": kept, "duplicate": duplicate })),
    )
    .await
}

fn error_code(resp: &Value) -> &Value {
    &resp["errors"][0]["extensions"]["code"]
}

#[tokio::test]
async fn graphql_homonyms_are_listed_until_confirmed_distinct() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let first = gql_named_person(&app, &tree_id, "FEMALE", "Élise", "Sample").await;
    let second = gql_named_person(&app, &tree_id, "FEMALE", "elise", "SAMPLE").await;
    gql_named_person(&app, &tree_id, "FEMALE", "Élise", "Other").await;

    assert_eq!(
        gql_homonym_ids(&app, &tree_id, &first).await,
        vec![second.clone()]
    );
    assert_eq!(
        gql_homonym_ids(&app, &tree_id, &second).await,
        vec![first.clone()]
    );

    for _ in 0..2 {
        let resp = gql_mark_distinct(&app, &tree_id, &first, &[&second]).await;
        assert_eq!(data(&resp)["markPersonsDistinct"], true);
    }
    assert!(gql_homonym_ids(&app, &tree_id, &first).await.is_empty());
    assert!(gql_homonym_ids(&app, &tree_id, &second).await.is_empty());

    let resp = gql_mark_distinct(&app, &tree_id, &first, &[&first]).await;
    assert_eq!(error_code(&resp), "VALIDATION_ERROR");

    let other_tree_id = gql_tree(&app).await;
    let stranger = gql_named_person(&app, &other_tree_id, "FEMALE", "Élise", "Sample").await;
    let resp = gql_mark_distinct(&app, &tree_id, &first, &[&stranger]).await;
    assert_eq!(error_code(&resp), "NOT_FOUND");
}

/// The same left-out items as REST: an event of the duplicate not taken is
/// dropped, and a third person's event cannot be left out.
#[tokio::test]
async fn graphql_merge_leaves_out_the_duplicates_items_not_taken() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let kept = gql_named_person(&app, &tree_id, "FEMALE", "Anna", "BRANCH_A").await;
    let duplicate = gql_named_person(&app, &tree_id, "FEMALE", "Anna", "BRANCH_A").await;
    let stranger = gql_named_person(&app, &tree_id, "MALE", "Otto", "BRANCH_B").await;
    let event = |person: String| {
        let app = app.clone();
        let tree_id = tree_id.clone();
        async move {
            let resp = graphql(
                app,
                r#"mutation($tree: ID!, $person: ID!) {
                    createEvent(treeId: $tree, input: { eventType: BIRTH, personId: $person, dateValue: "1850" }) { id }
                }"#,
                Some(json!({ "tree": tree_id, "person": person })),
            )
            .await;
            data(&resp)["createEvent"]["id"]
                .as_str()
                .unwrap()
                .to_string()
        }
    };
    let kept_birth = event(kept.clone()).await;
    let duplicate_birth = event(duplicate.clone()).await;
    let stranger_birth = event(stranger).await;
    let merge = |left_out: Vec<String>| {
        let app = app.clone();
        let (tree_id, kept, duplicate) = (tree_id.clone(), kept.clone(), duplicate.clone());
        async move {
            graphql(
                app,
                r#"mutation($tree: ID!, $kept: ID!, $duplicate: ID!, $out: [ID!]!) {
                    mergePersons(treeId: $tree, personId: $kept, duplicateId: $duplicate, choices: { leftOutEvents: $out }) { id }
                }"#,
                Some(json!({ "tree": tree_id, "kept": kept, "duplicate": duplicate, "out": left_out })),
            )
            .await
        }
    };
    let resp = merge(vec![stranger_birth]).await;
    assert_eq!(error_code(&resp), "VALIDATION_ERROR");
    let resp = merge(vec![duplicate_birth]).await;
    assert_eq!(data(&resp)["mergePersons"]["id"], kept.as_str());
    let resp = graphql(
        app.clone(),
        r#"query($tree: ID!, $person: ID!) {
            events(treeId: $tree, personId: $person) { edges { node { id } } }
        }"#,
        Some(json!({ "tree": tree_id, "person": kept })),
    )
    .await;
    let edges = &data(&resp)["events"]["edges"];
    assert_eq!(edges.as_array().unwrap().len(), 1, "{edges}");
    assert_eq!(edges[0]["node"]["id"], kept_birth.as_str());
}

/// Both name pieces taken: the duplicate's name, moved as a secondary name,
/// is promoted rather than written twice; the sex follows the choice.
#[tokio::test]
async fn graphql_merge_takes_the_chosen_name_and_sex() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let kept = gql_named_person(&app, &tree_id, "MALE", "Anna", "BRANCH_A").await;
    let duplicate = gql_named_person(&app, &tree_id, "FEMALE", "Anna Maria", "BRANCH_B").await;
    let resp = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $kept: ID!, $duplicate: ID!) {
            mergePersons(treeId: $tree, personId: $kept, duplicateId: $duplicate, choices: {
                surnameFromDuplicate: true, givenNamesFromDuplicate: true, sexFromDuplicate: true
            }) { id sex names { givenNames surname isPrimary } }
        }"#,
        Some(json!({ "tree": tree_id, "kept": kept, "duplicate": duplicate })),
    )
    .await;
    let merged = &data(&resp)["mergePersons"];
    assert_eq!(merged["sex"], "FEMALE");
    let names = merged["names"].as_array().unwrap();
    assert_eq!(names.len(), 2, "{names:?}");
    let primary: Vec<&Value> = names.iter().filter(|n| n["isPrimary"] == true).collect();
    assert_eq!(primary.len(), 1, "{names:?}");
    assert_eq!(primary[0]["givenNames"], "Anna Maria");
    assert_eq!(primary[0]["surname"], "BRANCH_B");
}

#[tokio::test]
async fn graphql_merge_moves_the_duplicate_onto_the_kept_person() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let kept = gql_named_person(&app, &tree_id, "FEMALE", "Élise", "Sample").await;
    let duplicate = gql_named_person(&app, &tree_id, "UNKNOWN", "Élise", "Sample").await;
    let parent = gql_named_person(&app, &tree_id, "MALE", "Parent", "Sample").await;
    let partner = gql_named_person(&app, &tree_id, "MALE", "Partner", "Spouse").await;
    let parents = gql_family(
        &app,
        &tree_id,
        &[(&parent, "HUSBAND")],
        &[&kept, &duplicate],
    )
    .await;
    let union = gql_family(
        &app,
        &tree_id,
        &[(&partner, "HUSBAND"), (&duplicate, "WIFE")],
        &[],
    )
    .await;
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: OCCUPATION, personId: "{duplicate}", description: "Weaver" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    data(&resp);

    let resp = gql_merge(&app, &tree_id, &kept, &duplicate).await;
    let merged = &data(&resp)["mergePersons"];
    assert_eq!(merged["id"], kept.as_str());
    assert_eq!(merged["sex"], "FEMALE");

    let resp = graphql(
        app.clone(),
        &format!(r#"{{ person(treeId: "{tree_id}", id: "{duplicate}") {{ id }} }}"#),
        None,
    )
    .await;
    assert!(data(&resp)["person"].is_null());

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{kept}") {{
                otherNames {{ surname }}
                occupation
                familyAsChild {{ familyId }}
                familiesAsSpouse {{ familyId spouseId }}
            }} }}"#
        ),
        None,
    )
    .await;
    let profile = &data(&resp)["personProfile"];
    assert!(profile["otherNames"].as_array().unwrap().is_empty());
    assert_eq!(profile["occupation"], "Weaver");
    assert_eq!(profile["familyAsChild"]["familyId"], parents.as_str());
    assert_eq!(profile["familiesAsSpouse"][0]["familyId"], union.as_str());
    assert_eq!(profile["familiesAsSpouse"][0]["spouseId"], partner.as_str());

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ personProfile(treeId: "{tree_id}", personId: "{parent}") {{ familiesAsSpouse {{ childrenIds }} }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(
        data(&resp)["personProfile"]["familiesAsSpouse"][0]["childrenIds"],
        json!([kept])
    );
    assert!(gql_homonym_ids(&app, &tree_id, &kept).await.is_empty());
}

#[tokio::test]
async fn graphql_merge_refuses_spouses_ancestors_and_the_same_person() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let husband = gql_named_person(&app, &tree_id, "MALE", "Sam", "Sample").await;
    let wife = gql_named_person(&app, &tree_id, "FEMALE", "Sam", "Sample").await;
    let child = gql_named_person(&app, &tree_id, "MALE", "Sam", "Sample").await;
    gql_family(
        &app,
        &tree_id,
        &[(&husband, "HUSBAND"), (&wife, "WIFE")],
        &[&child],
    )
    .await;

    for (kept, duplicate) in [
        (&husband, &wife),
        (&husband, &child),
        (&child, &wife),
        (&husband, &husband),
    ] {
        let resp = gql_merge(&app, &tree_id, kept, duplicate).await;
        assert_eq!(error_code(&resp), "VALIDATION_ERROR", "{resp}");
    }
    assert_eq!(gql_homonym_ids(&app, &tree_id, &husband).await.len(), 2);
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
async fn the_basemap_names_its_populated_places_over_graphql() {
    let app = setup_app().await;
    let response = graphql(
        app,
        "{ basemap { iso cities { name lon lat zoom population names { lang name } } } }",
        None,
    )
    .await;
    let countries = data(&response)["basemap"].as_array().unwrap().clone();
    let france = countries
        .iter()
        .find(|c| c["iso"] == "FR")
        .expect("France is on the basemap");
    let cities = france["cities"].as_array().unwrap();
    assert!(cities.len() > 10);
    assert!(cities[0]["zoom"].as_i64().unwrap() < 30);
}

#[tokio::test]
async fn test_dictionary_and_reference_over_graphql() {
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
                sourceUsage(treeId: "{tree_id}", sourceId: "{source_id}") {{ personId }}
                placeUsage(treeId: "{tree_id}", placeId: "{place_id}") {{ personId }}
                occupationReference(language: "fr", term: "Agriculteur") {{ label }}
                givenNameReference(language: "fr", term: "Marie") {{ label }}
                surnames: valueSuggestions(treeId: "{tree_id}", field: FAMILY_NAMES, language: "fr", query: "dur") {{ value count reference }}
                givenNames: valueSuggestions(treeId: "{tree_id}", field: GIVEN_NAMES, language: "fr", query: "mar", limit: 3) {{ value count reference }}
                occupations: valueSuggestions(treeId: "{tree_id}", field: OCCUPATIONS, language: "fr", query: "agri") {{ value count reference }}
                sources: valueSuggestions(treeId: "{tree_id}", field: SOURCES, language: "fr", query: "regis") {{ value count reference }}
                scopedGivenNames: valueSuggestions(treeId: "{tree_id}", field: GIVEN_NAMES, language: "fr", query: "mar", limit: 3, surname: "dur") {{ value count reference }}
                scopedSurnames: valueSuggestions(treeId: "{tree_id}", field: FAMILY_NAMES, language: "fr", query: "d", givenNames: "nobody") {{ value }}
            }}"#
        ),
        None,
    )
    .await;
    let response = data(&response);

    assert_eq!(
        response["surnames"],
        serde_json::json!([{ "value": "Durand", "count": 1, "reference": false }])
    );
    assert_eq!(
        response["givenNames"][0],
        serde_json::json!({ "value": "Marie", "count": 1, "reference": true })
    );
    assert_eq!(response["givenNames"].as_array().unwrap().len(), 3);
    assert_eq!(
        response["occupations"][0],
        serde_json::json!({ "value": "Agriculteur", "count": 1, "reference": true })
    );
    assert_eq!(
        response["sources"],
        serde_json::json!([{ "value": "Lyon register", "count": 1, "reference": false }])
    );
    // Scoped, the list holds the persons' own names only: no sheet term.
    assert_eq!(
        response["scopedGivenNames"],
        serde_json::json!([{ "value": "Marie", "count": 1, "reference": true }])
    );
    assert_eq!(response["scopedSurnames"], serde_json::json!([]));
    let scoped_source = format!(
        r#"{{ valueSuggestions(treeId: "{tree_id}", field: SOURCES, language: "fr", query: "regis", surname: "dur") {{ value }} }}"#
    );
    let rejected = graphql(app.clone(), &scoped_source, None).await;
    assert!(
        rejected["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "a scope only applies to name fields"
    );
    for (language, limit) in [("xx", 10), ("fr", 0), ("fr", 51)] {
        let query = format!(
            r#"{{ valueSuggestions(treeId: "{tree_id}", field: SOURCES, language: "{language}", query: "regis", limit: {limit}) {{ value }} }}"#
        );
        let response = graphql(app.clone(), &query, None).await;
        assert!(
            response["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty()),
            "should be rejected: {query}"
        );
    }
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
    assert_eq!(response["occupationReference"]["label"], "Agriculteur");
    assert_eq!(response["givenNameReference"]["label"], "Marie");
}

// ── date_sort is the server's to derive ───────────────────────────────

/// A date written in another calendar has to be normalised to Gregorian
/// before it can be sorted against the rest, and only the server can do that.
/// A Republican `2 BRUM 14` must sort by its Gregorian equivalent, not year 14.
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

    // Create the document, then its unheld page.
    let document_id = document_id_for(&app, &tree_id).await;
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMedia(treeId: "{tree_id}", input: {{ documentId: "{document_id}", fileName: "photo.jpg", mimeType: "image/jpeg", filePath: "/uploads/photo.jpg", fileSize: 1024 }}) {{ id fileName parentMediaId }} }}"#
        ),
        None,
    )
    .await;
    let media = &data(&resp)["uploadMedia"];
    assert_eq!(media["fileName"], "photo.jpg");
    assert_eq!(media["parentMediaId"], document_id);
    let media_id = document_id;

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
async fn a_note_without_text_is_refused() {
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
            r#"mutation {{ createNote(treeId: "{tree_id}", input: {{ text: "   " }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(resp["errors"][0]["extensions"]["code"], "VALIDATION_ERROR");
}

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

/// Mirrors the REST cross-tree test: a projection, a pedigree or a usage read
/// naming a record of another tree is not found, and the record's own tree
/// still answers.
#[tokio::test]
async fn projection_and_usage_queries_are_tree_scoped() {
    let app = setup_app().await;
    let mut tree_ids = Vec::new();
    for name in ["Scope tree", "Other tree"] {
        tree_ids.push(
            data(
                &graphql(
                    app.clone(),
                    &format!(r#"mutation {{ createTree(input: {{ name: "{name}" }}) {{ id }} }}"#),
                    None,
                )
                .await,
            )["createTree"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let (tree_id, other_tree_id) = (&tree_ids[0], &tree_ids[1]);
    let created = data(
        &graphql(
            app.clone(),
            &format!(
                r#"mutation {{
                    createPerson(treeId: "{other_tree_id}", input: {{ sex: FEMALE }}) {{ id }}
                    createSource(treeId: "{other_tree_id}", input: {{ title: "Sample register" }}) {{ id }}
                    createPlace(treeId: "{other_tree_id}", input: {{ name: "Sampleville" }}) {{ id }}
                }}"#
            ),
            None,
        )
        .await,
    )
    .clone();
    let person_id = created["createPerson"]["id"].as_str().unwrap();
    let source_id = created["createSource"]["id"].as_str().unwrap();
    let place_id = created["createPlace"]["id"].as_str().unwrap();

    let queries = |tree: &str| {
        [
            format!(
                r#"{{ personProfile(treeId: "{tree}", personId: "{person_id}") {{ personId }} }}"#
            ),
            format!(
                r#"{{ pedigree(treeId: "{tree}", rootPersonId: "{person_id}", ancestorDepth: 1, descendantDepth: 1) {{ rootPersonId }} }}"#
            ),
            format!(
                r#"{{ sourceUsage(treeId: "{tree}", sourceId: "{source_id}") {{ personId }} }}"#
            ),
            format!(r#"{{ placeUsage(treeId: "{tree}", placeId: "{place_id}") {{ personId }} }}"#),
        ]
    };
    for query in queries(tree_id) {
        let response = graphql(app.clone(), &query, None).await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "NOT_FOUND",
            "{query} answered across trees: {response}"
        );
    }
    for query in queries(other_tree_id) {
        let response = graphql(app.clone(), &query, None).await;
        assert!(response.get("errors").is_none(), "{query}: {response}");
    }
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

/// `GET /graphql`: its status and body.
async fn get_graphql(app: axum::Router) -> (StatusCode, String) {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/graphql")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// GraphiQL loads its scripts from a public CDN: it is served only where the
/// deployment enables it, and is an unknown route elsewhere.
#[tokio::test]
async fn test_graphiql_playground() {
    let app = setup_app().await;
    let (status, body) = get_graphql(app.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains(r#""error":"not_found""#), "{body}");

    // The marker exists only where GraphQL is compiled in.
    #[cfg(feature = "graphql")]
    {
        let enabled = app.layer(axum::Extension(oxidgene_api::graphql::GraphiQl));
        let (status, body) = get_graphql(enabled).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("graphiql"));
    }
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

    let db = setup_db().await;
    let state = AppState::new(
        db,
        std::env::temp_dir().join(format!(
            "oxidgene-gql-geneanet-wizard-{}",
            uuid::Uuid::now_v7()
        )),
    )
    .with_local_file_access();
    let app = build_router(state);
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

/// The export's `SUBM` record carries the tree's "Who am I?" person, and
/// `Not Provided` once the tree names nobody.
#[tokio::test]
async fn graphql_gedcom_export_names_the_trees_own_person_as_submitter() {
    let app = setup_app().await;
    let tree_id = tree_id_for(&app).await;
    let response = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: FEMALE }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let person_id = data(&response)["createPerson"]["id"]
        .as_str()
        .expect("person id")
        .to_string();
    let response = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ addPersonName(treeId: "{tree_id}", personId: "{person_id}", input: {{ nameType: BIRTH, givenNames: "Ada", surname: "Alpha", isPrimary: true }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    assert!(data(&response)["addPersonName"]["id"].is_string());

    for (self_person_id, expected) in [
        (
            format!(r#""{person_id}""#),
            "0 @SUBM1@ SUBM\n1 NAME Ada Alpha\n",
        ),
        ("null".to_string(), "0 @SUBM1@ SUBM\n1 NAME Not Provided\n"),
    ] {
        let response = graphql(
            app.clone(),
            &format!(
                r#"mutation {{ updateTree(id: "{tree_id}", input: {{ selfPersonId: {self_person_id} }}) {{ id }} }}"#
            ),
            None,
        )
        .await;
        assert!(data(&response)["updateTree"]["id"].is_string());
        let response = graphql(
            app.clone(),
            &format!(r#"{{ exportGedcom(treeId: "{tree_id}") {{ gedcom }} }}"#),
            None,
        )
        .await;
        let gedcom = data(&response)["exportGedcom"]["gedcom"]
            .as_str()
            .expect("gedcom");
        assert!(gedcom.contains("\n1 SUBM @SUBM1@\n"), "{gedcom}");
        assert!(gedcom.contains(expected), "{gedcom}");
    }
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

    let status_query = format!(
        r#"{{ exportJobStatus(treeId: "{tree_id}", jobId: "{job_id}") {{ phase downloadUrl expiresAt sizeBytes warnings error }} }}"#
    );
    let response = graphql(app.clone(), &status_query, None).await;
    let result = &data(&response)["exportJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert_eq!(
        result["downloadUrl"],
        format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}/download")
    );
    let expires_at: chrono::DateTime<chrono::Utc> = result["expiresAt"]
        .as_str()
        .expect("expiry, as REST")
        .parse()
        .expect("RFC 3339 expiry");
    assert!(expires_at > chrono::Utc::now() + chrono::Duration::minutes(59));
    assert!(result["sizeBytes"].as_i64().unwrap() > 0, "{result}");
    assert!(result["warnings"].as_array().unwrap().is_empty());
    assert!(result["error"].is_null());

    // Once the artifact has expired, the status offers no download, as REST.
    worker
        .maintain(chrono::Utc::now() + chrono::Duration::hours(2))
        .await;
    let response = graphql(app.clone(), &status_query, None).await;
    let result = &data(&response)["exportJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert!(result["downloadUrl"].is_null());
    assert!(result["expiresAt"].is_null());
    let response = graphql(
        app,
        &format!(r#"{{ downloadableExport(treeId: "{tree_id}") {{ jobId }} }}"#),
        None,
    )
    .await;
    assert!(data(&response)["downloadableExport"].is_null());
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
    )
    .with_local_file_access();
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
    // One photograph naming somebody outside the tree: they become a person
    // of their own, which the receipt lists. Its bytes were never gathered,
    // so the photograph itself is reported as skipped.
    let collection = json!({
        "deposits": [{"id": 1, "views": [{"id": 10, "files": {"normal": "https://example.invalid/normal.jpg"}}]}],
        "references": [{
            "deposit": {"id": 1, "views": [{"id": 10}]},
            "firstname": "person_c",
            "lastname": "BRANCH_C",
        }],
    })
    .to_string();
    let response = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $gw: String!, $collection: String!) {
            importGeneanet(treeId: $tree, input: { gwBase64: $gw, fileName: "family.gw", collection: $collection }) { jobId }
        }"#,
        Some(json!({ "tree": tree_id, "gw": gw_base64, "collection": collection })),
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
        app.clone(),
        &format!(
            r#"{{ importJobStatus(treeId: "{tree_id}", jobId: "{job_id}") {{ phase result {{ personsCount }} geneanetResult {{ personsCount familiesCount imagesCount documentsCount documentPagesCount isolatedCount isolatedPeople {{ personId surname givenNames }} }} error }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&response)["importJobStatus"];
    assert_eq!(result["phase"], "completed");
    assert!(result["result"].is_null());
    assert_eq!(result["geneanetResult"]["personsCount"], 2);
    assert_eq!(result["geneanetResult"]["familiesCount"], 1);
    for count in ["imagesCount", "documentsCount", "documentPagesCount"] {
        assert_eq!(result["geneanetResult"][count], 0, "{count}");
    }
    assert_eq!(result["geneanetResult"]["isolatedCount"], 1);
    let isolated = &result["geneanetResult"]["isolatedPeople"][0];
    assert_eq!(isolated["surname"], "BRANCH_C");
    assert_eq!(isolated["givenNames"], "person_c");
    let isolated_id = isolated["personId"].as_str().unwrap();
    let response = graphql(
        app.clone(),
        &format!(r#"{{ person(treeId: "{tree_id}", id: "{isolated_id}") {{ id }} }}"#),
        None,
    )
    .await;
    assert_eq!(data(&response)["person"]["id"], isolated_id);
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
        &format!(
            r#"query {{ personProfiles(treeId: "{tree_id}", first: 1) {{ totalCount pageInfo {{ hasNextPage endCursor }} edges {{ cursor node {{ personId }} }} }} }}"#
        ),
        None,
    )
    .await;
    let page = &data(&resp)["personProfiles"];
    assert_eq!(page["totalCount"], 1);
    assert_eq!(page["pageInfo"]["hasNextPage"], false);
    assert_eq!(page["edges"][0]["node"]["personId"], person_id);

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

    // The batched form answers for several roots at once, in request order,
    // and refuses a batch larger than the bound rather than truncating it.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"query {{ pedigrees(treeId: "{tree_id}", rootPersonIds: ["{person_id}", "{person_id}"],
                ancestorDepth: 2, descendantDepth: 1) {{
                rootPersonId pedigree {{ ancestorDepthLoaded }}
            }} }}"#
        ),
        None,
    )
    .await;
    let entries = data(&resp)["pedigrees"].as_array().unwrap().clone();
    assert_eq!(entries.len(), 2, "{resp}");
    assert_eq!(entries[0]["rootPersonId"], person_id);
    assert_eq!(entries[0]["pedigree"]["ancestorDepthLoaded"], 2);

    let roots = (0..65)
        .map(|_| format!("\"{}\"", uuid::Uuid::now_v7()))
        .collect::<Vec<_>>()
        .join(",");
    let resp = graphql(
        app.clone(),
        &format!(
            r#"query {{ pedigrees(treeId: "{tree_id}", rootPersonIds: [{roots}],
                ancestorDepth: 2, descendantDepth: 1) {{ rootPersonId }} }}"#
        ),
        None,
    )
    .await;
    assert!(
        resp["errors"]
            .as_array()
            .is_some_and(|errors| !errors.is_empty()),
        "oversized batch should be rejected: {resp}"
    );

    // Expansion is a read, like REST's `GET …/expand`.
    let resp = graphql(
        app.clone(),
        &format!(
            r#"query {{ expandPedigree(treeId: "{tree_id}", rootPersonId: "{person_id}",
                direction: ANCESTORS, fromDepth: 2, toDepth: 4, otherDepth: 1) {{
                ancestorDepthLoaded descendantDepthLoaded newNodes {{ personId }} }} }}"#
        ),
        None,
    )
    .await;
    let delta = &data(&resp)["expandPedigree"];
    assert_eq!(delta["ancestorDepthLoaded"], 4);
    assert_eq!(delta["descendantDepthLoaded"], 1);
    assert_eq!(delta["newNodes"], json!([]));

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

async fn document_id_for(app: &axum::Router, tree_id: &str) -> String {
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createMediaDocument(treeId: "{tree_id}", title: "Sample document") {{ id title pageCount }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(
        data(&resp)["createMediaDocument"]["title"],
        "Sample document"
    );
    assert_eq!(data(&resp)["createMediaDocument"]["pageCount"], 0);
    data(&resp)["createMediaDocument"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_upload_media_file_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 documentId: "{document_id}",
                 fileName: "portrait.png",
                 contentBase64: "{}"
               }}) {{ id fileName mimeType width height pageCount sha256 storageKey thumbnailKey parentMediaId }} }}"#,
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
    assert_eq!(media["parentMediaId"], document_id);
    assert_eq!(media["sha256"].as_str().unwrap().len(), 64);
    assert!(media["storageKey"].is_string());
    assert!(media["thumbnailKey"].is_string());
}

/// The GraphQL twin of `a_page_held_by_somebody_else_counts_like_one_we_store`.
///
/// `uploadMedia` writes a page whose bytes are somebody else's. The row landing
/// is not the whole job: a document's `pageCount` is maintained by whoever adds
/// the page, and a surface that skips it reports an empty document holding a
/// page.
#[tokio::test]
async fn a_remote_page_added_over_graphql_counts_toward_its_document() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMedia(treeId: "{tree_id}", input: {{
                 documentId: "{document_id}",
                 fileName: "folio-3.jpg",
                 mimeType: "image/jpeg",
                 filePath: "https://archives.example.org/dossier/3.jpg",
                 fileSize: 0
               }}) {{ id parentMediaId }} }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&resp)["uploadMedia"]["parentMediaId"], document_id);

    let resp = graphql(
        app.clone(),
        &format!(r#"{{ media(treeId: "{tree_id}", id: "{document_id}") {{ pageCount }} }}"#),
        None,
    )
    .await;
    assert_eq!(data(&resp)["media"]["pageCount"], 1);
}

#[tokio::test]
async fn deleting_a_page_over_graphql_removes_its_relations() {
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
                 documentId: "{document_id}", fileName: "page.png", contentBase64: "{}"
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

    let deleted = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ deleteMediaPage(treeId: "{tree_id}", documentId: "{document_id}", pageId: "{page_id}") }}"#
        ),
        None,
    )
    .await;
    assert_eq!(data(&deleted)["deleteMediaPage"], true);

    let result = graphql(
        app,
        &format!(
            r#"{{
                media(treeId: "{tree_id}", id: "{page_id}") {{ id }}
                document: media(treeId: "{tree_id}", id: "{document_id}") {{ id pageCount }}
                treeMediaLinks(treeId: "{tree_id}") {{ linkId }}
                vignettes(treeId: "{tree_id}", personId: "{person_id}") {{ id }}
                person(treeId: "{tree_id}", id: "{person_id}") {{ portraitMediaId portraitVignetteId }}
            }}"#
        ),
        None,
    )
    .await;
    let result = data(&result);
    assert!(result["media"].is_null());
    assert_eq!(result["document"]["id"], document_id);
    assert_eq!(result["document"]["pageCount"], 0);
    assert!(result["treeMediaLinks"].as_array().unwrap().is_empty());
    assert!(result["vignettes"].as_array().unwrap().is_empty());
    assert!(result["person"]["portraitMediaId"].is_null());
    assert!(result["person"]["portraitVignetteId"].is_null());
}

#[tokio::test]
async fn test_upload_media_file_rejects_content_that_is_not_base64() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 documentId: "{document_id}", fileName: "x.png", contentBase64: "not base64 at all!!"
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
    let document_id = document_id_for(&app, &tree_id).await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ uploadMediaFile(treeId: "{tree_id}", input: {{
                 documentId: "{document_id}", fileName: "register.png", contentBase64: "{}"
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
                             }}) {{ id x y width height }} }}"#
        ),
        None,
    )
    .await;
    let vignette = &data(&resp)["createVignette"];
    assert_eq!(vignette["x"], 10);
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

#[tokio::test]
async fn tree_statistics_match_rest() {
    let app = setup_app().await;
    let response = graphql(
        app.clone(),
        r#"mutation { createTree(input: { name: "Statistics" }) { id } }"#,
        None,
    )
    .await;
    let tree_id = data(&response)["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let response = graphql(
        app.clone(),
        &format!(
            r#"{{ treeStatistics(treeId: "{tree_id}", approximate: true, language: "fr") {{
                persons unknownSex sources firstYear topSurnames {{ label count }}
                topGivenNamesMen {{ label }} eventTypes {{ label count }}
                lifespan {{ all {{ count mean median stdDev min max }} men {{ mean }} }}
                generationInterval {{ mean }} childrenHistogram mortality {{ year counts }}
                records {{ kind persons {{ name }} value value2 }}
                largestFamilies {{ children }} birthsByCountry {{ label count }} countries
                ageAtDeath {{ men {{ year sum count }} women {{ year sum count }} }}
                birthsByMonth {{ year counts }} unionDuration {{ year sum count }}
                recentUnions {{ familyId }} locatedPlaces {{ name }} unlocatedPlaces
            }} }}"#
        ),
        None,
    )
    .await;
    let stats = &data(&response)["treeStatistics"];
    assert_eq!(stats["persons"], 0);
    assert_eq!(stats["ageAtDeath"]["men"], serde_json::json!([]));
    assert_eq!(stats["birthsByMonth"], serde_json::json!([]));
    assert_eq!(stats["lifespan"]["all"]["count"], 0);
    assert_eq!(stats["lifespan"]["all"]["mean"], serde_json::Value::Null);
    assert_eq!(stats["records"], serde_json::json!([]));
    assert_eq!(stats["firstYear"], serde_json::Value::Null);

    // An unknown language is refused, as REST answers 400.
    let response = graphql(
        app.clone(),
        &format!(r#"{{ treeStatistics(treeId: "{tree_id}", language: "xx") {{ persons }} }}"#),
        None,
    )
    .await;
    assert!(
        response["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown language should be rejected: {response}"
    );

    let response = graphql(
        app,
        &format!(
            r#"{{ treeStatistics(treeId: "{}") {{ persons }} }}"#,
            uuid::Uuid::now_v7()
        ),
        None,
    )
    .await;
    assert!(
        response["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown tree should be rejected, as REST answers 404: {response}"
    );
}

// ───────────────────────── Tools ─────────────────────────

async fn gql_event(app: &axum::Router, tree_id: &str, owner: &str, kind: &str, date: &str) {
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: {kind}, dateValue: "{date}", {owner} }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    data(&resp);
}

/// The same walk as REST: found ancestors, listed missing parents, implied
/// branches, the same bounds and the same errors.
#[tokio::test]
async fn ancestry_completeness_matches_rest() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let query = |tree_id: &str, generations: &str| {
        format!(
            r#"{{ ancestryCompleteness(treeId: "{tree_id}"{generations}) {{
                root {{ personId name }}
                generations {{ generation expected found withBirth withDeath withUnion living impliedMissing
                    entries {{ sosa person {{ personId name sex hasBirth hasDeath hasUnion living birth {{ value qualifier calendar }} }} }} }}
            }} }}"#
        )
    };

    let resp = graphql(app.clone(), &query(&tree_id, ""), None).await;
    assert!(data(&resp)["ancestryCompleteness"]["root"].is_null());

    let root = gql_named_person(&app, &tree_id, "FEMALE", "Child", "BRANCH_A").await;
    let father = gql_named_person(&app, &tree_id, "MALE", "Parent", "BRANCH_A").await;
    let family = gql_family(&app, &tree_id, &[(&father, "HUSBAND")], &[&root]).await;
    gql_event(
        &app,
        &tree_id,
        &format!(r#"personId: "{father}""#),
        "BIRTH",
        "1900",
    )
    .await;
    gql_event(
        &app,
        &tree_id,
        &format!(r#"familyId: "{family}""#),
        "MARRIAGE",
        "1925",
    )
    .await;
    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ updateTree(id: "{tree_id}", input: {{ sosaRootPersonId: "{root}" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    data(&resp);

    let resp = graphql(app.clone(), &query(&tree_id, ", generations: 3"), None).await;
    let result = &data(&resp)["ancestryCompleteness"];
    assert_eq!(result["root"]["personId"], root.as_str());
    let parents = &result["generations"][1];
    assert_eq!(parents["found"], 1);
    assert_eq!(parents["withUnion"], 1);
    assert_eq!(parents["entries"][0]["sosa"], 2);
    assert_eq!(parents["entries"][0]["person"]["hasBirth"], true);
    assert_eq!(parents["entries"][0]["person"]["birth"]["value"], "1900");
    assert!(parents["entries"][1]["person"].is_null());
    assert_eq!(result["generations"][2]["impliedMissing"], 2);

    let resp = graphql(app.clone(), &query(&tree_id, ", generations: 16"), None).await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "too many generations should be rejected, as REST answers 400: {resp}"
    );
    let resp = graphql(app, &query(&uuid::Uuid::now_v7().to_string(), ""), None).await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown tree should be rejected, as REST answers 404: {resp}"
    );
}

/// The same anomalies and unlocated places as REST, with the same errors.
#[tokio::test]
async fn anomalies_and_unlocated_places_match_rest() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let person = gql_named_person(&app, &tree_id, "MALE", "Backwards", "BRANCH_A").await;
    gql_event(
        &app,
        &tree_id,
        &format!(r#"personId: "{person}""#),
        "BIRTH",
        "1850",
    )
    .await;
    gql_event(
        &app,
        &tree_id,
        &format!(r#"personId: "{person}""#),
        "DEATH",
        "1840",
    )
    .await;

    let resp = graphql(
        app.clone(),
        &format!(
            r#"{{ treeAnomalies(treeId: "{tree_id}") {{ persons rules {{ rule category severity count
                items {{ persons {{ personId name }} familyId value eventType text }} }} }} }}"#
        ),
        None,
    )
    .await;
    let result = &data(&resp)["treeAnomalies"];
    assert_eq!(result["persons"], 1);
    let rule = result["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "death_before_birth")
        .expect("the death before the birth is found");
    assert_eq!(rule["severity"], "error");
    assert_eq!(rule["items"][0]["persons"][0]["personId"], person.as_str());

    let resp = graphql(
        app.clone(),
        &format!(
            r#"mutation {{ createPlace(treeId: "{tree_id}", input: {{ name: "Qzxv Nowhere Hamlet" }}) {{ id }} }}"#
        ),
        None,
    )
    .await;
    let place_id = data(&resp)["createPlace"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    gql_event(
        &app,
        &tree_id,
        &format!(r#"personId: "{person}", placeId: "{place_id}""#),
        "RESIDENCE",
        "1845",
    )
    .await;
    let resp = graphql(
        app.clone(),
        &format!(r#"{{ unlocatedPlaces(treeId: "{tree_id}") {{ placeId name count latitude }} }}"#),
        None,
    )
    .await;
    let places = &data(&resp)["unlocatedPlaces"];
    assert_eq!(places[0]["placeId"], place_id.as_str());
    assert_eq!(places[0]["count"], 1);
    assert!(places[0]["latitude"].is_null());

    let unknown = uuid::Uuid::now_v7();
    for query in [
        format!(r#"{{ treeAnomalies(treeId: "{unknown}") {{ persons }} }}"#),
        format!(r#"{{ unlocatedPlaces(treeId: "{unknown}") {{ name }} }}"#),
    ] {
        let resp = graphql(app.clone(), &query, None).await;
        assert!(
            resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
            "an unknown tree should be rejected, as REST answers 404: {resp}"
        );
    }
}

/// The same pairs as REST, gone once confirmed distinct, the same errors.
#[tokio::test]
async fn potential_duplicates_match_rest() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let first = gql_named_person(&app, &tree_id, "FEMALE", "Anna", "BRANCH_A").await;
    let second = gql_named_person(&app, &tree_id, "FEMALE", "Anna", "BRANCH_A").await;
    for person in [&first, &second] {
        gql_event(
            &app,
            &tree_id,
            &format!(r#"personId: "{person}""#),
            "BIRTH",
            "1850",
        )
        .await;
    }
    let query = format!(
        r#"{{ potentialDuplicates(treeId: "{tree_id}") {{ count pairs {{ score reasons
            first {{ personId surname birthYear }} second {{ personId }}
            firstDates {{ birth {{ value qualifier calendar }} death {{ value }} }} }} }} }}"#
    );
    let resp = graphql(app.clone(), &query, None).await;
    let found = &data(&resp)["potentialDuplicates"];
    assert_eq!(found["count"], 1);
    assert_eq!(found["pairs"][0]["score"], 50);
    assert_eq!(
        found["pairs"][0]["reasons"],
        json!(["same_name", "same_birth_year"])
    );
    assert_eq!(found["pairs"][0]["first"]["birthYear"], "1850");
    assert_eq!(found["pairs"][0]["firstDates"]["birth"]["value"], "1850");
    assert!(found["pairs"][0]["firstDates"]["death"].is_null());

    let resp = gql_mark_distinct(&app, &tree_id, &first, &[&second]).await;
    data(&resp);
    let resp = graphql(app.clone(), &query, None).await;
    assert_eq!(data(&resp)["potentialDuplicates"]["count"], 0);

    let resp = graphql(
        app,
        &format!(
            r#"{{ potentialDuplicates(treeId: "{}") {{ count }} }}"#,
            uuid::Uuid::now_v7()
        ),
        None,
    )
    .await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "an unknown tree should be rejected, as REST answers 404: {resp}"
    );
}

// ── Media library (Dictionary › Media) ───────────────────────────────

/// A document with the given title; returns its id.
async fn gql_document(app: &axum::Router, tree_id: &str, title: &str) -> String {
    let resp = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $title: String!) {
            createMediaDocument(treeId: $tree, title: $title) { id }
        }"#,
        Some(json!({ "tree": tree_id, "title": title })),
    )
    .await;
    data(&resp)["createMediaDocument"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Run a mutation for its side effect, failing on any error.
async fn gql_do(app: &axum::Router, query: &str, variables: Value) {
    data(&graphql(app.clone(), query, Some(variables)).await);
}

/// Three documents, as the REST fixture builds them: a census scan linked to
/// a person, a parish PDF linked to a dated event of another, and a
/// photograph with a crop identifying that other person.
async fn gql_library_fixture(app: &axum::Router, tree_id: &str) {
    let first = gql_named_person(app, tree_id, "FEMALE", "Élodie", "Fictive").await;
    let second = gql_named_person(app, tree_id, "MALE", "Marc", "Exemple").await;
    gql_census_fixture(app, tree_id, &first).await;
    gql_parish_fixture(app, tree_id, &second).await;
    gql_photo_fixture(app, tree_id, &second).await;
}

const GQL_ADD_TAG: &str = r#"mutation($tree: ID!, $id: ID!, $tag: String!) {
    addMediaTag(treeId: $tree, id: $id, tag: $tag) { id }
}"#;

const GQL_UPLOAD_PAGE: &str = r#"mutation($tree: ID!, $doc: ID!, $name: String!, $content: String!) {
    uploadMediaFile(treeId: $tree, input: { documentId: $doc, fileName: $name, contentBase64: $content }) { id }
}"#;

/// A census scan tagged twice and linked to `person`.
async fn gql_census_fixture(app: &axum::Router, tree_id: &str, person: &str) {
    let census = gql_document(app, tree_id, "Census sheet").await;
    gql_do(
        app,
        GQL_UPLOAD_PAGE,
        json!({ "tree": tree_id, "doc": census, "name": "sheet.png", "content": png_base64(40, 30) }),
    )
    .await;
    gql_do(
        app,
        r#"mutation($tree: ID!, $id: ID!) {
            updateMedia(treeId: $tree, id: $id, input: { documentCategory: CENSUS }) { id }
        }"#,
        json!({ "tree": tree_id, "id": census }),
    )
    .await;
    for tag in ["Village Alpha", "Survey"] {
        gql_do(
            app,
            GQL_ADD_TAG,
            json!({ "tree": tree_id, "id": census, "tag": tag }),
        )
        .await;
    }
    gql_do(
        app,
        r#"mutation($tree: ID!, $media: ID!, $person: ID!) {
            createMediaLink(treeId: $tree, input: { mediaId: $media, personId: $person }) { id }
        }"#,
        json!({ "tree": tree_id, "media": census, "person": person }),
    )
    .await;
}

/// A parish PDF linked to a dated baptism of `person`.
async fn gql_parish_fixture(app: &axum::Router, tree_id: &str, person: &str) {
    let parish = gql_document(app, tree_id, "Écrits de paroisse").await;
    gql_do(
        app,
        r#"mutation($tree: ID!, $doc: ID!) {
            uploadMedia(treeId: $tree, input: { documentId: $doc, fileName: "register.pdf", mimeType: "application/pdf", filePath: "register.pdf", fileSize: 0 }) { id }
        }"#,
        json!({ "tree": tree_id, "doc": parish }),
    )
    .await;
    gql_do(
        app,
        GQL_ADD_TAG,
        json!({ "tree": tree_id, "id": parish, "tag": "village alpha" }),
    )
    .await;
    let resp = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $person: ID!) {
            createEvent(treeId: $tree, input: { eventType: BAPTISM, personId: $person, dateValue: "12 MAR 1890" }) { id }
        }"#,
        Some(json!({ "tree": tree_id, "person": person })),
    )
    .await;
    let event = data(&resp)["createEvent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    gql_do(
        app,
        r#"mutation($tree: ID!, $media: ID!, $event: ID!) {
            createMediaLink(treeId: $tree, input: { mediaId: $media, eventId: $event }) { id }
        }"#,
        json!({ "tree": tree_id, "media": parish, "event": event }),
    )
    .await;
}

/// A photograph with a crop identifying `person`.
async fn gql_photo_fixture(app: &axum::Router, tree_id: &str, person: &str) {
    let photo = gql_document(app, tree_id, "Group photo").await;
    let resp = graphql(
        app.clone(),
        GQL_UPLOAD_PAGE,
        Some(
            json!({ "tree": tree_id, "doc": photo, "name": "garden.png", "content": png_base64(80, 60) }),
        ),
    )
    .await;
    let page = data(&resp)["uploadMediaFile"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    gql_do(
        app,
        GQL_ADD_TAG,
        json!({ "tree": tree_id, "id": photo, "tag": "Village Alpha" }),
    )
    .await;
    gql_do(
        app,
        r#"mutation($tree: ID!, $page: ID!, $person: ID!) {
            createVignette(treeId: $tree, input: { mediaId: $page, personId: $person, x: 0, y: 0, width: 20, height: 20 }) { id }
        }"#,
        json!({ "tree": tree_id, "page": page, "person": person }),
    )
    .await;
}

/// The titles `mediaList` returns under `filter`, checking `totalCount`.
async fn gql_library_titles(app: &axum::Router, tree_id: &str, filter: Value) -> Vec<String> {
    let resp = graphql(
        app.clone(),
        r#"query($tree: ID!, $filter: MediaListFilterInput) {
            mediaList(treeId: $tree, filter: $filter) { totalCount edges { node { title } } }
        }"#,
        Some(json!({ "tree": tree_id, "filter": filter })),
    )
    .await;
    let list = &data(&resp)["mediaList"];
    let titles: Vec<String> = list["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| {
            edge["node"]["title"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(list["totalCount"], titles.len(), "{filter}: {list}");
    titles
}

#[tokio::test]
async fn the_media_library_narrows_by_every_filter_over_graphql() {
    let today = chrono::Utc::now().date_naive();
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    gql_library_fixture(&app, &tree_id).await;

    let all = ["Census sheet", "Écrits de paroisse", "Group photo"];
    let titles = |filter: Value| gql_library_titles(&app, &tree_id, filter);
    assert_eq!(titles(json!(null)).await, all);
    assert_eq!(titles(json!({ "tags": ["VILLAGE ALPHA"] })).await, all);
    assert_eq!(
        titles(json!({ "tags": ["survey"] })).await,
        ["Census sheet"]
    );
    assert_eq!(
        titles(json!({ "tags": ["village alpha", "Survey"] })).await,
        ["Census sheet"],
        "a document must carry every tag given"
    );
    assert_eq!(
        titles(json!({ "kind": "PDF" })).await,
        ["Écrits de paroisse"]
    );
    assert_eq!(
        titles(json!({ "kind": "IMAGE" })).await,
        ["Census sheet", "Group photo"]
    );
    assert_eq!(
        titles(json!({ "category": "CENSUS" })).await,
        ["Census sheet"]
    );
    assert_eq!(
        titles(json!({ "name": "ECRITS" })).await,
        ["Écrits de paroisse"]
    );
    assert_eq!(titles(json!({ "name": "garden" })).await, ["Group photo"]);
    assert_eq!(
        titles(json!({ "linkedName": "elodie fict" })).await,
        ["Census sheet"]
    );
    assert_eq!(
        titles(json!({ "linkedName": "exemple" })).await,
        ["Écrits de paroisse", "Group photo"]
    );
    assert_eq!(
        titles(json!({ "eventFrom": 1885, "eventTo": 1895 })).await,
        ["Écrits de paroisse"]
    );
    assert!(titles(json!({ "eventFrom": 1891 })).await.is_empty());
    assert_eq!(titles(json!({ "addedFrom": today.to_string() })).await, all);
    assert!(
        titles(json!({ "addedTo": today.pred_opt().unwrap().to_string() }))
            .await
            .is_empty()
    );
    assert_eq!(
        titles(json!({ "tags": ["village alpha"], "kind": "IMAGE", "linkedName": "exemple" }))
            .await,
        ["Group photo"]
    );

    let resp = graphql(
        app.clone(),
        r#"query($tree: ID!) {
            mediaList(treeId: $tree, filter: { eventFrom: 1900, eventTo: 1800 }) { totalCount }
        }"#,
        Some(json!({ "tree": tree_id })),
    )
    .await;
    assert!(
        resp["errors"].as_array().is_some_and(|e| !e.is_empty()),
        "a backwards range is refused, as REST answers 400: {resp}"
    );

    let page = |after: Option<String>| {
        let app = app.clone();
        let tree_id = tree_id.clone();
        async move {
            let resp = graphql(
                app,
                r#"query($tree: ID!, $after: String) {
                    mediaList(treeId: $tree, first: 2, after: $after, filter: { tags: ["village alpha"] }) {
                        totalCount pageInfo { hasNextPage endCursor }
                        edges { usageCount node { title } }
                    }
                }"#,
                Some(json!({ "tree": tree_id, "after": after })),
            )
            .await;
            data(&resp)["mediaList"].clone()
        }
    };
    let first = page(None).await;
    assert_eq!(first["totalCount"], 3, "{first}");
    assert_eq!(first["pageInfo"]["hasNextPage"], true);
    assert_eq!(first["edges"][0]["node"]["title"], "Census sheet");
    assert_eq!(first["edges"][0]["usageCount"], 1);
    assert_eq!(first["edges"][1]["usageCount"], 1);
    let rest = page(first["pageInfo"]["endCursor"].as_str().map(String::from)).await;
    assert_eq!(rest["edges"].as_array().unwrap().len(), 1, "{rest}");
    assert_eq!(rest["edges"][0]["node"]["title"], "Group photo");
    assert_eq!(rest["edges"][0]["usageCount"], 0);
    assert_eq!(rest["pageInfo"]["hasNextPage"], false);
}

#[tokio::test]
async fn the_media_facets_over_graphql_match_rest() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    gql_library_fixture(&app, &tree_id).await;

    let resp = graphql(
        app.clone(),
        r#"query($tree: ID!) {
            narrowed: mediaFacets(treeId: $tree, tags: ["survey"]) { tags { tag count } }
        }"#,
        Some(json!({ "tree": tree_id })),
    )
    .await;
    assert_eq!(
        data(&resp)["narrowed"]["tags"],
        json!([
            { "tag": "Survey", "count": 1 },
            { "tag": "Village Alpha", "count": 1 },
        ]),
        "with tags selected, the tags are counted among their documents"
    );

    let resp = graphql(
        app,
        r#"query($tree: ID!) {
            mediaFacets(treeId: $tree) {
                tags { tag count } kinds { kind count } categories { category count }
            }
        }"#,
        Some(json!({ "tree": tree_id })),
    )
    .await;
    let facets = &data(&resp)["mediaFacets"];
    assert_eq!(
        facets["tags"],
        json!([
            { "tag": "Survey", "count": 1 },
            { "tag": "Village Alpha", "count": 3 },
        ])
    );
    assert_eq!(
        facets["kinds"],
        json!([{ "kind": "IMAGE", "count": 2 }, { "kind": "PDF", "count": 1 }])
    );
    assert_eq!(
        facets["categories"],
        json!([{ "category": "CENSUS", "count": 1 }])
    );
}

/// As over REST, accents fold like case: "Église" and "EGLISE" are one tag,
/// spelled as first entered, and "eglise" removes it.
#[tokio::test]
async fn graphql_media_tags_fold_accents_like_case() {
    let app = setup_app().await;
    let tree_id = gql_tree(&app).await;
    let media = gql_document(&app, &tree_id, "Parish scan").await;
    for tag in ["Église", "EGLISE"] {
        gql_do(
            &app,
            GQL_ADD_TAG,
            json!({ "tree": tree_id, "id": media, "tag": tag }),
        )
        .await;
    }
    let tags = |app: axum::Router, tree_id: String, media: String| async move {
        let resp = graphql(
            app,
            r#"query($tree: ID!, $id: ID!) { media(treeId: $tree, id: $id) { tags } }"#,
            Some(json!({ "tree": tree_id, "id": media })),
        )
        .await;
        data(&resp)["media"]["tags"].clone()
    };
    assert_eq!(
        tags(app.clone(), tree_id.clone(), media.clone()).await,
        json!(["Église"])
    );
    gql_do(
        &app,
        r#"mutation($tree: ID!, $id: ID!, $tag: String!) {
            removeMediaTag(treeId: $tree, id: $id, tag: $tag)
        }"#,
        json!({ "tree": tree_id, "id": media, "tag": "eglise" }),
    )
    .await;
    assert_eq!(tags(app.clone(), tree_id, media).await, json!([]));
}

/// On a SQLite file, a query is answered while a write holds the single
/// writer — an import holds it for its whole transaction, played here by a
/// transaction the test keeps open — and a mutation waits for its turn, as
/// on REST.
#[tokio::test]
async fn queries_are_answered_while_a_write_holds_the_writer() {
    use oxidgene_db::sea_orm::{ConnectionTrait as _, TransactionTrait as _};

    let directory = tempfile::tempdir().unwrap();
    let state = AppState::new(
        common::setup_file_db(directory.path()).await,
        directory.path().join("media"),
    );
    let app = build_router(state.clone());
    let tree_id = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Held" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let import = state.db.begin().await.unwrap();
    import
        .execute_unprepared("UPDATE tree SET name = name")
        .await
        .unwrap();

    let within = std::time::Duration::from_secs(5);
    let query = format!(
        r#"{{ tree(id: "{tree_id}") {{ name personCount }} persons(treeId: "{tree_id}") {{ totalCount }} }}"#
    );
    let response = tokio::time::timeout(within, graphql(app.clone(), &query, None))
        .await
        .expect("a query does not wait for the writer");
    assert_eq!(data(&response)["tree"]["name"], "Held");

    let mutation = format!(
        r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
    );
    let write = tokio::spawn({
        let app = app.clone();
        async move { graphql(app, &mutation, None).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(!write.is_finished(), "a mutation waits for the writer");
    import.commit().await.unwrap();
    let response = tokio::time::timeout(within, write)
        .await
        .expect("the mutation proceeds once the writer is free")
        .unwrap();
    assert!(data(&response)["createPerson"]["id"].is_string());
}

/// GraphQL answers go through the same compression as REST ones: a client
/// accepting gzip gets a gzipped body.
#[tokio::test]
async fn graphql_responses_are_compressed_like_rest_ones() {
    let app = setup_app().await;
    let body = json!({ "query": "{ __schema { types { name description } } }" });
    let request = Request::builder()
        .method(Method::POST)
        .uri("/graphql")
        .header("content-type", "application/json")
        .header("accept-encoding", "gzip")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .map(|value| value.to_str().unwrap()),
        Some("gzip")
    );
}

/// The resolvers' own refusals carry a domain error, so they are reported
/// with its code like every other failure: a malformed `.gw`, an unknown
/// entity type, an image source missing its payload.
#[tokio::test]
async fn resolver_refusals_report_the_domain_error_code() {
    let app = setup_app().await;
    let tree_id = data(
        &graphql(
            app.clone(),
            r#"mutation { createTree(input: { name: "Refusals" }) { id } }"#,
            None,
        )
        .await,
    )["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for query in [
        r#"{ inspectGeneweb(gwBase64: "not base64!", fileName: "family.gw") { personCount } }"#
            .to_string(),
        format!(
            r#"{{ entityMedia(treeId: "{tree_id}", entityType: "nothing", entityId: "{tree_id}") {{ linkId }} }}"#
        ),
        format!(r#"{{ imageData(treeId: "{tree_id}", sources: [{{ kind: THUMBNAIL }}]) }}"#),
    ] {
        let response = graphql(app.clone(), &query, None).await;
        assert_eq!(
            response["errors"][0]["extensions"]["code"], "VALIDATION_ERROR",
            "{query}: {response}"
        );
    }
}

/// The GraphQL twin of `media_test.rs`'s
/// `a_remote_page_tile_draws_its_thumbnail_address` and
/// `a_thumbnail_address_is_a_web_address_of_a_remote_page`.
#[tokio::test]
async fn a_remote_page_thumbnail_address_over_graphql() {
    let (app, _root) = setup_app_with_media().await;
    let tree_id = tree_id_for(&app).await;
    let document_id = document_id_for(&app, &tree_id).await;
    let url = "https://archives.example.invalid/iiif/view-5/full/max/0/default.jpg";
    let thumbnail = "https://archives.example.invalid/images/view-5_thumbnail.jpg";

    let resp = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $doc: String!, $url: String!, $thumbnail: String) {
            uploadMedia(treeId: $tree, input: {
                documentId: $doc, fileName: "default.jpg", mimeType: "",
                filePath: $url, fileSize: 0, thumbnailUrl: $thumbnail,
                width: 3000, height: 2000
            }) { id thumbnailUrl width height }
        }"#,
        Some(json!({ "tree": tree_id, "doc": document_id, "url": url, "thumbnail": thumbnail })),
    )
    .await;
    let page = data(&resp)["uploadMedia"].clone();
    assert_eq!(page["thumbnailUrl"], thumbnail);
    assert_eq!(page["width"], 3000);
    let page_id = page["id"].as_str().unwrap().to_string();

    let bundle = data(
        &graphql(
            app.clone(),
            &format!(
                r#"{{ galleryBundle(treeId: "{tree_id}", mediaIds: ["{document_id}"], vignetteIds: []) {{ media {{ documentPreviews {{ kind url }} }} }} }}"#
            ),
            None,
        )
        .await,
    )["galleryBundle"]
        .clone();
    assert_eq!(
        bundle["media"][0]["documentPreviews"],
        json!([{ "kind": "REMOTE", "url": thumbnail }]),
        "{bundle}"
    );

    let update = r#"mutation($tree: ID!, $id: ID!, $input: UpdateMediaInput!) {
        updateMedia(treeId: $tree, id: $id, input: $input) { thumbnailUrl }
    }"#;
    // `null` clears it.
    let resp = graphql(
        app.clone(),
        update,
        Some(json!({ "tree": tree_id, "id": page_id, "input": { "thumbnailUrl": null } })),
    )
    .await;
    assert!(
        data(&resp)["updateMedia"]["thumbnailUrl"].is_null(),
        "{resp}"
    );

    // Only a web address, and only on a page held as one.
    let resp = graphql(
        app.clone(),
        update,
        Some(json!({ "tree": tree_id, "id": page_id, "input": { "thumbnailUrl": "file:///tmp/7.jpg" } })),
    )
    .await;
    assert_eq!(
        resp["errors"][0]["extensions"]["code"], "VALIDATION_ERROR",
        "{resp}"
    );
    let resp = graphql(
        app.clone(),
        update,
        Some(json!({ "tree": tree_id, "id": document_id, "input": { "thumbnailUrl": thumbnail } })),
    )
    .await;
    assert_eq!(
        resp["errors"][0]["extensions"]["code"], "VALIDATION_ERROR",
        "{resp}"
    );
    let resp = graphql(
        app.clone(),
        r#"mutation($tree: ID!, $doc: String!) {
            uploadMedia(treeId: $tree, input: {
                documentId: $doc, fileName: "7.jpg", mimeType: "image/jpeg",
                filePath: "scans/7.jpg", fileSize: 0,
                thumbnailUrl: "https://archives.example.invalid/7_thumb.jpg"
            }) { id }
        }"#,
        Some(json!({ "tree": tree_id, "doc": document_id })),
    )
    .await;
    assert_eq!(
        resp["errors"][0]["extensions"]["code"], "VALIDATION_ERROR",
        "{resp}"
    );
}

/// GraphQL twin of the REST test: a source documents the persons cited
/// directly, the person of a cited individual event, and the spouses of a
/// cited family or family event. A deleted couple has no spouses.
#[tokio::test]
async fn graphql_source_usage_resolves_family_and_event_citations() {
    let app = setup_app().await;
    let run = |query: String| {
        let app = app.clone();
        async move { data(&graphql(app, &query, None).await).clone() }
    };
    let tree_id = run(r#"mutation { createTree(input: { name: "Sample tree" }) { id } }"#.into())
        .await["createTree"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut persons = Vec::new();
    for _ in 0..4 {
        persons.push(
            run(format!(
                r#"mutation {{ createPerson(treeId: "{tree_id}", input: {{ sex: UNKNOWN }}) {{ id }} }}"#
            ))
            .await["createPerson"]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let [direct, husband, wife, lone] = [&persons[0], &persons[1], &persons[2], &persons[3]];
    let family_id = run(format!(
        r#"mutation {{ createFamily(treeId: "{tree_id}") {{ id }} }}"#
    ))
    .await["createFamily"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (person_id, role) in [(husband, "HUSBAND"), (wife, "WIFE")] {
        run(format!(
            r#"mutation {{ addSpouse(treeId: "{tree_id}", familyId: "{family_id}", input: {{ personId: "{person_id}", role: {role} }}) {{ id }} }}"#
        ))
        .await;
    }
    let marriage_id = run(format!(
        r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: MARRIAGE, familyId: "{family_id}" }}) {{ id }} }}"#
    ))
    .await["createEvent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let birth_id = run(format!(
        r#"mutation {{ createEvent(treeId: "{tree_id}", input: {{ eventType: BIRTH, personId: "{lone}" }}) {{ id }} }}"#
    ))
    .await["createEvent"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let source_id = run(format!(
        r#"mutation {{ createSource(treeId: "{tree_id}", input: {{ title: "Sample register" }}) {{ id }} }}"#
    ))
    .await["createSource"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let cite = |fields: String| {
        run(format!(
            r#"mutation {{ createCitation(treeId: "{tree_id}", input: {{ sourceId: "{source_id}", {fields} }}) {{ id }} }}"#
        ))
    };
    let usage = |expected: Vec<&String>| {
        let mut expected: Vec<String> = expected.into_iter().cloned().collect();
        expected.sort();
        let query = format!(
            r#"{{ sourceUsage(treeId: "{tree_id}", sourceId: "{source_id}") {{ personId }} }}"#
        );
        let run = &run;
        async move {
            let mut ids: Vec<String> = run(query).await["sourceUsage"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["personId"].as_str().unwrap().to_string())
                .collect();
            ids.sort();
            assert_eq!(ids, expected);
        }
    };

    cite(format!(
        r#"eventId: "{marriage_id}", familyId: "{family_id}""#
    ))
    .await;
    usage(vec![husband, wife]).await;

    cite(format!(r#"personId: "{direct}""#)).await;
    cite(format!(r#"eventId: "{birth_id}""#)).await;
    cite(format!(r#"familyId: "{family_id}""#)).await;
    usage(vec![direct, husband, wife, lone]).await;

    run(format!(
        r#"mutation {{ deleteFamily(treeId: "{tree_id}", id: "{family_id}") }}"#
    ))
    .await;
    usage(vec![direct, lone]).await;
}
