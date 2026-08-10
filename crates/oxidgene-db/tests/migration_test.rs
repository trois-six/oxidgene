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
