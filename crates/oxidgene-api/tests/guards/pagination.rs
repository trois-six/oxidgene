//! Pagination contract: every collection pages by cursor and clamps `first`
//! to 1–100; every other list is a declared exception (docs/api.md,
//! Pagination).
//!
//! Drift it prevents: a list endpoint that returns a whole tree's rows in
//! one answer, or a connection that ignores the clamp and lets a client ask
//! for a million rows.
//!
//! On a tree of 110 persons with one record of every kind, every GET route
//! of the OpenAPI document that takes `first` is asked for 0, 101 and one
//! row, then for the next page after the first one's cursor; every other GET
//! route answering a bare JSON array must be in `WHOLE_LISTS`. GraphQL
//! connections get the same clamp checks.
//!
//! Fixing a failure: page the list (`PaginationParams`, a `Connection`) on
//! both surfaces, or — when docs/api.md declares it whole — add it to
//! `WHOLE_LISTS` with the category docs/api.md gives.

use axum::http::Method;
use serde_json::Value;

use super::surface::Spec;
use crate::common::populated::populated_tree;
use crate::common::{app_on, ok, send, setup_db};

/// GET routes answering a whole list, by path, with docs/api.md's reason.
const WHOLE_LISTS: &[(&str, &str)] = &[
    (
        "/api/v1/trees/{tree_id}/persons/{person_id}/names",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/persons/{person_id}/homonyms",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/persons/{person_id}/ancestors",
        "bounded by max_depth",
    ),
    (
        "/api/v1/trees/{tree_id}/persons/{person_id}/descendants",
        "bounded by max_depth",
    ),
    (
        "/api/v1/trees/{tree_id}/families/{family_id}/spouses",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/families/{family_id}/children",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/events/{event_id}/witnesses",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/sources/{source_id}/repositories",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/repositories/{repository_id}/sources",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/media/{media_id}/pages",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/media/{media_id}/vignettes",
        "a list under one record",
    ),
    (
        "/api/v1/trees/{tree_id}/vignettes",
        "a list under one record (its filter is required)",
    ),
    (
        "/api/v1/trees/{tree_id}/media-links",
        "whole: tree-wide links, or one entity's or medium's",
    ),
    (
        "/api/v1/trees/{tree_id}/portraits",
        "whole: one short row per person with a portrait",
    ),
    (
        "/api/v1/trees/{tree_id}/persons/recently-modified",
        "limited by its documented limit",
    ),
    (
        "/api/v1/trees/{tree_id}/suggestions/{field}",
        "limited by its documented limit",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/family-names",
        "whole: the dictionary's index",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/occupations",
        "whole: the dictionary's index",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/places",
        "whole: the dictionary's index",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/sources",
        "whole: one drill-down level",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/family-names/usage",
        "whole: a usage list",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/occupations/usage",
        "whole: a usage list",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/places/{place_id}/usage",
        "whole: a usage list",
    ),
    (
        "/api/v1/trees/{tree_id}/dictionary/sources/{source_id}/usage",
        "whole: a usage list",
    ),
    (
        "/api/v1/reference/{lang}/places",
        "limited by its documented limit",
    ),
    (
        "/api/v1/reference/basemap",
        "static reference data, the same for every tree",
    ),
    (
        "/api/v1/trees/{tree_id}/unlocated-places",
        "a tool's report, bounded by the tree's places",
    ),
    ("/api/v1/trees/{tree_id}/duplicates", "a tool's report"),
];

fn edges(page: &Value) -> usize {
    page["edges"].as_array().map_or(0, Vec::len)
}

