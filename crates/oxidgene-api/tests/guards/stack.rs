//! Stack guard: every operation runs within half a tokio worker's stack.
//!
//! Drift it prevents: an operation whose stack outgrows the 2 MiB a tokio
//! worker thread has, which aborts the whole server process. A debug build
//! gives every awaited future a slot of its own in the frame that polls it,
//! so the stack an operation needs is the sum of its futures along the call:
//! async-graphql's dispatch over every root field once needed nearly 2 MiB
//! for one mutation. Every REST route and every GraphQL root field, and the
//! background jobs they queue, run here on a thread with a deliberately
//! small stack, [`STACK`] — 1 MiB in a debug build, half what a worker has —
//! so a regression fails long before it reaches a worker's limit.
//!
//! Coverage is generic, as for the cross-tree guard: the routes come from the
//! OpenAPI document, the fields from introspection, all against the
//! populated fixture's own tree, reads first and deletions last so that the
//! writes find the records they name and take their full path.
//!
//! A failure aborts the test binary with `thread '<operation>' has
//! overflowed its stack`. Fixing it: box the large future on the path where
//! it is created (`Box::pin`), as the root resolvers do through
//! `graphql::scope::boxed` and the projection rebuild does with its fetches;
//! Clippy's `large_futures` finds the single large ones, a debugger's
//! backtrace with each frame's stack pointer the sum. Do not raise
//! [`STACK`]: it stands for the worker's stack, not for the code's needs.

use std::future::Future;

use axum::Router;
use axum::http::Method;
use oxidgene_db::sea_orm::DatabaseConnection;
use tokio::runtime::{Handle, Runtime};

use super::surface::Spec;
use crate::common::populated::{Populated, populated_tree};
use crate::common::{
    app_on, family_blocks_gedcom, import_gedcom, new_tree, ok, send, setup_db, worker_on,
};

/// The stack each operation runs on: half a tokio worker's 2 MiB in a debug
/// build, a quarter in an optimized one, whose frames are about three times
/// smaller.
const STACK: usize = if cfg!(debug_assertions) {
    1024 * 1024
} else {
    512 * 1024
};

/// A runtime for the fixture: its workers drive the database pool's I/O and
/// timers while an operation runs on its own thread.
fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime")
}

/// Run `operation` on a thread of [`STACK`] bytes named `name`, then the
/// background jobs it queued, there too; its output.
fn on_small_stack<F>(
    handle: &Handle,
    db: &DatabaseConnection,
    name: String,
    operation: F,
) -> F::Output
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handle = handle.clone();
    let db = db.clone();
    std::thread::Builder::new()
        .name(name)
        .stack_size(STACK)
        .spawn(move || {
            handle.block_on(async move {
                let output = operation.await;
                while worker_on(&db).run_once().await.expect("a job iteration") {}
                output
            })
        })
        .expect("a thread")
        .join()
        .expect("the operation does not panic")
}

/// The order operations run in: reads, then writes, then removals, and the
/// tree's own deletion last.
fn rank(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name == "delete /api/v1/trees/{tree_id}" || name == "mutation deletetree" {
        4
    } else if ["delete", "remove", "drop", "release", "merge"]
        .iter()
        .any(|verb| name.contains(verb))
    {
        3
    } else if name.starts_with("get ") || name.starts_with("query ") {
        0
    } else {
        1
    }
}

/// The fixture: a router, its database and the populated tree.
fn fixture(rt: &Runtime) -> (Router, DatabaseConnection, Populated) {
    rt.block_on(async {
        let db = setup_db().await;
        let app = app_on(db.clone());
        let tree = populated_tree(&app, &db, "Tree B", 1).await;
        (app, db, tree)
    })
}

#[test]
fn every_rest_route_runs_on_a_small_stack() {
    let rt = runtime();
    let (app, db, b) = fixture(&rt);
    let spec = Spec(rt.block_on(ok(&app, Method::GET, "/api/v1/openapi.json", None)));

    let mut requests = Vec::new();
    for (path, operations) in spec.0["paths"].as_object().unwrap() {
        for (method, operation) in operations.as_object().unwrap() {
            let Ok((uri, _)) = spec.fill(path, operation, &b.tree_id, &b) else {
                continue;
            };
            let body = spec.request_body(operation, &b, path);
            requests.push((format!("{} {path}", method.to_uppercase()), uri, body));
        }
    }
    requests.sort_by_key(|(name, _, _)| rank(name));

    let mut succeeded = 0;
    for (name, uri, body) in &requests {
        let method = Method::from_bytes(name.split(' ').next().unwrap().as_bytes()).unwrap();
        let (app, uri, body) = (app.clone(), uri.clone(), body.clone());
        let (status, _) = on_small_stack(rt.handle(), &db, name.clone(), async move {
            send(&app, method, &uri, body).await
        });
        succeeded += usize::from(status.is_success());
    }
    // The import job of a whole GEDCOM file, which the generated request
    // body cannot make.
    let tree = rt.block_on(new_tree(&app, "Tree I"));
    let (app_i, db_i) = (app.clone(), db.clone());
    on_small_stack(rt.handle(), &db, "import job".into(), async move {
        import_gedcom(&app_i, &db_i, &tree, &family_blocks_gedcom(3)).await
    });

    // Most requests must succeed, or the fixture no longer drives the
    // operations down their full path.
    assert!(
        succeeded * 4 >= requests.len() * 3,
        "only {succeeded} of {} requests succeeded",
        requests.len()
    );
}

#[cfg(feature = "graphql")]
#[test]
fn every_graphql_field_runs_on_a_small_stack() {
    use super::introspection::{INTROSPECTION, Schema, record_of};
    use crate::common::gql;
    use serde_json::json;

    let rt = runtime();
    let (app, db, b) = fixture(&rt);
    let schema =
        Schema(rt.block_on(gql(&app, INTROSPECTION, json!({})))["data"]["__schema"].clone());

    let mut operations = Vec::new();
    for (root, keyword) in [("queryType", "query"), ("mutationType", "mutation")] {
        for field in schema.root_fields(root) {
            let name = field["name"].as_str().unwrap();
            let mut args = Vec::new();
            for arg in field["args"].as_array().into_iter().flatten() {
                let arg_name = arg["name"].as_str().unwrap();
                let names_tree =
                    arg_name == "treeId" || (arg_name == "id" && record_of("id", name).is_none());
                let value = if names_tree {
                    Some(format!("\"{}\"", b.tree_id))
                } else {
                    schema.literal(&arg["type"], arg_name, name, &b, 0)
                };
                if let Some(value) = value {
                    args.push(format!("{arg_name}: {value}"));
                }
            }
            let args = if args.is_empty() {
                String::new()
            } else {
                format!("({})", args.join(", "))
            };
            let document = format!(
                "{keyword} {{ {name}{args}{} }}",
                schema.selection(&field["type"], 0)
            );
            operations.push((format!("{keyword} {name}"), document));
        }
    }
    operations.sort_by_key(|(name, _)| rank(name));

    let mut succeeded = 0;
    for (name, document) in &operations {
        let (app, document) = (app.clone(), document.clone());
        let response = on_small_stack(rt.handle(), &db, name.clone(), async move {
            gql(&app, &document, json!({})).await
        });
        succeeded += usize::from(response["errors"].is_null());
    }
    // As for REST: enough answers that the resolvers ran their full path.
    assert!(
        succeeded * 2 >= operations.len(),
        "only {succeeded} of {} operations succeeded",
        operations.len()
    );
}
