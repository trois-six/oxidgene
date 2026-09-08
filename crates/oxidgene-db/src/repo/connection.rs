//! Database connection and migration utilities.

use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, DbErr,
    Statement,
};
use sea_orm_migration::MigratorTrait;
use tracing::{info, warn};

use crate::Migrator;

/// Connect to a database using the provided URL.
///
/// # Supported URLs
/// - `sqlite::memory:` — in-memory SQLite (for tests)
/// - `sqlite://path/to/db.sqlite` — file-based SQLite
/// - `postgres://user:pass@host/db` — PostgreSQL
///
/// Note that sqlx rejects any unknown query parameter in the URL, so SQLite
/// pragmas cannot be passed that way — [`enable_wal`] sets them afterwards.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opts = ConnectOptions::new(database_url);
    opts.sqlx_logging(false);
    // SeaORM records the parameterized statement; bound values remain separate.
    opts.record_stmt_in_spans(true);
    let db = Database::connect(opts).await?;
    enable_wal(&db).await;
    info!("Connected to database");
    Ok(db)
}

/// Switch a SQLite database to write-ahead logging.
///
/// In the default `journal_mode=delete`, a write transaction takes an
/// EXCLUSIVE lock on the whole file, so a long delete blocks *readers* too and
/// the entire app goes unresponsive — not just the mutation. WAL lets readers
/// proceed against the last committed snapshot while a writer works.
///
/// `journal_mode` is stored in the database header, so this one statement
/// applies to every later connection in the pool. It is a no-op on
/// PostgreSQL and on in-memory SQLite (which does not support WAL).
async fn enable_wal(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }
    let stmt = Statement::from_string(DatabaseBackend::Sqlite, "PRAGMA journal_mode=WAL");
    match db.query_one_raw(stmt).await {
        // In-memory databases silently stay in `memory` journal mode.
        Ok(_) => info!("SQLite journal_mode set to WAL"),
        Err(_) => warn!(
            error = "sqlite_wal",
            "could not enable WAL; writes will block readers"
        ),
    }
}

/// Run all pending migrations on the given database connection.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(db, None).await?;
    info!("Migrations applied successfully");
    reclaim_free_pages(db).await;
    Ok(())
}

/// Number of free 4 KiB pages past which a SQLite file is worth rewriting.
/// 5,000 pages is about 20 MB, above the churn of ordinary use.
const VACUUM_THRESHOLD_PAGES: i64 = 5_000;

/// Reclaim significant unused SQLite space at startup, outside transactions.
/// The free-page threshold avoids rewriting the file on ordinary starts.
async fn reclaim_free_pages(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }

    let free_pages = match db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA freelist_count",
        ))
        .await
    {
        Ok(Some(row)) => row.try_get::<i32>("", "freelist_count").unwrap_or(0) as i64,
        _ => return,
    };

    if free_pages < VACUUM_THRESHOLD_PAGES {
        return;
    }

    info!(free_pages, "reclaiming free database pages (VACUUM)");
    match db
        .execute_raw(Statement::from_string(DatabaseBackend::Sqlite, "VACUUM"))
        .await
    {
        Ok(_) => info!("database file compacted"),
        // Not fatal: the database is correct, just larger than it needs to be.
        Err(_) => warn!(
            error = "sqlite_vacuum",
            "VACUUM failed; database file stays at its current size"
        ),
    }
}

/// Roll back all migrations on the given database connection.
pub async fn rollback_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::down(db, None).await?;
    info!("Migrations rolled back successfully");
    Ok(())
}
