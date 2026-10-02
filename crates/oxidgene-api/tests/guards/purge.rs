//! Purge completeness: deleting a tree leaves nothing of it behind, in any
//! table or in the media store.
//!
//! Drift it prevents: a new table (or a new kind of stored object) that the
//! purge does not know about, whose rows or files outlive the tree they
//! belonged to — personal data the user believes deleted.
//!
//! The tree holds one record of every kind (`common::populated`), with a
//! document's pages and their vignettes, history, an import job and an
//! export job with its artifact. Before the purge, every row of every table
//! that references the tree, or a row that does, transitively, is collected
//! from the schema itself (`sqlite_master`, `pragma_table_info`): a table
//! added later is covered without touching this file. After the tree is
//! deleted and purged, no table may hold a row referencing any of them, and
//! the media store no file under the tree's or its jobs' keys.
//!
//! Fixing a failure: teach `service::purge` to delete the table's rows (or
//! the store's objects) with the tree.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use uuid::Uuid;

use crate::common::populated::populated_tree;
use crate::common::{app_on, new_tree, send, setup_db, test_media_root};

async fn rows(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Vec<oxidgene_db::sea_orm::QueryResult> {
    db.query_all_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        values,
    ))
    .await
    .unwrap_or_else(|error| panic!("{sql}: {error}"))
}

/// Every table and its columns.
async fn schema(db: &DatabaseConnection) -> Vec<(String, Vec<String>)> {
    let mut tables = Vec::new();
    let names = rows(
        db,
        "SELECT name FROM sqlite_master WHERE type = 'table' \
         AND name NOT LIKE 'sqlite_%' AND name <> 'seaql_migrations'",
        vec![],
    )
    .await;
    for row in names {
        let table: String = row.try_get("", "name").unwrap();
        let columns = rows(
            db,
            "SELECT name FROM pragma_table_info(?)",
            vec![table.clone().into()],
        )
        .await
        .iter()
        .map(|c| c.try_get::<String>("", "name").unwrap())
        .collect();
        tables.push((table, columns));
    }
    tables
}

/// `ids` as SQL values, both as stored UUIDs and as their text.
fn values(ids: &BTreeSet<Uuid>) -> Vec<Value> {
    ids.iter()
        .flat_map(|id| [Value::from(*id), Value::from(id.to_string())])
        .collect()
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(", ")
}

/// The ids of every row referencing one of `ids`, recursively, `ids` included.
async fn closure(
    db: &DatabaseConnection,
    tables: &[(String, Vec<String>)],
    ids: BTreeSet<Uuid>,
) -> BTreeSet<Uuid> {
    let mut known = ids;
    loop {
        let before = known.len();
        let bound = values(&known);
        for (table, columns) in tables {
            if !columns.iter().any(|c| c == "id") {
                continue;
            }
            for column in columns.iter().filter(|c| *c != "id") {
                let sql = format!(
                    "SELECT id FROM \"{table}\" WHERE \"{column}\" IN ({})",
                    placeholders(bound.len())
                );
                for row in rows(db, &sql, bound.clone()).await {
                    let id = row.try_get::<Uuid>("", "id").ok().or_else(|| {
                        row.try_get::<String>("", "id")
                            .ok()
                            .and_then(|s| s.parse().ok())
                    });
                    if let Some(id) = id {
                        known.insert(id);
                    }
                }
            }
        }
        if known.len() == before {
            return known;
        }
    }
}

/// `table.column` pairs still holding one of `ids`, with how many rows.
async fn remaining(
    db: &DatabaseConnection,
    tables: &[(String, Vec<String>)],
    ids: &BTreeSet<Uuid>,
) -> Vec<String> {
    let bound = values(ids);
    let mut found = Vec::new();
    for (table, columns) in tables {
        for column in columns {
            let sql = format!(
                "SELECT COUNT(*) AS n FROM \"{table}\" WHERE \"{column}\" IN ({})",
                placeholders(bound.len())
            );
            let n: i64 = rows(db, &sql, bound.clone()).await[0]
                .try_get("", "n")
                .unwrap();
            if n > 0 {
                found.push(format!("{table}.{column}: {n} rows"));
            }
        }
    }
    found
}

/// Files under `root` whose path names one of `ids`.
fn files_naming(root: &Path, ids: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let text = path.to_string_lossy();
                if ids.iter().any(|id| text.contains(id.as_str())) {
                    found.push(text.into_owned());
                }
            }
        }
    }
    found
}

#[tokio::test]
async fn a_purged_tree_leaves_no_row_and_no_file() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Purged", 1).await;
    // A neighbour, which the purge must leave alone.
    let neighbour = new_tree(&app, "Kept").await;
    let tree_id: Uuid = tree.tree_id.parse().unwrap();
    let tables = schema(&db).await;

    let owned = closure(&db, &tables, BTreeSet::from([tree_id])).await;
    let tables_holding = remaining(&db, &tables, &owned).await;
    assert!(
        tables_holding.len() > 20,
        "the fixture should reach most tables: {tables_holding:?}"
    );
    let keys = [
        tree.tree_id.clone(),
        tree.id("job_id").to_string(),
        tree.id("export_job_id").to_string(),
    ];
    let media_root = test_media_root();
    assert!(
        !files_naming(&media_root, &keys).is_empty(),
        "the fixture stored no media or job file"
    );

    let (status, _) = send(
        &app,
        Method::DELETE,
        &format!("/api/v1/trees/{tree_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let mut purged = false;
    for _ in 0..200 {
        let left = rows(
            &db,
            "SELECT COUNT(*) AS n FROM tree WHERE id = ?",
            vec![tree_id.into()],
        )
        .await;
        if left[0].try_get::<i64>("", "n").unwrap() == 0 {
            purged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(purged, "the purge worker never removed the tree row");

    let left = remaining(&db, &tables, &owned).await;
    assert!(
        left.is_empty(),
        "rows the purge left behind:\n{}",
        left.join("\n")
    );
    let files = files_naming(&media_root, &keys);
    assert!(files.is_empty(), "files the purge left behind: {files:?}");
    let (status, _) = send(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{neighbour}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the purge reached another tree");
}