#[tokio::test]
async fn every_collection_pages_and_every_whole_list_is_declared() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Pagination", 11).await;
    let spec = Spec(ok(&app, Method::GET, "/api/v1/openapi.json", None).await);

    let mut problems = Vec::new();
    let mut connections = 0;
    let paths = spec.0["paths"].as_object().unwrap().clone();
    for (path, operations) in &paths {
        let Some(operation) = operations.get("get") else {
            continue;
        };
        let Ok((uri, _)) = spec.fill(path, operation, &tree.tree_id, &tree) else {
            continue;
        };
        let takes_first = operation["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|p| p["in"] == "query" && p["name"] == "first");
        let join = if uri.contains('?') { '&' } else { '?' };
        if takes_first {
            connections += 1;
            if let Some(problem) = check_connection(&app, path, &format!("{uri}{join}")).await {
                problems.push(problem);
            }
            continue;
        }
        let (status, answer) = send(&app, Method::GET, &uri, None).await;
        if status.is_success()
            && answer.is_array()
            && !WHOLE_LISTS.iter().any(|(whole, _)| whole == path)
        {
            problems.push(format!(
                "GET {path}: a whole list neither paged nor declared in WHOLE_LISTS"
            ));
        }
    }
    for (whole, _) in WHOLE_LISTS {
        if !paths.contains_key(*whole) {
            problems.push(format!("WHOLE_LISTS names {whole}, which is no route"));
        }
    }
    assert!(
        connections >= 12,
        "only {connections} paged collections found"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The clamp and cursor checks of one paged collection; `prefix` ends with
/// `?` or `&`.
async fn check_connection(app: &axum::Router, path: &str, prefix: &str) -> Option<String> {
    let get = |query: String| {
        let uri = format!("{prefix}{query}");
        async move { send(app, Method::GET, &uri, None).await }
    };
    let (status, page) = get("first=101".into()).await;
    if !status.is_success() {
        return Some(format!("GET {path}?first=101: {status} {page}"));
    }
    let Some(total) = page["total_count"].as_u64() else {
        return Some(format!(
            "GET {path}: not a connection (no total_count): {page}"
        ));
    };
    if page["page_info"]["has_next_page"].is_null() {
        return Some(format!("GET {path}: no page_info.has_next_page"));
    }
    let total = usize::try_from(total).unwrap();
    if edges(&page) != total.min(100) {
        return Some(format!(
            "GET {path}?first=101: {} rows of {total}, not clamped to 100",
            edges(&page)
        ));
    }
    let (_, page) = get("first=0".into()).await;
    if edges(&page) != total.min(1) {
        return Some(format!(
            "GET {path}?first=0: {} rows, not clamped to 1",
            edges(&page)
        ));
    }
    if total > 1 {
        let (_, first) = get("first=1".into()).await;
        let cursor = first["page_info"]["end_cursor"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if first["page_info"]["has_next_page"] != true || cursor.is_empty() {
            return Some(format!("GET {path}?first=1: no next page announced"));
        }
        let (_, second) = get(format!("first=1&after={cursor}")).await;
        if edges(&second) != 1 || second["edges"][0]["node"] == first["edges"][0]["node"] {
            return Some(format!(
                "GET {path}: the cursor does not lead to the next row"
            ));
        }
    }
    None
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn every_graphql_connection_clamps_first() {
    use super::introspection::{INTROSPECTION, Schema, named};
    use crate::common::gql;
    use serde_json::json;

    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Pagination", 11).await;
    let schema = Schema(gql(&app, INTROSPECTION, json!({})).await["data"]["__schema"].clone());

    let mut problems = Vec::new();
    let mut checked = 0;
    for field in schema.root_fields("queryType") {
        let name = field["name"].as_str().unwrap();
        if !named(&field["type"]).ends_with("Connection") {
            continue;
        }
        let mut args = Vec::new();
        let mut complete = true;
        for arg in field["args"].as_array().unwrap() {
            let arg_name = arg["name"].as_str().unwrap();
            if arg_name == "treeId" {
                args.push(format!("treeId: \"{}\"", tree.tree_id));
            } else if arg["type"]["kind"] == "NON_NULL" && arg_name != "first" {
                match schema.literal(&arg["type"], arg_name, name, &tree, 0) {
                    Some(value) => args.push(format!("{arg_name}: {value}")),
                    None => complete = false,
                }
            }
        }
        if !complete {
            problems.push(format!("{name}: a required argument the guard cannot fill"));
            continue;
        }
        checked += 1;
        for (first, most) in [(101, 100), (0, 1)] {
            let mut all = args.clone();
            all.push(format!("first: {first}"));
            let document = format!(
                "{{ {name}({}) {{ totalCount edges {{ cursor }} pageInfo {{ hasNextPage }} }} }}",
                all.join(", ")
            );
            let response = gql(&app, &document, json!({})).await;
            let data = &response["data"][name];
            let total = data["totalCount"].as_u64().unwrap_or(0) as usize;
            let rows = data["edges"].as_array().map_or(0, Vec::len);
            if response["errors"].is_array() || rows != total.min(most) {
                problems.push(format!(
                    "{name}(first: {first}): {rows} rows of {total}: {response}"
                ));
            }
        }
    }
    assert!(checked >= 10, "only {checked} GraphQL connections found");
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
