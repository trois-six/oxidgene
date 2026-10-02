//! History without duplicates: the history stores past states only
//! (docs/data-model.md, history).
//!
//! Drift it prevents: a write that stores the state it just wrote — a copy
//! of the live row, which the record itself already is — an import that
//! versions every record it brought, and a deletion marker that repeats the
//! state the soft-deleted row still holds. Each multiplies the database by
//! data it holds anyway.
//!
//! Fixing a failure: the write must go through `history::Change`'s two
//! phases — `prepare` before the write stores the pre-image, `record` after
//! it writes only the audit entry.

use axum::http::Method;
use oxidgene_db::sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde_json::Value;
use uuid::Uuid;

use crate::common::populated::populated_tree;
use crate::common::{app_on, family_blocks_gedcom, import_gedcom, new_tree, ok, setup_db};

async fn count(db: &impl ConnectionTrait, sql: &str, tree: Uuid) -> i64 {
    db.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        sql,
        [tree.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "n")
    .unwrap()
}

#[tokio::test]
async fn an_import_stores_no_version() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = new_tree(&app, "Imported").await;
    import_gedcom(&app, &db, &tree, &family_blocks_gedcom(2)).await;
    let stored = count(
        &db,
        "SELECT COUNT(*) AS n FROM record_version WHERE tree_id = ?",
        tree.parse().unwrap(),
    )
    .await;
    assert_eq!(
        stored, 0,
        "an import stored versions of the records it brought"
    );
}

#[tokio::test]
async fn no_stored_version_repeats_the_live_state() {
    let db = setup_db().await;
    let app = app_on(db.clone());
    let tree = populated_tree(&app, &db, "Edited", 1).await;
    let t = |path: &str| format!("/api/v1/trees/{}{path}", tree.tree_id);
    // A deletion, so the history holds a marker.
    ok(
        &app,
        Method::DELETE,
        &t(&format!("/notes/{}", tree.id("note_id"))),
        None,
    )
    .await;
    let tree_id: Uuid = tree.tree_id.parse().unwrap();

    let markers_with_state = count(
        &db,
        "SELECT COUNT(*) AS n FROM record_version WHERE tree_id = ? AND deleted AND snapshot IS NOT NULL",
        tree_id,
    )
    .await;
    assert_eq!(markers_with_state, 0, "deletion markers carry a snapshot");

    let records = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT DISTINCT record_type, record_id FROM record_version WHERE tree_id = ?",
            [tree_id.into()],
        ))
        .await
        .unwrap();
    assert!(
        records.len() >= 3,
        "the fixture's edits stored too few versions"
    );
    let mut duplicates = Vec::new();
    for row in records {
        let record_type: String = row.try_get("", "record_type").unwrap();
        let record_id: Uuid = row.try_get("", "record_id").unwrap();
        let versions = ok(
            &app,
            Method::GET,
            &t(&format!("/history/{record_type}/{record_id}?first=100")),
            None,
        )
        .await;
        let nodes: Vec<&Value> = versions["edges"]
            .as_array()
            .unwrap()
            .iter()
            .map(|edge| &edge["node"])
            .collect();
        let Some(current) = nodes.iter().find(|v| v["current"] == true) else {
            continue;
        };
        if current["deleted"] == true {
            continue;
        }
        for stored in nodes
            .iter()
            .filter(|v| v["current"] != true && v["deleted"] != true)
        {
            if stored["snapshot"] == current["snapshot"] {
                duplicates.push(format!(
                    "{record_type} {record_id}: version {} equals the live state",
                    stored["version"]
                ));
            }
        }
    }
    assert!(
        duplicates.is_empty(),
        "stored versions that copy the live record:\n{}",
        duplicates.join("\n")
    );
}
