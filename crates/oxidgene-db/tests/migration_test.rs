//! Integration test: run migrations against in-memory SQLite, and against
//! PostgreSQL when one is pointed at.

use oxidgene_db::repo::{connect, rollback_migrations, run_migrations};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

#[tokio::test]
async fn test_migrate_up_and_down_sqlite() {
    let db = connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");

    // Apply all migrations.
    run_migrations(&db).await.expect("Migration up failed");
    assert_media_storage_schema(&db).await;

    // Roll back all migrations.
    rollback_migrations(&db)
        .await
        .expect("Migration down failed");

    // Re-apply to ensure idempotency.
    run_migrations(&db).await.expect("Re-migration up failed");
    assert_media_storage_schema(&db).await;
}

/// The same migration run, against a real PostgreSQL.
///
/// Skipped unless `OXIDGENE_TEST_DATABASE_URL` names a database the test may
/// create and drop tables in — there is no container harness in this repo, and
/// silently passing on a machine with no server would be worse than saying so.
/// Run it with, for example:
///
/// ```text
/// OXIDGENE_TEST_DATABASE_URL=postgres://oxidgene:oxidgene@localhost/oxidgene_test \
///   cargo test -p oxidgene-db --features postgres
/// ```
#[tokio::test]
#[cfg(feature = "postgres")]
async fn test_migrate_up_and_down_postgres() {
    let Ok(url) = std::env::var("OXIDGENE_TEST_DATABASE_URL") else {
        eprintln!("skipping: OXIDGENE_TEST_DATABASE_URL is not set");
        return;
    };

    let db = connect(&url).await.expect("connect to PostgreSQL");

    // Start from a known state: a previous failed run may have left tables.
    let _ = rollback_migrations(&db).await;

    run_migrations(&db).await.expect("Migration up failed");
    assert_media_storage_schema(&db).await;

    rollback_migrations(&db)
        .await
        .expect("Migration down failed");

    run_migrations(&db).await.expect("Re-migration up failed");
    assert_media_storage_schema(&db).await;

    rollback_migrations(&db).await.expect("cleanup failed");
}

/// Check that Sprint F.1's columns and table are actually queryable.
///
/// Written as SQL against the backend rather than as a schema introspection so
/// it reads the same on both: SQLite and PostgreSQL disagree on almost every
/// catalogue table, but they agree on what a `SELECT` of a missing column does.
async fn assert_media_storage_schema(db: &DatabaseConnection) {
    let backend = db.get_database_backend();

    db.query_all_raw(Statement::from_string(
        backend,
        "SELECT storage_key, sha256, thumbnail_key, width, height, page_count FROM media"
            .to_owned(),
    ))
    .await
    .expect("media should carry the storage columns");

    db.query_all_raw(Statement::from_string(
        backend,
        "SELECT id, media_id, page, x, y, width, height, person_id, event_id, \
         created_at, updated_at FROM vignette"
            .to_owned(),
    ))
    .await
    .expect("the vignette table should exist");
}
