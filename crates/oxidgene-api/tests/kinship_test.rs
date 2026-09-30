//! Integration tests for the kinship operation, through REST and GraphQL
//! alike.
//!
//! The path search itself is covered by the service's unit tests; these check
//! what both surfaces add around it: tree scoping, validation, and the person
//! rows that come with the paths.

mod common;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use common::{send, setup_app};

async fn create_tree(app: &axum::Router) -> String {
    let (status, body) = send(
        app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "Test Tree" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_str().unwrap().to_string()
}

async fn create_person(app: &axum::Router, tree_id: &str, given_names: &str) -> String {
    let (status, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons"),
        Some(json!({ "sex": "unknown" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let person_id = body["id"].as_str().unwrap().to_string();
    let (status, _) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
        Some(json!({
            "name_type": "birth",
            "given_names": given_names,
            "surname": "Branch_A",
            "is_primary": true
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    person_id
}

async fn create_family(
    app: &axum::Router,
    tree_id: &str,
    spouses: &[(&str, &str)],
    children: &[&str],
) -> String {
    let (_, body) = send(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/families"),
        None,
    )
    .await;
    let family_id = body["id"].as_str().unwrap().to_string();
    for (index, (person_id, role)) in spouses.iter().enumerate() {
        let (status, _) = send(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
            Some(json!({ "person_id": person_id, "role": role, "sort_order": index })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    for person_id in children {
        let (status, _) = send(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
            Some(json!({ "person_id": person_id, "child_type": "biological", "sort_order": 0 })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    family_id
}

async fn rest_kinship(
    app: &axum::Router,
    tree_id: &str,
    from: &str,
    to: &str,
) -> (StatusCode, Value) {
    send(
        app,
        Method::GET,
        &format!("/api/v1/trees/{tree_id}/persons/{from}/kinship/{to}"),
        None,
    )
    .await
}

async fn graphql_kinship(app: &axum::Router, tree_id: &str, from: &str, to: &str) -> Value {
    let (status, body) = send(
        app,
        Method::POST,
        "/graphql",
        Some(json!({
            "query": r#"query($tree: ID!, $from: ID!, $to: ID!) {
                kinship(treeId: $tree, personId: $from, otherPersonId: $to) {
                    fromPersonId toPersonId truncated
                    paths { segments { ancestorIds familyId fromLine toLine half unionFamilyId } }
                    persons { personId givenNames }
                }
            }"#,
            "variables": { "tree": tree_id, "from": from, "to": to },
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body
}

/// A small tree: grandparents with two children, one grandchild from each,
/// and the spouse of one child, who has a sibling of their own.
struct Cousins {
    tree_id: String,
    grandfather: String,
    grandmother: String,
    grandparents_family: String,
    parent_a: String,
    parent_b: String,
    cousin_a: String,
    cousin_b: String,
    in_law: String,
    in_law_sibling: String,
    marriage: String,
}

async fn cousins(app: &axum::Router) -> Cousins {
    let tree_id = create_tree(app).await;
    let grandfather = create_person(app, &tree_id, "Grandfather_1").await;
    let grandmother = create_person(app, &tree_id, "Grandmother_1").await;
    let parent_a = create_person(app, &tree_id, "Parent_A").await;
    let parent_b = create_person(app, &tree_id, "Parent_B").await;
    let cousin_a = create_person(app, &tree_id, "Cousin_A").await;
    let cousin_b = create_person(app, &tree_id, "Cousin_B").await;
    let in_law = create_person(app, &tree_id, "Spouse_1").await;
    let in_law_sibling = create_person(app, &tree_id, "Sibling_1").await;
    let in_law_parent = create_person(app, &tree_id, "Parent_C").await;

    let grandparents_family = create_family(
        app,
        &tree_id,
        &[(&grandfather, "husband"), (&grandmother, "wife")],
        &[&parent_a, &parent_b],
    )
    .await;
    let marriage = create_family(
        app,
        &tree_id,
        &[(&parent_a, "husband"), (&in_law, "wife")],
        &[&cousin_a],
    )
    .await;
    create_family(app, &tree_id, &[(&parent_b, "wife")], &[&cousin_b]).await;
    create_family(
        app,
        &tree_id,
        &[(&in_law_parent, "husband")],
        &[&in_law, &in_law_sibling],
    )
    .await;

    Cousins {
        tree_id,
        grandfather,
        grandmother,
        grandparents_family,
        parent_a,
        parent_b,
        cousin_a,
        cousin_b,
        in_law,
        in_law_sibling,
        marriage,
    }
}

#[tokio::test]
async fn both_surfaces_trace_first_cousins_to_their_grandparents() {
    let app = setup_app().await;
    let tree = cousins(&app).await;

    let (status, rest) = rest_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_b).await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    assert_eq!(rest["truncated"], false);
    let paths = rest["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 1);
    let segment = &paths[0]["segments"][0];
    assert_eq!(
        segment["ancestor_ids"],
        json!([tree.grandfather, tree.grandmother])
    );
    assert_eq!(segment["family_id"], json!(tree.grandparents_family));
    assert_eq!(segment["from_line"], json!([tree.parent_a, tree.cousin_a]));
    assert_eq!(segment["to_line"], json!([tree.parent_b, tree.cousin_b]));
    assert_eq!(segment["half"], false);

    // Every person the path names comes with a search row, the two ends
    // first.
    let persons: Vec<&str> = rest["persons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|person| person["person_id"].as_str().unwrap())
        .collect();
    assert_eq!(persons.len(), 6);
    assert_eq!(
        &persons[..2],
        [tree.cousin_a.as_str(), tree.cousin_b.as_str()]
    );
    assert_eq!(rest["persons"][0]["given_names"], "Cousin_A");

    let graphql = graphql_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_b).await;
    let kinship = &graphql["data"]["kinship"];
    assert_eq!(kinship["truncated"], false);
    let segment = &kinship["paths"][0]["segments"][0];
    assert_eq!(
        segment["ancestorIds"],
        json!([tree.grandfather, tree.grandmother])
    );
    assert_eq!(segment["familyId"], json!(tree.grandparents_family));
    assert_eq!(segment["fromLine"], json!([tree.parent_a, tree.cousin_a]));
    assert_eq!(segment["toLine"], json!([tree.parent_b, tree.cousin_b]));
    assert_eq!(kinship["persons"].as_array().unwrap().len(), 6);
}

#[tokio::test]
async fn both_surfaces_fall_back_to_a_path_through_a_union() {
    let app = setup_app().await;
    let tree = cousins(&app).await;

    let (status, rest) =
        rest_kinship(&app, &tree.tree_id, &tree.cousin_b, &tree.in_law_sibling).await;
    assert_eq!(status, StatusCode::OK, "{rest}");
    let segments = rest["paths"][0]["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 2);
    // Up to the grandparents and down to Parent_A...
    assert_eq!(segments[0]["to_line"], json!([tree.parent_a]));
    // ...married to Spouse_1, sibling of the target.
    assert_eq!(segments[1]["union_family_id"], json!(tree.marriage));
    assert_eq!(segments[1]["from_line"], json!([tree.in_law]));
    assert_eq!(segments[1]["to_line"], json!([tree.in_law_sibling]));

    let graphql = graphql_kinship(&app, &tree.tree_id, &tree.cousin_b, &tree.in_law_sibling).await;
    let segments = graphql["data"]["kinship"]["paths"][0]["segments"]
        .as_array()
        .unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1]["unionFamilyId"], json!(tree.marriage));
    assert_eq!(segments[1]["toLine"], json!([tree.in_law_sibling]));
}

#[tokio::test]
async fn a_deleted_person_links_nobody() {
    let app = setup_app().await;
    let tree = cousins(&app).await;

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/persons/{}", tree.tree_id, tree.parent_b),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, rest) = rest_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_b).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rest["paths"], json!([]));
    let graphql = graphql_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_b).await;
    assert_eq!(graphql["data"]["kinship"]["paths"], json!([]));
}

#[tokio::test]
async fn both_surfaces_refuse_the_same_person_and_other_trees() {
    let app = setup_app().await;
    let tree = cousins(&app).await;

    let (status, _) = rest_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_a).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let graphql = graphql_kinship(&app, &tree.tree_id, &tree.cousin_a, &tree.cousin_a).await;
    assert_eq!(
        graphql["errors"][0]["extensions"]["code"],
        "VALIDATION_ERROR"
    );

    let other_tree = create_tree(&app).await;
    let stranger = create_person(&app, &other_tree, "Stranger_1").await;
    let (status, _) = rest_kinship(&app, &tree.tree_id, &tree.cousin_a, &stranger).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let graphql = graphql_kinship(&app, &tree.tree_id, &tree.cousin_a, &stranger).await;
    assert_eq!(graphql["errors"][0]["extensions"]["code"], "NOT_FOUND");
}
