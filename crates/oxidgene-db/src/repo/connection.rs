//! Database connection and migration utilities.

use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
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
/// SQLite connections open in write-ahead-log mode with `synchronous=NORMAL`.
///
/// In the default `journal_mode=delete`, a write transaction takes an
/// EXCLUSIVE lock on the whole file, so a long delete blocks *readers* too and
/// the entire app goes unresponsive — not just the mutation. WAL lets readers
/// proceed against the last committed snapshot while a writer works.
///
/// `synchronous=FULL`, the default, flushes the log to disk on every commit,
/// and every mutation here commits: an import or a projection refresh pays one
/// `fsync` per transaction. Under WAL, `NORMAL` syncs at checkpoints instead.
/// The database stays consistent through any crash; what a power loss can cost
/// is the last few commits, never the file. Both pragmas are set per
/// connection, so every connection in the pool gets them. In-memory databases
/// have no log and silently keep their `memory` journal.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opts = ConnectOptions::new(database_url);
    opts.sqlx_logging(false);
    // SeaORM records the parameterized statement; bound values remain separate.
    opts.record_stmt_in_spans(true);
    opts.map_sqlx_sqlite_opts(|sqlite| {
        sqlite
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
    });
    let db = Database::connect(opts).await?;
    info!("Connected to database");
    Ok(db)
}

/// Run all pending migrations on the given database connection.
pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(db, None).await?;
    info!("Migrations applied successfully");
    reclaim_free_pages(db).await;
    Ok(())
}

/// Refresh SQLite's query-planner statistics after a bulk load.
///
/// Without statistics SQLite cannot tell a selective index from an
/// unselective one, and it gets the most common query shape here wrong:
/// `tree_id = ? AND person_id IN (…)` is answered from the tree index —
/// reading every row of the tree — instead of one primary-key lookup per id.
/// On a 41 000-person tree that made each 500-person projection read take
/// 70 ms instead of 1, and a nine-generation pedigree most of a second.
///
/// A full `ANALYZE`: a sampled one caps the rows it counts per key, which is
/// exactly the estimate that has to be right, and on a 170 000-event database
/// the full pass takes about a tenth of a second — nothing beside the import
/// that calls it. PostgreSQL keeps its own statistics through autovacuum.
/// Best effort: a failure leaves the previous statistics in place.
pub async fn refresh_statistics(db: &impl ConnectionTrait) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }
    if db.execute_unprepared("ANALYZE").await.is_err() {
        warn!(
            error = "sqlite_analyze",
            "could not refresh query planner statistics"
        );
    }
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
