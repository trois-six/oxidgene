//! The Sources tab's exact-title reads: `dictionary/sources?exact=true` over
//! REST and `dictionarySources(exact: true)` over GraphQL return the same
//! sources, and both refuse an exact lookup without a title. Each case has its
//! REST test and its GraphQL twin.
//!
//! All data is fictitious.

use crate::common;

use axum::http::{Method, StatusCode};
use common::{app_on, gql, gql_ok, new_tree, ok, send, setup_db};
use serde_json::{Value, json};

/// Fictitious source titles: one exact title in two spellings, a title that
/// only starts with it, and an unrelated title.
const TITLES: [&str; 5] = [
    "Civil register",
    "CIVIL REGISTER",
    "Civil register - Book 1",
    "Civil",
    "Parish register",
];

/// Creates [`TITLES`] in `tree` over REST.
async fn create_sources(app: &axum::Router, tree: &str) {
    for title in TITLES {
        ok(
            app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/sources"),
            Some(json!({ "title": title })),
        )
        .await;
    }
}

/// The sorted titles of a flat list of REST dictionary entries.
fn rest_titles(entries: &Value) -> Vec<String> {
    let mut titles: Vec<String> = entries
        .as_array()
        .expect("a flat list")
        .iter()
        .map(|entry| entry["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    titles
}

/// The sorted titles of a GraphQL dictionary-source list.
fn graphql_titles(entries: &Value) -> Vec<String> {
    let mut titles: Vec<String> = entries
        .as_array()
        .expect("a flat list")
        .iter()
        .map(|entry| entry["source"]["title"].as_str().unwrap().to_owned())
        .collect();
    titles.sort();
    titles
}

#[tokio::test]
async fn exact_title_sources_match_over_rest() {
    let app = app_on(setup_db().await);
    let tree = new_tree(&app, "Exact Titles REST").await;
    create_sources(&app, &tree).await;
    let base = format!("/api/v1/trees/{tree}/dictionary/sources");

    // The exact group: the title itself, ignoring case, and nothing longer.
    let exact = ok(
        &app,
        Method::GET,
        &format!("{base}?prefix=civil%20register&exact=true"),
        None,
    )
    .await;
    assert_eq!(rest_titles(&exact), ["CIVIL REGISTER", "Civil register"]);

    // Without `exact`, the prefix keeps every title starting with it.
    let prefixed = ok(
        &app,
        Method::GET,
        &format!("{base}?prefix=civil%20register"),
        None,
    )
    .await;
    assert_eq!(prefixed.as_array().unwrap().len(), 3);

    // `exact=false` is the prefix mode.
    let explicit = ok(
        &app,
        Method::GET,
        &format!("{base}?prefix=civil%20register&exact=false"),
        None,
    )
    .await;
    assert_eq!(explicit, prefixed);

    // An exact lookup needs a title: absent or blank is refused.
    for query in ["exact=true", "prefix=%20%20&exact=true"] {
        let (status, body) = send(&app, Method::GET, &format!("{base}?{query}"), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
    }
}

#[tokio::test]
async fn exact_title_sources_match_over_graphql() {
    let app = app_on(setup_db().await);
    let tree = new_tree(&app, "Exact Titles GraphQL").await;
    create_sources(&app, &tree).await;

    let data = gql_ok(
        &app,
        r#"query ($tree: ID!) {
            exact: dictionarySources(treeId: $tree, prefix: "civil register", exact: true) { source { title } }
            prefixed: dictionarySources(treeId: $tree, prefix: "civil register") { source { title } }
            explicit: dictionarySources(treeId: $tree, prefix: "civil register", exact: false) { source { title } }
        }"#,
        json!({ "tree": tree }),
    )
    .await;
    assert_eq!(
        graphql_titles(&data["exact"]),
        ["CIVIL REGISTER", "Civil register"]
    );
    assert_eq!(graphql_titles(&data["prefixed"]).len(), 3);
    assert_eq!(data["explicit"], data["prefixed"]);

    // The same refusals as over REST.
    for query in [
        r#"query ($tree: ID!) { dictionarySources(treeId: $tree, exact: true) { count } }"#,
        r#"query ($tree: ID!) { dictionarySources(treeId: $tree, prefix: "  ", exact: true) { count } }"#,
    ] {
        let response = gql(&app, query, json!({ "tree": tree })).await;
        assert!(
            response["errors"]
                .as_array()
                .is_some_and(|errors| !errors.is_empty()),
            "should be refused: {query}"
        );
    }
}
