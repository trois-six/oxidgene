//! Integration test: run migrations against in-memory SQLite.

use oxidgene_db::repo::{connect, rollback_migrations, run_migrations};

#[tokio::test]
async fn test_migrate_up_and_down_sqlite() {
    let db = connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");

    // Apply all migrations.
    run_migrations(&db).await.expect("Migration up failed");

    // Roll back all migrations.
    rollback_migrations(&db)
        .await
        .expect("Migration down failed");

    // Re-apply to ensure idempotency.
    run_migrations(&db).await.expect("Re-migration up failed");
}

/// The sanitize pass only matters for rows that already exist, which the
/// migration above never has — this drives the loop it actually skips.
#[tokio::test]
async fn sanitize_note_html_migration_cleans_existing_rows() {
    use oxidgene_db::migration::m20260802_000001_sanitize_note_html::Migration;
    use sea_orm::{ConnectionTrait, Statement, TryGetable, Value};
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    use uuid::Uuid;

    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrate");

    let tree_id = Uuid::now_v7();
    let note_id = Uuid::now_v7();
    let now = chrono::Utc::now();

    db.execute(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO tree (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        [
            Value::from(tree_id),
            Value::from("t".to_string()),
            Value::from(now),
            Value::from(now),
        ],
    ))
    .await
    .expect("insert tree");

    // Written the way a pre-sanitizer import would have left it.
    db.execute(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO note (id, tree_id, text, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        [
            Value::from(note_id),
            Value::from(tree_id),
            Value::from(r#"<p onclick="x()">Ne en 1802</p><script>alert(1)</script>"#.to_string()),
            Value::from(now),
            Value::from(now),
        ],
    ))
    .await
    .expect("insert note");

    Migration
        .up(&SchemaManager::new(&db))
        .await
        .expect("sanitize migration");

    let row = db
        .query_one(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT text FROM note WHERE id = ?",
            [Value::from(note_id)],
        ))
        .await
        .expect("select")
        .expect("row present");
    let text = String::try_get(&row, "", "text").expect("text column");

    assert!(!text.contains("onclick"), "got: {text}");
    assert!(!text.contains("alert"), "got: {text}");
    assert!(text.contains("Ne en 1802"), "got: {text}");
}

/// Same idea for the break pass: the rows it has to fix are the ones a GeneWeb
/// import left behind before line breaks were canonicalised.
#[tokio::test]
async fn normalize_note_line_breaks_migration_folds_existing_rows() {
    use oxidgene_db::migration::m20260803_000002_normalize_note_line_breaks::Migration;
    use sea_orm::{ConnectionTrait, Statement, TryGetable, Value};
    use sea_orm_migration::{MigrationTrait, SchemaManager};
    use uuid::Uuid;

    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrate");

    let tree_id = Uuid::now_v7();
    let now = chrono::Utc::now();

    db.execute(Statement::from_sql_and_values(
        db.get_database_backend(),
        "INSERT INTO tree (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)",
        [
            Value::from(tree_id),
            Value::from("t".to_string()),
            Value::from(now),
            Value::from(now),
        ],
    ))
    .await
    .expect("insert tree");

    // The same note as a `.gw` import and as a GEDCOM one would have stored it.
    let geneweb_id = Uuid::now_v7();
    let gedcom_id = Uuid::now_v7();
    for (id, text) in [
        (geneweb_id, "Ligne un<br/>\nLigne deux"),
        (gedcom_id, "Ligne un\nLigne deux"),
    ] {
        db.execute(Statement::from_sql_and_values(
            db.get_database_backend(),
            "INSERT INTO note (id, tree_id, text, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            [
                Value::from(id),
                Value::from(tree_id),
                Value::from(text.to_string()),
                Value::from(now),
                Value::from(now),
            ],
        ))
        .await
        .expect("insert note");
    }

    Migration
        .up(&SchemaManager::new(&db))
        .await
        .expect("normalize migration");

    for id in [geneweb_id, gedcom_id] {
        let row = db
            .query_one(Statement::from_sql_and_values(
                db.get_database_backend(),
                "SELECT text FROM note WHERE id = ?",
                [Value::from(id)],
            ))
            .await
            .expect("select")
            .expect("row present");
        let text = String::try_get(&row, "", "text").expect("text column");
        assert_eq!(text, "Ligne un\nLigne deux", "note {id}");
    }
}
