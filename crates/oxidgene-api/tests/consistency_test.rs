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

// ── Trees ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_tree_needs_a_name_on_both_surfaces() {
    let app = setup_app().await;
    let (status, body) = send(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "  " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let response = gql(
        &app,
        r#"mutation { createTree(input: { name: "  " }) { id } }"#,
        json!({}),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");

    let tree_id = common::new_tree(&app, "Named").await;
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response = gql(
        &app,
        r#"mutation($t: ID!) { updateTree(id: $t, input: { name: "" }) { id } }"#,
        json!({ "t": tree_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
    let response = gql(
        &app,
        r#"mutation($t: ID!) { duplicateTree(treeId: $t, name: "") { id } }"#,
        json!({ "t": tree_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
}

#[tokio::test]
async fn tree_settings_only_name_persons_of_the_tree() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Home").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let stranger = common::new_person(&app, &other_tree).await;
    let member = common::new_person(&app, &tree_id).await;

    for field in ["sosa_root_person_id", "self_person_id"] {
        let (status, body) = send(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}"),
            Some(json!({ field: stranger })),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{field}: {body}");
    }
    for field in ["sosaRootPersonId", "selfPersonId"] {
        let response = gql(
            &app,
            &format!(
                "mutation($t: ID!, $p: String!) {{ updateTree(id: $t, input: {{ {field}: $p }}) {{ id }} }}"
            ),
            json!({ "t": tree_id, "p": stranger }),
        )
        .await;
        assert_eq!(
            gql_error_code(&response),
            "NOT_FOUND",
            "{field}: {response}"
        );
    }

    // A person of the tree itself is accepted, and clearing works.
    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}"),
        Some(json!({ "sosa_root_person_id": member })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sosa_root_person_id"], member);
    let data = common::gql_ok(
        &app,
        "mutation($t: ID!) { updateTree(id: $t, input: { sosaRootPersonId: null }) { sosaRootPersonId } }",
        json!({ "t": tree_id }),
    )
    .await;
    assert!(data["updateTree"]["sosaRootPersonId"].is_null(), "{data}");
}

#[tokio::test]
async fn the_tree_list_reports_a_running_import_on_both_surfaces() {
    let app = setup_app().await;
    let idle = common::new_tree(&app, "Idle").await;
    let busy = common::new_tree(&app, "Busy").await;
    // No worker runs in the tests: the job stays queued.
    let (status, body) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{busy}/import-jobs?format=gedcom"),
        Some(json!("0 HEAD\n0 TRLR\n")),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let job_id = body["job_id"].as_str().unwrap().to_owned();

    let list = common::ok(&app, Method::GET, "/api/v1/trees", None).await;
    let node = |id: &str| {
        list["edges"]
            .as_array()
            .unwrap()
            .iter()
            .find(|edge| edge["node"]["id"] == id)
            .unwrap()["node"]
            .clone()
    };
    assert_eq!(node(&busy)["import_in_progress"], true);
    assert_eq!(node(&busy)["import_job_id"], job_id);
    assert_eq!(node(&idle)["import_in_progress"], false);
    assert!(node(&idle)["import_job_id"].is_null());

    let data = common::gql_ok(
        &app,
        "{ trees { edges { node { id importInProgress importJobId } } } }",
        json!({}),
    )
    .await;
    let edges = data["trees"]["edges"].as_array().unwrap();
    let gql_node =
        |id: &str| edges.iter().find(|edge| edge["node"]["id"] == id).unwrap()["node"].clone();
    assert_eq!(gql_node(&busy)["importInProgress"], true);
    assert_eq!(gql_node(&busy)["importJobId"], job_id);
    assert_eq!(gql_node(&idle)["importInProgress"], false);

    // A single tree answers the same.
    let data = common::gql_ok(
        &app,
        "query($t: ID!) { tree(id: $t) { importInProgress importJobId } }",
        json!({ "t": busy }),
    )
    .await;
    assert_eq!(data["tree"]["importJobId"], job_id);
}

// ── Persons ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_person_list_filters_by_name_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Names").await;
    for (given, surname) in [("Alice", "Ashdown"), ("Bernard", "Birchley")] {
        let person_id = common::new_person(&app, &tree_id).await;
        common::ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
            Some(json!({
                "name_type": "birth",
                "given_names": given,
                "surname": surname,
                "is_primary": true
            })),
        )
        .await;
    }

    let list = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons?search=Birch"),
        None,
    )
    .await;
    assert_eq!(list["total_count"], 1, "{list}");
    let data = common::gql_ok(
        &app,
        r#"query($t: ID!) { persons(treeId: $t, search: "Birch") { totalCount } }"#,
        json!({ "t": tree_id }),
    )
    .await;
    assert_eq!(data["persons"]["totalCount"], 1, "{data}");

    let all = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons"),
        None,
    )
    .await;
    assert_eq!(all["total_count"], 2);
}

#[tokio::test]
async fn ancestry_depth_is_bounded_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Depth").await;
    let person_id = common::new_person(&app, &tree_id).await;

    for depth in ["-1", "0", "65"] {
        let (status, body) = send(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/ancestors?max_depth={depth}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{depth}: {body}");
        let response = gql(
            &app,
            "query($t: ID!, $p: ID!, $d: Int!) { descendants(treeId: $t, personId: $p, maxDepth: $d) { depth } }",
            json!({ "t": tree_id, "p": person_id, "d": depth.parse::<i32>().unwrap() }),
        )
        .await;
        assert_eq!(
            gql_error_code(&response),
            "VALIDATION_ERROR",
            "{depth}: {response}"
        );
    }
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/descendants?max_depth=64"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_sosa_root_of_another_tree_is_no_root() {
    let db = common::setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = common::new_tree(&app, "Home").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let member = common::new_person(&app, &tree_id).await;
    let stranger = common::new_person(&app, &other_tree).await;

    // Stored before the settings were checked: the API refuses it now.
    oxidgene_db::repo::TreeRepo::update(
        &db,
        tree_id.parse().unwrap(),
        oxidgene_db::repo::TreeChanges {
            sosa_root_person_id: Some(Some(stranger.parse().unwrap())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/sosa/1"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let data = common::gql_ok(
        &app,
        "query($t: ID!) { personBySosa(treeId: $t, number: 1) { id } }",
        json!({ "t": tree_id }),
    )
    .await;
    assert!(data["personBySosa"].is_null(), "{data}");
    let person = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{member}"),
        None,
    )
    .await;
    assert!(person["sosa_number"].is_null(), "{person}");
}

// ── Person names ────────────────────────────────────────────────────────

/// A person of `tree_id` with one primary name; the person's id and the
/// name's.
async fn named_person(app: &axum::Router, tree_id: &str, given: &str) -> (String, String) {
    let person_id = common::new_person(app, tree_id).await;
    let name = common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(json!({
            "name_type": "birth",
            "given_names": given,
            "surname": "Coldwell",
            "is_primary": true
        })),
    )
    .await;
    (person_id, name["id"].as_str().unwrap().to_owned())
}

#[tokio::test]
async fn a_name_is_only_reachable_through_its_own_person() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Names").await;
    let (alice, alice_name) = named_person(&app, &tree_id, "Alice").await;
    let (bernard, _) = named_person(&app, &tree_id, "Bernard").await;

    // Bernard's path, Alice's name.
    let uri = format!("/api/v1/trees/{tree_id}/persons/{bernard}/names/{alice_name}");
    let (status, _) = send(&app, Method::PUT, &uri, Some(json!({ "given_names": "X" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(&app, Method::DELETE, &uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let vars = json!({ "t": tree_id, "p": bernard, "n": alice_name });
    let response = gql(
        &app,
        r#"mutation($t: ID!, $p: ID!, $n: ID!) {
            updatePersonName(treeId: $t, personId: $p, id: $n, input: { givenNames: "X" }) { id }
        }"#,
        vars.clone(),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
    let response = gql(
        &app,
        "mutation($t: ID!, $p: ID!, $n: ID!) { deletePersonName(treeId: $t, personId: $p, id: $n) }",
        vars,
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");

    // Untouched.
    let profile = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{alice}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_name"]["given_names"], "Alice", "{profile}");

    // Through its own person, GraphQL rewrites the owner's projection as REST
    // does.
    common::gql_ok(
        &app,
        r#"mutation($t: ID!, $p: ID!, $n: ID!) {
            updatePersonName(treeId: $t, personId: $p, id: $n, input: { givenNames: "Alicia" }) { id }
        }"#,
        json!({ "t": tree_id, "p": alice, "n": alice_name }),
    )
    .await;
    let profile = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{alice}"),
        None,
    )
    .await;
    assert_eq!(
        profile["primary_name"]["given_names"], "Alicia",
        "{profile}"
    );
}

// ── Families ────────────────────────────────────────────────────────────

/// A new family of `tree_id`; its id.
async fn new_family(app: &axum::Router, tree_id: &str) -> String {
    common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn a_family_link_is_only_removed_through_its_own_family() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Links").await;
    let first = new_family(&app, &tree_id).await;
    let second = new_family(&app, &tree_id).await;
    let parent = common::new_person(&app, &tree_id).await;
    let child = common::new_person(&app, &tree_id).await;
    let spouse_link = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families/{second}/spouses"),
        Some(json!({ "person_id": parent, "role": "husband" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let child_link = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families/{second}/children"),
        Some(json!({ "person_id": child, "child_type": "biological" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Through the first family, the second's links are not found.
    for (kind, link) in [("spouses", &spouse_link), ("children", &child_link)] {
        let (status, _) = send(
            &app,
            Method::DELETE,
            &format!("/api/v1/trees/{tree_id}/families/{first}/{kind}/{link}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{kind}");
    }
    for (mutation, link) in [("removeSpouse", &spouse_link), ("removeChild", &child_link)] {
        let response = gql(
            &app,
            &format!("mutation($t: ID!, $f: ID!, $l: ID!) {{ {mutation}(treeId: $t, familyId: $f, id: $l) }}"),
            json!({ "t": tree_id, "f": first, "l": link }),
        )
        .await;
        assert_eq!(
            gql_error_code(&response),
            "NOT_FOUND",
            "{mutation}: {response}"
        );
    }

    // Both links are still there, and the parent's projection still shows
    // the family.
    for kind in ["spouses", "children"] {
        let links = common::ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/families/{second}/{kind}"),
            None,
        )
        .await;
        assert_eq!(links.as_array().unwrap().len(), 1, "{kind}");
    }
    let profile = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{parent}"),
        None,
    )
    .await;
    assert_eq!(
        profile["families_as_spouse"].as_array().unwrap().len(),
        1,
        "{profile}"
    );

    // Through its own family, GraphQL removes it and refreshes the parent.
    common::gql_ok(
        &app,
        "mutation($t: ID!, $f: ID!, $l: ID!) { removeSpouse(treeId: $t, familyId: $f, id: $l) }",
        json!({ "t": tree_id, "f": second, "l": spouse_link }),
    )
    .await;
    let profile = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{parent}"),
        None,
    )
    .await;
    assert!(
        profile["families_as_spouse"].as_array().unwrap().is_empty(),
        "{profile}"
    );
}

#[tokio::test]
async fn a_malformed_family_update_is_refused() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Families").await;
    let family_id = new_family(&app, &tree_id).await;
    let uri = format!("/api/v1/trees/{tree_id}/families/{family_id}");

    let (status, body) = send(&app, Method::PUT, &uri, Some(json!({ "privacy": 42 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    let response = gql(
        &app,
        "mutation($t: ID!, $f: ID!) { updateFamily(treeId: $t, id: $f, input: { privacy: 42 }) { id } }",
        json!({ "t": tree_id, "f": family_id }),
    )
    .await;
    assert!(response["errors"].is_array(), "{response}");

    // No body at all still touches the family.
    let (status, _) = send(&app, Method::PUT, &uri, None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = send(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "privacy": "private" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["privacy"], "private");
}

// ── Events ──────────────────────────────────────────────────────────────

/// A birth of `person_id` in `tree_id`; its id.
async fn new_birth(app: &axum::Router, tree_id: &str, person_id: &str) -> String {
    common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events"),
        Some(json!({ "event_type": "birth", "date_value": "1850", "person_id": person_id })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A place named `name` in `tree_id`; its id.
async fn new_place(app: &axum::Router, tree_id: &str, name: &str) -> String {
    common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/places"),
        Some(json!({ "name": name })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn an_event_cannot_move_to_a_place_of_another_tree() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Home").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let event_id = new_birth(&app, &tree_id, &person_id).await;
    let foreign_place = new_place(&app, &other_tree, "Northfield").await;

    let (status, body) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        Some(json!({ "place_id": foreign_place })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let response = gql(
        &app,
        "mutation($t: ID!, $e: ID!, $p: String!) { updateEvent(treeId: $t, id: $e, input: { placeId: $p }) { id } }",
        json!({ "t": tree_id, "e": event_id, "p": foreign_place }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");

    let event = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events/{event_id}"),
        None,
    )
    .await;
    assert!(event["place_id"].is_null(), "{event}");
}

#[tokio::test]
async fn an_event_filter_naming_another_tree_is_not_found_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Home").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let stranger = common::new_person(&app, &other_tree).await;

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events?person_id={stranger}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        "query($t: ID!, $p: ID!) { events(treeId: $t, personId: $p) { totalCount } }",
        json!({ "t": tree_id, "p": stranger }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

#[tokio::test]
async fn a_witness_is_only_removed_through_its_own_event() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Witnesses").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let witness_person = common::new_person(&app, &tree_id).await;
    let first = new_birth(&app, &tree_id, &person_id).await;
    let second = new_birth(&app, &tree_id, &person_id).await;
    let witness_id = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/events/{second}/witnesses"),
        Some(json!({ "person_id": witness_person, "relation": "godfather" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/events/{first}/witnesses/{witness_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        "mutation($t: ID!, $e: ID!, $w: ID!) { removeEventWitness(treeId: $t, id: $w, eventId: $e) }",
        json!({ "t": tree_id, "e": first, "w": witness_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
    let witnesses = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/events/{second}/witnesses"),
        None,
    )
    .await;
    assert_eq!(witnesses.as_array().unwrap().len(), 1);

    common::gql_ok(
        &app,
        "mutation($t: ID!, $e: ID!, $w: ID!) { removeEventWitness(treeId: $t, id: $w, eventId: $e) }",
        json!({ "t": tree_id, "e": second, "w": witness_id }),
    )
    .await;
}

// ── Places, sources, citations, notes ───────────────────────────────────

#[tokio::test]
async fn a_place_needs_a_name_and_a_source_a_title_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Blank").await;
    let place_id = new_place(&app, &tree_id, "Southmere").await;
    let source_id = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(json!({ "title": "Parish register" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    for (method, uri, body) in [
        (
            Method::POST,
            format!("/api/v1/trees/{tree_id}/places"),
            json!({ "name": " " }),
        ),
        (
            Method::PUT,
            format!("/api/v1/trees/{tree_id}/places/{place_id}"),
            json!({ "name": "" }),
        ),
        (
            Method::POST,
            format!("/api/v1/trees/{tree_id}/sources"),
            json!({ "title": " " }),
        ),
        (
            Method::PUT,
            format!("/api/v1/trees/{tree_id}/sources/{source_id}"),
            json!({ "title": "" }),
        ),
    ] {
        let (status, response) = send(&app, method, &uri, Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {response}");
    }

    let vars = json!({ "t": tree_id, "p": place_id, "s": source_id });
    for mutation in [
        r#"mutation($t: ID!) { createPlace(treeId: $t, input: { name: " " }) { id } }"#,
        r#"mutation($t: ID!, $p: ID!) { updatePlace(treeId: $t, id: $p, input: { name: "" }) { id } }"#,
        r#"mutation($t: ID!) { createSource(treeId: $t, input: { title: " " }) { id } }"#,
        r#"mutation($t: ID!, $s: ID!) { updateSource(treeId: $t, id: $s, input: { title: "" }) { id } }"#,
    ] {
        let response = gql(&app, mutation, vars.clone()).await;
        assert_eq!(
            gql_error_code(&response),
            "VALIDATION_ERROR",
            "{mutation}: {response}"
        );
    }

    let place = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/places/{place_id}"),
        None,
    )
    .await;
    assert_eq!(place["name"], "Southmere");
}

/// A citation's confidence is optional: omitted at creation it is not
/// assessed (null), an update leaves it alone when omitted and clears it on
/// null — on both surfaces.
#[tokio::test]
async fn a_citation_confidence_is_optional_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Assessed").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let source_id = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/sources"),
        Some(json!({ "title": "Parish register" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // REST.
    let created = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/citations"),
        Some(json!({ "source_id": source_id, "person_id": person_id })),
    )
    .await;
    assert!(created["confidence"].is_null(), "{created}");
    let uri = format!(
        "/api/v1/trees/{tree_id}/citations/{}",
        created["id"].as_str().unwrap()
    );
    let assessed = common::ok(
        &app,
        Method::PUT,
        &uri,
        Some(json!({ "confidence": "low" })),
    )
    .await;
    assert_eq!(assessed["confidence"], "low");
    let kept = common::ok(&app, Method::PUT, &uri, Some(json!({ "page": "f. 2" }))).await;
    assert_eq!(kept["confidence"], "low");
    let cleared = common::ok(&app, Method::PUT, &uri, Some(json!({ "confidence": null }))).await;
    assert!(cleared["confidence"].is_null(), "{cleared}");

    // GraphQL.
    let vars = json!({ "t": tree_id, "s": source_id, "p": person_id });
    let created = common::gql_ok(
        &app,
        "mutation($t: ID!, $s: String!, $p: String!) { createCitation(treeId: $t, input: { sourceId: $s, personId: $p }) { id confidence } }",
        vars,
    )
    .await;
    let citation = &created["createCitation"];
    assert!(citation["confidence"].is_null(), "{created}");
    let vars = json!({ "t": tree_id, "c": citation["id"] });
    for (input, expected) in [
        ("{ confidence: LOW }", json!("LOW")),
        (r#"{ page: "f. 2" }"#, json!("LOW")),
        ("{ confidence: null }", serde_json::Value::Null),
    ] {
        let updated = common::gql_ok(
            &app,
            &format!(
                "mutation($t: ID!, $c: ID!) {{ updateCitation(treeId: $t, id: $c, input: {input}) {{ confidence }} }}"
            ),
            vars.clone(),
        )
        .await;
        assert_eq!(updated["updateCitation"]["confidence"], expected, "{input}");
    }
}

#[tokio::test]
async fn a_citation_filter_naming_another_tree_is_not_found_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Home").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let foreign_source = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{other_tree}/sources"),
        Some(json!({ "title": "Census" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/citations?source_id={foreign_source}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        "query($t: ID!, $s: ID!) { citations(treeId: $t, sourceId: $s) { totalCount } }",
        json!({ "t": tree_id, "s": foreign_source }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

#[tokio::test]
async fn a_single_note_is_readable_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Notes").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let note_id = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/notes"),
        Some(json!({ "text": "Moved to the coast." })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let note = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(note["text"], "Moved to the coast.");
    let data = common::gql_ok(
        &app,
        "query($t: ID!, $n: ID!) { note(treeId: $t, id: $n) { id text } }",
        json!({ "t": tree_id, "n": note_id }),
    )
    .await;
    assert_eq!(data["note"]["text"], "Moved to the coast.");

    // Through another tree, it is not there.
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{other_tree}/notes/{note_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let data = common::gql_ok(
        &app,
        "query($t: ID!, $n: ID!) { note(treeId: $t, id: $n) { id } }",
        json!({ "t": other_tree, "n": note_id }),
    )
    .await;
    assert!(data["note"].is_null(), "{data}");
}

// ── Media ───────────────────────────────────────────────────────────────

/// A document of `tree_id` with one page naming a remote picture, linked to
/// `person_id`; the document's id and the page's.
async fn linked_document(app: &axum::Router, tree_id: &str, person_id: &str) -> (String, String) {
    let document = common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/document"),
        Some(json!({ "title": "Portrait" })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let page = common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(json!({
            "document_id": document,
            "file_name": "portrait.jpg",
            "mime_type": "image/jpeg",
            "file_path": "https://example.org/portrait.jpg",
            "file_size": 0
        })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    common::ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media-links"),
        Some(json!({ "media_id": document, "person_id": person_id })),
    )
    .await;
    (document, page)
}

/// The projection of `person_id`.
async fn profile(app: &axum::Router, tree_id: &str, person_id: &str) -> serde_json::Value {
    common::ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/profiles/{person_id}"),
        None,
    )
    .await
}

#[tokio::test]
async fn deleting_a_media_rewrites_the_cards_that_drew_it_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Album").await;
    let rest_person = common::new_person(&app, &tree_id).await;
    let gql_person = common::new_person(&app, &tree_id).await;
    let (rest_document, _) = linked_document(&app, &tree_id, &rest_person).await;
    let (gql_document, _) = linked_document(&app, &tree_id, &gql_person).await;
    for person in [&rest_person, &gql_person] {
        let card = profile(&app, &tree_id, person).await;
        assert!(card["primary_media"].is_object(), "{card}");
        assert_eq!(card["media_count"], 1);
    }

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media/{rest_document}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    common::gql_ok(
        &app,
        "mutation($t: ID!, $m: ID!) { deleteMedia(treeId: $t, id: $m) }",
        json!({ "t": tree_id, "m": gql_document }),
    )
    .await;

    for person in [&rest_person, &gql_person] {
        let card = profile(&app, &tree_id, person).await;
        assert!(card["primary_media"].is_null(), "{card}");
        assert_eq!(card["media_count"], 0, "{card}");
    }
}

#[tokio::test]
async fn deleting_the_page_a_card_drew_rewrites_the_card_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Pages").await;
    let rest_person = common::new_person(&app, &tree_id).await;
    let gql_person = common::new_person(&app, &tree_id).await;
    let (rest_document, rest_page) = linked_document(&app, &tree_id, &rest_person).await;
    let (gql_document, gql_page) = linked_document(&app, &tree_id, &gql_person).await;

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/media/{rest_document}/pages/{rest_page}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    common::gql_ok(
        &app,
        "mutation($t: ID!, $d: ID!, $p: ID!) { deleteMediaPage(treeId: $t, documentId: $d, pageId: $p) }",
        json!({ "t": tree_id, "d": gql_document, "p": gql_page }),
    )
    .await;

    // The documents stand, linked and empty: nothing left to draw.
    for person in [&rest_person, &gql_person] {
        let card = profile(&app, &tree_id, person).await;
        assert!(card["primary_media"].is_null(), "{card}");
    }
}

#[tokio::test]
async fn media_writes_validate_alike_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Media").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let (document, page) = linked_document(&app, &tree_id, &person_id).await;
    let foreign_place = new_place(&app, &other_tree, "Westford").await;

    // A page needs a file name.
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media"),
        Some(json!({
            "document_id": document,
            "file_name": " ",
            "mime_type": "image/jpeg",
            "file_path": "https://example.org/x.jpg",
            "file_size": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let response = gql(
        &app,
        r#"mutation($t: ID!, $d: String!) {
            uploadMedia(treeId: $t, input: {
                documentId: $d, fileName: " ", mimeType: "image/jpeg",
                filePath: "https://example.org/x.jpg", fileSize: 0
            }) { id }
        }"#,
        json!({ "t": tree_id, "d": document }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");

    // A place of another tree is not found, and the error is classified.
    let (status, _) = send(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree_id}/media/{page}"),
        Some(json!({ "place_id": foreign_place })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        "mutation($t: ID!, $m: ID!, $p: String!) { updateMedia(treeId: $t, id: $m, input: { placeId: $p }) { id } }",
        json!({ "t": tree_id, "m": page, "p": foreign_place }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");

    // A stored-file rule refused by the service keeps its code over GraphQL.
    let response = gql(
        &app,
        "mutation($t: ID!, $m: ID!) { updateMedia(treeId: $t, id: $m, input: { width: 10 }) { id } }",
        json!({ "t": tree_id, "m": page }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "VALIDATION_ERROR", "{response}");
}

// ── Vignettes and media links ───────────────────────────────────────────

#[tokio::test]
async fn a_vignette_only_names_persons_and_events_of_its_tree() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Crops").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let stranger = common::new_person(&app, &other_tree).await;
    let foreign_event = new_birth(&app, &other_tree, &stranger).await;
    let (_, page) = linked_document(&app, &tree_id, &person_id).await;

    // Creation.
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/{page}/vignettes"),
        Some(json!({ "x": 0, "y": 0, "width": 10, "height": 10, "person_id": stranger })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        r#"mutation($t: ID!, $m: String!, $p: String!) {
            createVignette(treeId: $t, input: { mediaId: $m, x: 0, y: 0, width: 10, height: 10, personId: $p }) { id }
        }"#,
        json!({ "t": tree_id, "m": page, "p": stranger }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");

    // Update.
    let vignette = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/{page}/vignettes"),
        Some(json!({ "x": 0, "y": 0, "width": 10, "height": 10, "person_id": person_id })),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for body in [
        json!({ "person_id": stranger }),
        json!({ "event_id": foreign_event }),
    ] {
        let (status, response) = send(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}/vignettes/{vignette}"),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
    }
    for input in ["personId: $x", "eventId: $x"] {
        let response = gql(
            &app,
            &format!(
                "mutation($t: ID!, $v: ID!, $x: String!) {{ updateVignette(treeId: $t, id: $v, input: {{ {input} }}) {{ id }} }}"
            ),
            json!({ "t": tree_id, "v": vignette, "x": if input.starts_with("person") { &stranger } else { &foreign_event } }),
        )
        .await;
        assert_eq!(
            gql_error_code(&response),
            "NOT_FOUND",
            "{input}: {response}"
        );
    }
}

#[tokio::test]
async fn deleting_a_portrait_crop_rewrites_the_card_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Crops").await;
    let rest_person = common::new_person(&app, &tree_id).await;
    let gql_person = common::new_person(&app, &tree_id).await;
    let mut crops = Vec::new();
    for person in [&rest_person, &gql_person] {
        let (_, page) = linked_document(&app, &tree_id, person).await;
        let vignette = common::ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media/{page}/vignettes"),
            Some(json!({ "x": 1, "y": 1, "width": 5, "height": 5, "person_id": person })),
        )
        .await["id"]
            .as_str()
            .unwrap()
            .to_owned();
        common::ok(
            &app,
            Method::PUT,
            &format!("/api/v1/trees/{tree_id}/persons/{person}/portrait"),
            Some(json!({ "vignette_id": vignette })),
        )
        .await;
        let card = profile(&app, &tree_id, person).await;
        assert_eq!(card["primary_media"]["vignette_id"], vignette, "{card}");
        crops.push(vignette);
    }

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}/vignettes/{}", crops[0]),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    common::gql_ok(
        &app,
        "mutation($t: ID!, $v: ID!) { deleteVignette(treeId: $t, id: $v) }",
        json!({ "t": tree_id, "v": crops[1] }),
    )
    .await;

    // Back to the first linked picture, whole.
    for person in [&rest_person, &gql_person] {
        let card = profile(&app, &tree_id, person).await;
        assert!(card["primary_media"].is_object(), "{card}");
        assert!(card["primary_media"]["vignette_id"].is_null(), "{card}");
    }
}

#[tokio::test]
async fn linking_a_picture_over_graphql_rewrites_the_card_as_rest_does() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Links").await;
    let owner = common::new_person(&app, &tree_id).await;
    let person_id = common::new_person(&app, &tree_id).await;
    let (document, _) = linked_document(&app, &tree_id, &owner).await;

    let data = common::gql_ok(
        &app,
        "mutation($t: ID!, $m: String!, $p: String!) { createMediaLink(treeId: $t, input: { mediaId: $m, personId: $p }) { id } }",
        json!({ "t": tree_id, "m": document, "p": person_id }),
    )
    .await;
    let card = profile(&app, &tree_id, &person_id).await;
    assert_eq!(card["media_count"], 1, "{card}");
    assert!(card["primary_media"].is_object(), "{card}");

    common::gql_ok(
        &app,
        "mutation($t: ID!, $l: ID!) { deleteMediaLink(treeId: $t, id: $l) }",
        json!({ "t": tree_id, "l": data["createMediaLink"]["id"] }),
    )
    .await;
    let card = profile(&app, &tree_id, &person_id).await;
    assert_eq!(card["media_count"], 0, "{card}");
    assert!(card["primary_media"].is_null(), "{card}");
}

// ── Import and export ───────────────────────────────────────────────────

/// The export entries of `tree_id`'s audit log.
async fn exports(app: &axum::Router, tree_id: &str) -> Vec<serde_json::Value> {
    common::ok(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/audit?category=export"),
        None,
    )
    .await["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| edge["node"].clone())
        .collect()
}

#[tokio::test]
async fn a_gedcom_export_is_audited_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Exported").await;

    common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/gedcom/export"),
        None,
    )
    .await;
    assert_eq!(exports(&app, &tree_id).await.len(), 1);

    let data = common::gql_ok(
        &app,
        "query($t: ID!) { exportGedcom(treeId: $t) { gedcom } }",
        json!({ "t": tree_id }),
    )
    .await;
    assert!(
        data["exportGedcom"]["gedcom"]
            .as_str()
            .unwrap()
            .starts_with("0 HEAD")
    );
    let entries = exports(&app, &tree_id).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(
        entries
            .iter()
            .all(|entry| entry["details"]["format"] == "gedcom")
    );
}

#[tokio::test]
async fn job_status_reads_alike_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Jobs").await;
    let import = common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/import-jobs?format=gedcom"),
        Some(json!("0 HEAD\n0 TRLR\n")),
    )
    .await["job_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let rest = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{import}"),
        None,
    )
    .await;
    let data = common::gql_ok(
        &app,
        "query($t: ID!, $j: ID!) { importJobStatus(treeId: $t, jobId: $j) { phase done total error } }",
        json!({ "t": tree_id, "j": import }),
    )
    .await;
    let gql_status = &data["importJobStatus"];
    assert_eq!(rest["phase"], gql_status["phase"]);
    assert_eq!(rest["done"], gql_status["done"]);
    assert_eq!(rest["total"], gql_status["total"]);
    assert!(rest.get("result").is_none(), "{rest}");

    // An import job is not an export job, on either surface.
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/export-jobs/{import}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let response = gql(
        &app,
        "query($t: ID!, $j: ID!) { exportJobStatus(treeId: $t, jobId: $j) { phase } }",
        json!({ "t": tree_id, "j": import }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}

// ── Pedigrees ───────────────────────────────────────────────────────────

#[tokio::test]
async fn pedigree_depths_are_bounded_alike_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Pedigree").await;
    let root = common::new_person(&app, &tree_id).await;

    // Within the limit, both answer.
    common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/pedigree/{root}?ancestor_depth=10&descendant_depth=0"),
        None,
    )
    .await;
    common::gql_ok(
        &app,
        "query($t: ID!, $r: ID!) { pedigree(treeId: $t, rootPersonId: $r, ancestorDepth: 10, descendantDepth: 0) { ancestorDepthLoaded } }",
        json!({ "t": tree_id, "r": root }),
    )
    .await;

    // Past it, or negative, both refuse.
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/pedigree/{root}?ancestor_depth=11&descendant_depth=0"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/pedigrees"),
        Some(json!({ "root_person_ids": [root], "ancestor_depth": 2, "descendant_depth": 11 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = send(
        &app,
        Method::PATCH,
        &format!(
            "/api/v1/trees/{tree_id}/pedigree/{root}/expand?direction=ancestors&from_depth=2&to_depth=11"
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let vars = json!({ "t": tree_id, "r": root });
    for operation in [
        "query($t: ID!, $r: ID!) { pedigree(treeId: $t, rootPersonId: $r, ancestorDepth: -1, descendantDepth: 0) { ancestorDepthLoaded } }",
        "query($t: ID!, $r: ID!) { pedigree(treeId: $t, rootPersonId: $r, ancestorDepth: 11, descendantDepth: 0) { ancestorDepthLoaded } }",
        "query($t: ID!, $r: ID!) { pedigrees(treeId: $t, rootPersonIds: [$r], ancestorDepth: 2, descendantDepth: -3) { rootPersonId } }",
        "mutation($t: ID!, $r: ID!) { expandPedigree(treeId: $t, rootPersonId: $r, direction: ANCESTORS, fromDepth: 2, toDepth: 11) { ancestorDepthLoaded } }",
        "mutation($t: ID!, $r: ID!) { expandPedigree(treeId: $t, rootPersonId: $r, direction: ANCESTORS, fromDepth: -2, toDepth: 3) { ancestorDepthLoaded } }",
    ] {
        let response = gql(&app, operation, vars.clone()).await;
        assert_eq!(
            gql_error_code(&response),
            "VALIDATION_ERROR",
            "{operation}: {response}"
        );
    }
}

// ── GraphQL nested lists ────────────────────────────────────────────────

#[tokio::test]
async fn graphql_nested_lists_are_complete_past_a_hundred() {
    use oxidgene_core::{Calendar, Confidence, DateQualifier, EventType, SpouseRole};
    use oxidgene_db::repo::{
        CitationRepo, EventRepo, FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaRepo, SourceRepo,
    };
    use uuid::Uuid;

    const MANY: usize = 101;
    let db = common::setup_db().await;
    let app = common::app_on(db.clone());
    let tree_id = common::new_tree(&app, "Large").await;
    let person_id = common::new_person(&app, &tree_id).await;
    let (tree, person): (Uuid, Uuid) = (tree_id.parse().unwrap(), person_id.parse().unwrap());
    let event = |event_type| {
        let db = db.clone();
        async move {
            EventRepo::create(
                &db,
                Uuid::now_v7(),
                tree,
                event_type,
                None,
                None,
                None,
                Some(person),
                None,
                None,
                DateQualifier::default(),
                None,
                Calendar::default(),
                None,
            )
            .await
            .unwrap()
        }
    };
    let birth = event(EventType::Birth).await;
    let mut family_ids = Vec::new();
    for i in 0..MANY {
        let family = FamilyRepo::create(&db, Uuid::now_v7(), tree).await.unwrap();
        FamilySpouseRepo::create(
            &db,
            Uuid::now_v7(),
            family.id,
            person,
            SpouseRole::Husband,
            0,
        )
        .await
        .unwrap();
        family_ids.push(family.id);
        let source = SourceRepo::create(
            &db,
            Uuid::now_v7(),
            tree,
            format!("Register {i}"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        for (person_id, event_id) in [(Some(person), None), (None, Some(birth.id))] {
            CitationRepo::create(
                &db,
                Uuid::now_v7(),
                source.id,
                person_id,
                event_id,
                None,
                None,
                Some(Confidence::Medium),
                None,
            )
            .await
            .unwrap();
        }
        let document = MediaRepo::create_document(
            &db,
            Uuid::now_v7(),
            tree,
            Some(format!("Scan {i}")),
            chrono::Utc::now(),
        )
        .await
        .unwrap();
        for (person_id, event_id) in [(Some(person), None), (None, Some(birth.id))] {
            MediaLinkRepo::create(
                &db,
                Uuid::now_v7(),
                document.id,
                person_id,
                event_id,
                None,
                None,
                i as i32,
            )
            .await
            .unwrap();
        }
        event(EventType::Residence).await;
    }

    let data = common::gql_ok(
        &app,
        r#"query($t: ID!, $p: ID!, $b: ID!) {
            person(treeId: $t, id: $p) {
                families { id spouses { person { id } } }
                citations { id }
                media { id }
                events { id }
            }
            event(treeId: $t, id: $b) { citations { id } media { id } }
            tree(id: $t) { personCount familyCount }
        }"#,
        json!({ "t": tree_id, "p": person_id, "b": birth.id }),
    )
    .await;
    let person = &data["person"];
    let len = |value: &serde_json::Value| value.as_array().unwrap().len();
    assert_eq!(len(&person["families"]), MANY);
    assert!(
        person["families"]
            .as_array()
            .unwrap()
            .iter()
            .all(|family| family["spouses"][0]["person"]["id"] == person_id)
    );
    assert_eq!(len(&person["citations"]), MANY);
    assert_eq!(len(&person["media"]), MANY);
    assert_eq!(len(&person["events"]), MANY + 1);
    assert_eq!(len(&data["event"]["citations"]), MANY);
    assert_eq!(len(&data["event"]["media"]), MANY);
    assert_eq!(data["tree"]["personCount"], 1);
    assert_eq!(data["tree"]["familyCount"], MANY);
}

// ── Audit log ───────────────────────────────────────────────────────────

#[tokio::test]
async fn a_single_audit_entry_is_readable_on_both_surfaces() {
    let app = setup_app().await;
    let tree_id = common::new_tree(&app, "Audited").await;
    let other_tree = common::new_tree(&app, "Elsewhere").await;
    common::new_person(&app, &tree_id).await;
    let page = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/audit"),
        None,
    )
    .await;
    let newest = page["edges"][0]["node"].clone();
    let entry_id = newest["id"].as_str().unwrap().to_owned();

    let entry = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/audit/{entry_id}"),
        None,
    )
    .await;
    assert_eq!(entry, newest);
    let data = common::gql_ok(
        &app,
        "query($t: ID!, $e: ID!) { auditEntry(treeId: $t, id: $e) { id } }",
        json!({ "t": tree_id, "e": entry_id }),
    )
    .await;
    assert_eq!(data["auditEntry"]["id"], entry_id);

    // Another tree's log does not hold it.
    let (status, body) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{other_tree}/audit/{entry_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let response = gql(
        &app,
        "query($t: ID!, $e: ID!) { auditEntry(treeId: $t, id: $e) { id } }",
        json!({ "t": other_tree, "e": entry_id }),
    )
    .await;
    assert_eq!(gql_error_code(&response), "NOT_FOUND", "{response}");
}
