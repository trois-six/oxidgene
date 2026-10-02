//! How an entry form finds the records its text names without reading the
//! tree's whole lists, on REST and GraphQL alike: a source by its title, a
//! place by its name or by id, and the tree's places as suggestions.

mod common;

use axum::http::{Method, StatusCode};
use common::{gql_ok, new_tree, ok, send, setup_app};
use serde_json::json;

async fn create(app: &axum::Router, tree: &str, what: &str, body: serde_json::Value) -> String {
    ok(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/{what}"),
        Some(body),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_source_is_found_by_its_title_trimmed_and_ignoring_case() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Lookups").await;
    let wanted = create(
        &app,
        &tree,
        "sources",
        json!({ "title": "Parish Register A" }),
    )
    .await;
    create(
        &app,
        &tree,
        "sources",
        json!({ "title": "Parish Register B" }),
    )
    .await;

    let page = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/sources?title=%20parish%20register%20a%20"),
        None,
    )
    .await;
    let found = page["edges"].as_array().unwrap();
    assert_eq!(found.len(), 1, "{page}");
    assert_eq!(found[0]["node"]["id"], wanted);

    let data = gql_ok(
        &app,
        r#"query($t: ID!) { sources(treeId: $t, title: "PARISH REGISTER A") { totalCount edges { node { id } } } }"#,
        json!({ "t": tree }),
    )
    .await;
    assert_eq!(data["sources"]["totalCount"], 1);
    assert_eq!(data["sources"]["edges"][0]["node"]["id"], wanted);

    let none = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/sources?title=Parish"),
        None,
    )
    .await;
    assert_eq!(none["total_count"], 0, "only whole titles match");
}

#[tokio::test]
async fn a_place_is_found_by_its_name_or_by_id() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Lookups").await;
    let northfield = create(
        &app,
        &tree,
        "places",
        json!({ "name": "Northfield, Shire" }),
    )
    .await;
    let southfield = create(
        &app,
        &tree,
        "places",
        json!({ "name": "Southfield, Shire" }),
    )
    .await;
    create(&app, &tree, "places", json!({ "name": "Eastfield, Shire" })).await;

    let named = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/places?name=northfield,%20shire"),
        None,
    )
    .await;
    assert_eq!(named["total_count"], 1, "{named}");
    assert_eq!(named["edges"][0]["node"]["id"], northfield);

    let by_id = ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/places?ids={northfield},{southfield}"),
        None,
    )
    .await;
    assert_eq!(by_id["total_count"], 2, "{by_id}");

    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/places?ids=not-an-id"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let data = gql_ok(
        &app,
        r#"query($t: ID!, $ids: [ID!]) {
            named: places(treeId: $t, name: "SOUTHFIELD, SHIRE") { edges { node { id } } }
            byId: places(treeId: $t, ids: $ids) { totalCount }
        }"#,
        json!({ "t": tree, "ids": [northfield] }),
    )
    .await;
    assert_eq!(data["named"]["edges"][0]["node"]["id"], southfield);
    assert_eq!(data["byId"]["totalCount"], 1);
}

#[tokio::test]
async fn the_tree_places_are_suggested_ignoring_accents_from_a_word_start() {
    let app = setup_app().await;
    let tree = new_tree(&app, "Lookups").await;
    for name in [
        "Łąka Górna, Shire",
        "Le Bourg-Neuf, Shire",
        "Northfield, Shire",
    ] {
        create(&app, &tree, "places", json!({ "name": name })).await;
    }
    let suggested = |q: &'static str| {
        let app = app.clone();
        let tree = tree.clone();
        async move {
            ok(
                &app,
                Method::GET,
                &format!("/api/v1/trees/{tree}/suggestions/places?q={q}&lang=en"),
                None,
            )
            .await
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["value"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
        }
    };
    assert_eq!(suggested("laka%20gor").await, ["Łąka Górna, Shire"]);
    assert_eq!(suggested("neuf").await, ["Le Bourg-Neuf, Shire"]);
    assert!(suggested("ourg").await.is_empty());

    let data = gql_ok(
        &app,
        r#"query($t: ID!) { valueSuggestions(treeId: $t, field: PLACES, query: "north", language: "en") { value } }"#,
        json!({ "t": tree }),
    )
    .await;
    assert_eq!(data["valueSuggestions"][0]["value"], "Northfield, Shire");
}
