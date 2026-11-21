//! SQL plans: no statement reads a large table in full without an index,
//! and no page of one sorts what it read.
//!
//! Drift it prevents: a new query, or a changed `WHERE`, that SQLite answers
//! by reading every row of `person` or `event` of every tree — invisible on
//! a test tree, seconds on a real one, and multiplied by every request. And
//! a page of a list (`LIMIT`) whose order no index gives: SQLite reads every
//! row the filter keeps and sorts them in a temporary B-tree before keeping
//! a page, so the first page of a tree's persons costs the whole tree. The
//! keyset lists filter by tree and order by id, which the `(tree_id, id)`
//! indexes give in one range read.
//!
//! The test captures every statement SeaORM runs (its `db.statement` span
//! field) while it builds the populated fixture and calls every GET route
//! over it, then asks SQLite for each distinct statement's plan (`EXPLAIN
//! QUERY PLAN` on the bare schema, parameters bound to `NULL`). A
//! bare `SCAN` of one of `LARGE_TABLES` fails unless `WHOLE_TREE_READS`
//! declares the statement an intentional whole-table read; a plan that
//! sorts in a temporary B-tree for the `ORDER BY` of a statement with a
//! `LIMIT` reading one of them fails unless `SORTED_PAGES` declares it. It
//! runs apart from
//! the functional suite (`just sql-plans`, the CI SQL plans job): it reads
//! the plans of a few thousand statements.
//!
//! Fixing a failure: add the index the statement needs (in the initial
//! migration while the product is unreleased), or filter by an indexed
//! column; declare a read that must see every row in `WHOLE_TREE_READS`
//! with why. For a sorted page, add the index whose columns are the
//! filter's equalities then the order's (`(tree_id, id)`), or declare in
//! `SORTED_PAGES`, with why, a page whose order is computed — a search by
//! name ranked by how well each person matches.

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
    "record_tag",
    "person_denorm",
];

/// Statements allowed to scan a large table, by a fragment of their SQL,
/// with why.
const WHOLE_TREE_READS: &[(&str, &str)] = &[];

/// Pages of a large table allowed to sort what they read, by a fragment of
/// their SQL, with why.
const SORTED_PAGES: &[(&str, &str)] = &[(
    // `HistoryRepo`: the persons last edited.
    r#"GROUP BY "audit_entry"."subject_id" ORDER BY MAX("audit_entry"."id") DESC"#,
    "the persons last edited, ranked by their latest entry: an order computed \
     from the history of each, which no index holds; one row per edited person",
)];

/// The table a plan row reads, if it reads one: `SCAN person …`, `SEARCH
/// person USING …`.
fn table_read(detail: &str) -> Option<&str> {
    let rest = detail
        .strip_prefix("SCAN ")
        .or_else(|| detail.strip_prefix("SEARCH "))?;
    rest.split_whitespace().next()
}

/// Whether a plan row sorts for the statement's `ORDER BY`, wholly or in
/// part (`USE TEMP B-TREE FOR ORDER BY`, `… FOR RIGHT PART OF ORDER BY`).
fn sorts(detail: &str) -> bool {
    detail.starts_with("USE TEMP B-TREE FOR") && detail.ends_with("ORDER BY")
}

/// Whether `sql` keeps a page of what it reads.
fn is_page(sql: &str) -> bool {
    sql.to_ascii_uppercase().contains(" LIMIT ")
}

/// What the plans showed: full scans of a large table, and pages of one
/// sorted in a temporary B-tree, each with its statement.
#[derive(Default)]
struct Findings {
    scans: Vec<String>,
    sorted_pages: Vec<String>,
}

impl Findings {
    /// Review the plan `details` of statement `sql`, less what
    /// `WHOLE_TREE_READS` and `SORTED_PAGES` declare.
    fn review(&mut self, sql: &str, details: &[String]) {
        let declared =
            |reads: &[(&str, &str)]| reads.iter().any(|(fragment, _)| sql.contains(fragment));
        let large: Vec<&String> = details
            .iter()
            .filter(|detail| table_read(detail).is_some_and(|table| LARGE_TABLES.contains(&table)))
            .collect();
        if !declared(WHOLE_TREE_READS) {
            for detail in &large {
                if detail.starts_with("SCAN ") && !detail.contains(" USING ") {
                    self.scans.push(format!("{detail}\n    {sql}"));
                }
            }
        }
        if !large.is_empty()
            && is_page(sql)
            && details.iter().any(|detail| sorts(detail))
            && !declared(SORTED_PAGES)
        {
            self.sorted_pages
                .push(format!("{}\n    {sql}", details.join(" / ")));
        }
    }
}

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

#[test]
fn a_plan_row_names_its_table_and_its_sort() {
    assert_eq!(
        table_read("SEARCH person USING INDEX uq_person_tree_id (tree_id=? AND id>?)"),
        Some("person")
    );
    assert_eq!(table_read("SCAN event"), Some("event"));
    assert_eq!(table_read("USE TEMP B-TREE FOR ORDER BY"), None);
    assert!(sorts("USE TEMP B-TREE FOR ORDER BY"));
    assert!(sorts("USE TEMP B-TREE FOR RIGHT PART OF ORDER BY"));
    assert!(!sorts("USE TEMP B-TREE FOR DISTINCT"));
    assert!(is_page(
        "SELECT id FROM person WHERE tree_id = ? ORDER BY id LIMIT ?"
    ));
    assert!(!is_page(
        "SELECT id FROM person WHERE tree_id = ? ORDER BY id"
    ));
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
    let mut findings = Findings::default();
    for sql in &statements {
        let nulls = vec![Value::String(None); placeholders(sql)];
        let plan = schema_only
            .query_all_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                format!("EXPLAIN QUERY PLAN {sql}"),
                nulls,
            ))
            .await
            .unwrap_or_else(|error| panic!("EXPLAIN {sql}: {error}"));
        let details: Vec<String> = plan
            .iter()
            .map(|row| row.try_get("", "detail").unwrap_or_default())
            .collect();
        findings.review(sql, &details);
    }
    assert!(
        findings.scans.is_empty(),
        "statements scanning a large table:\n{}",
        findings.scans.join("\n")
    );
    assert!(
        findings.sorted_pages.is_empty(),
        "pages of a large table sorting what they read:\n{}",
        findings.sorted_pages.join("\n")
    );
}
