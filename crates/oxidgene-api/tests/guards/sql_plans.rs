//! SQL plans: no statement reads a large table in full without an index.
//!
//! Drift it prevents: a new query, or a changed `WHERE`, that SQLite answers
//! by reading every row of `person` or `event` of every tree — invisible on
//! a test tree, seconds on a real one, and multiplied by every request.
//!
//! The test captures every statement SeaORM runs (its `db.statement` span
//! field) while it builds the populated fixture and calls every GET route
//! over it, then asks SQLite for each distinct statement's plan (`EXPLAIN
//! QUERY PLAN` on the bare schema, parameters bound to `NULL`). A
//! bare `SCAN` of one of `LARGE_TABLES` fails unless `WHOLE_TREE_READS`
//! declares the statement an intentional whole-table read. It runs apart from
//! the functional suite (`just sql-plans`, the CI SQL plans job): it reads
//! the plans of a few thousand statements.
//!
//! Fixing a failure: add the index the statement needs (in the initial
//! migration while the product is unreleased), or filter by an indexed
//! column; declare a read that must see every row in `WHOLE_TREE_READS`
//! with why.

use std::collections::BTreeSet;

use axum::http::Method;
use oxidgene_db::sea_orm::{ConnectionTrait, DbBackend, Statement, Value};

use super::surface::Spec;
use crate::common::capture::global_capture;
use crate::common::populated::populated_tree;
use crate::common::{app_on, ok, send, setup_db};

/// The tables that grow with a tree.
const LARGE_TABLES: &[&str] = &[
    "event",
    "person",
    "person_name",
    "family_child",
    "family_spouse",
    "citation",
    "record_version",
    "audit_entry",
    "media",
    "media_link",
    "person_denorm",
];

/// Statements allowed to scan a large table, by a fragment of their SQL,
/// with why.
const WHOLE_TREE_READS: &[(&str, &str)] = &[];

/// The `?` placeholders of `sql`, outside string literals.
fn placeholders(sql: &str) -> usize {
    let mut quoted = false;
    sql.chars()
        .filter(|c| {
            if *c == '\'' {
                quoted = !quoted;
            }
            *c == '?' && !quoted
        })
        .count()
}

#[tokio::test]
#[ignore = "reads the plan of every statement: run by `just sql-plans`"]
async fn no_statement_scans_a_large_table() {
    let capture = global_capture();
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Plans", 4).await;
    let spec = Spec(ok(&app, Method::GET, "/api/v1/openapi.json", None).await);
    let paths = spec.0["paths"].as_object().unwrap().clone();
    for (path, operations) in &paths {
        if let Some(operation) = operations.get("get")
            && let Ok((uri, _)) = spec.fill(path, operation, &tree.tree_id, &tree)
        {
            send(&app, Method::GET, &uri, None).await;
        }
    }

    let statements: BTreeSet<String> = capture
        .take()
        .into_iter()
        .filter(|c| c.field == "db.statement")
        .map(|c| c.value)
        .filter(|sql| {
            let head = sql.trim_start().to_ascii_uppercase();
            ["SELECT", "UPDATE", "DELETE", "WITH"]
                .iter()
                .any(|k| head.starts_with(k))
        })
        .collect();
    assert!(
        statements.len() > 100,
        "only {} statements captured",
        statements.len()
    );

    // Plans are read on an empty, freshly migrated database: without
    // statistics SQLite takes any usable index, so a `SCAN` there means the
    // statement has none. The fixture's statistics would describe a toy tree
    // and make the plans depend on its shape instead.
    let schema_only = setup_db().await;
    let mut scans = Vec::new();
    for sql in &statements {
        if WHOLE_TREE_READS
            .iter()
            .any(|(fragment, _)| sql.contains(fragment))
        {
            continue;
        }
        let nulls = vec![Value::String(None); placeholders(sql)];
        let plan = schema_only
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("EXPLAIN QUERY PLAN {sql}"),
                nulls,
            ))
            .await
            .unwrap_or_else(|error| panic!("EXPLAIN {sql}: {error}"));
        for row in plan {
            let detail: String = row.try_get("", "detail").unwrap_or_default();
            let Some(table) = detail.strip_prefix("SCAN ") else {
                continue;
            };
            let table = table.split_whitespace().next().unwrap_or_default();
            if LARGE_TABLES.contains(&table) && !detail.contains(" USING ") {
                scans.push(format!("{detail}\n    {sql}"));
            }
        }
    }
    assert!(
        scans.is_empty(),
        "statements scanning a large table:\n{}",
        scans.join("\n")
    );
}
