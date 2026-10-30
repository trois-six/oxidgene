//! What the repository tests share: a fresh, migrated database.

use oxidgene_db::repo::{connect, run_migrations};
use sea_orm::{ConnectionTrait, DatabaseConnection};

/// A migrated, empty database: in-memory SQLite, or a schema of its own in
/// the disposable PostgreSQL database `OXIDGENE_TEST_POSTGRES_URL` names
/// (`just test-postgres`, docs/development.md §2.7).
///
/// The repositories run on both: the web deployments use PostgreSQL, and
/// SQL that only SQLite accepts fails there alone.
pub async fn setup_db() -> DatabaseConnection {
    match std::env::var("OXIDGENE_TEST_POSTGRES_URL") {
        Ok(url) => setup_postgres_db(&url).await,
        Err(_) => setup_sqlite_db().await,
    }
}

/// A migrated, empty in-memory SQLite database, whatever the run: for the
/// tests of what SQLite alone has, its FTS5 index or its catalogue.
pub async fn setup_sqlite_db() -> DatabaseConnection {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    db
}

/// A migrated, empty schema of its own in the PostgreSQL database at `url`:
/// nextest runs each test in a process of its own, all at once, against the
/// one database.
async fn setup_postgres_db(url: &str) -> DatabaseConnection {
    let schema = format!("test_{}", uuid::Uuid::now_v7().simple());
    connect(url)
        .await
        .expect("connect to OXIDGENE_TEST_POSTGRES_URL")
        .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
        .await
        .expect("create the test's schema");
    let separator = if url.contains('?') { '&' } else { '?' };
    let db = connect(&format!(
        "{url}{separator}options=-c%20search_path%3D{schema}"
    ))
    .await
    .expect("connect to the test's schema");
    run_migrations(&db).await.expect("migrations");
    db
}
