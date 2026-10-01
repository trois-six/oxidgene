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
