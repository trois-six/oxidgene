//! Cross-tree guard: no operation reaches a record of another tree.
//!
//! Drift it prevents: a handler that loads a record by its id and forgets to
//! check the tree in the path — reading, editing or deleting another tree's
//! data through a tree the caller names. Tree B holds one record of every
//! kind (`common::populated`); every REST route and every GraphQL field that
//! names a record is called on an empty tree A with B's ids, and must answer
//! a client error (`404` / `NOT_FOUND`, or a validation error) — or, for a
//! read, an answer that mentions none of B's records.
//!
//! Coverage is generic: the routes come from the OpenAPI document the build
//! generates from the router, request bodies from its schemas (required
//! fields, plus every field naming a record), the GraphQL fields from
//! introspection. A new route or field is checked without touching this
//! file, unless one of its parameters names a record kind `surface` does not
//! know: the test then fails until it is added there.
//!
//! Fixing a failure: make the service check the record's tree
//! (`service::scope::require_tree_resource`), for both surfaces.

use axum::http::{Method, StatusCode};

use super::surface::Spec;
use crate::common::populated::populated_tree;
use crate::common::{app_on, new_tree, ok, send, setup_db};

/// POST routes that only read: a batch of ids in the body, whose answer
/// leaves out what is not of the tree.
const BATCH_READS: &[&str] = &[
    "/api/v1/trees/{tree_id}/gallery-bundle",
    "/api/v1/trees/{tree_id}/image-data",
    "/api/v1/trees/{tree_id}/pedigrees",
    "/api/v1/trees/{tree_id}/portrait-images",
    "/api/v1/trees/{tree_id}/relation-labels",
];

#[tokio::test]
async fn no_rest_route_reaches_another_trees_record() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let b = populated_tree(&app, &db, "Tree B", 1).await;
    let tree_a = new_tree(&app, "Tree A").await;
    let spec = Spec(ok(&app, Method::GET, "/api/v1/openapi.json", None).await);

    let mut checked = 0;
    let (mut controls, mut failed_controls) = (0, Vec::new());
    let mut leaks = Vec::new();
    let paths = spec.0["paths"].as_object().unwrap().clone();
    for (path, operations) in &paths {
        if !path.contains("{tree_id}") {
            continue;
        }
        for (method, operation) in operations.as_object().unwrap() {
            let (uri, names_a_record) = match spec.fill(path, operation, &tree_a, &b) {
                Ok(filled) => filled,
                Err(name) => {
                    leaks.push(format!(
                        "{method} {path}: parameter `{name}` names no known record kind (surface::record_key)"
                    ));
                    continue;
                }
            };
            let body = spec.request_body(operation, &b, path);
            let body_names_a_record = body.as_ref().is_some_and(|body| {
                let text = body.to_string();
                b.ids.values().any(|id| text.contains(id.as_str()))
            });
            if !(names_a_record || body_names_a_record) {
                continue;
            }
            checked += 1;
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let (status, answer) = send(&app, method.clone(), &uri, body).await;
            let refused = status.is_client_error() && status != StatusCode::METHOD_NOT_ALLOWED;
            // A read may answer, as long as it answers nothing of tree B.
            let read = method == Method::GET || BATCH_READS.contains(&path.as_str());
            let mentions_b = {
                let text = answer.to_string();
                b.ids.values().any(|id| text.contains(id.as_str()))
            };
            if !(refused || (read && status.is_success() && !mentions_b)) {
                leaks.push(format!("{method} {path}: {status} {answer}"));
            }
            // The control: the same read on the records' own tree answers,
            // so a refusal above is the tree check, not a malformed request.
            if method == Method::GET {
                let own = uri.replace(&tree_a, &b.tree_id);
                if send(&app, Method::GET, &own, None).await.0.is_success() {
                    controls += 1;
                } else {
                    failed_controls.push(path.clone());
                }
            }
        }
    }
    assert!(
        failed_controls.len() * 10 <= controls,
        "too many reads fail on their own tree, the requests are malformed: {failed_controls:?}"
    );
    // Every record route of the fixture's kinds, at the time of writing.
    assert!(checked > 80, "only {checked} routes checked");
    assert!(
        leaks.is_empty(),
        "routes that reached tree B through tree A:\n{}",
        leaks.join("\n")
    );
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn no_graphql_field_reaches_another_trees_record() {
    use super::introspection::{INTROSPECTION, Schema, holds_nothing};
    use crate::common::gql;
    use serde_json::json;

    let db = setup_db().await;
    let app = app_on(db.clone());
    let b = populated_tree(&app, &db, "Tree B", 1).await;
    let tree_a = new_tree(&app, "Tree A").await;
    let schema = Schema(gql(&app, INTROSPECTION, json!({})).await["data"]["__schema"].clone());

    let mut checked = 0;
    let mut leaks = Vec::new();
    for (root, keyword) in [("queryType", "query"), ("mutationType", "mutation")] {
        for field in schema.root_fields(root) {
            let name = field["name"].as_str().unwrap();
            let args = field["args"].as_array().unwrap();
            if !args.iter().any(|a| a["name"] == "treeId") {
                continue;
            }
            let rendered = match schema.arguments(&field, &tree_a, &b) {
                Ok(Some(rendered)) => rendered,
                Ok(None) => continue,
                Err(arg) => {
                    leaks.push(format!(
                        "{name}: argument `{arg}` names no known record kind (surface::record_of)"
                    ));
                    continue;
                }
            };
            checked += 1;
            let operation = format!(
                "{keyword} {{ {name}({rendered}){} }}",
                schema.selection(&field["type"], 0)
            );
            let response = gql(&app, &operation, json!({})).await;
            let errors = response["errors"].as_array().cloned().unwrap_or_default();
            // An error without a code is a document the generator got wrong,
            // not an answer: it would hide a leak.
            if errors.iter().any(|e| e["extensions"]["code"].is_null()) {
                leaks.push(format!(
                    "{name}: invalid generated document {operation}: {response}"
                ));
            } else if errors.is_empty() && !holds_nothing(&response["data"][name]) {
                leaks.push(format!("{name}: {response}"));
            }
        }
    }
    assert!(checked > 70, "only {checked} fields checked");
    assert!(
        leaks.is_empty(),
        "GraphQL fields that reached tree B through tree A:\n{}",
        leaks.join("\n")
    );
}
