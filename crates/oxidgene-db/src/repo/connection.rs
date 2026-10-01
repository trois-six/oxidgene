//! Database connection and migration utilities.

use oxidgene_core::error::OxidGeneError;
use sea_orm::sqlx::sqlite::{SqliteAutoVacuum, SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    DatabaseTransaction, DbErr, Statement, TransactionTrait,
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
///
/// `auto_vacuum=INCREMENTAL` lets [`erase_deleted_content`] give the pages a
/// purge frees back to the file system without rewriting the file. SQLite
/// records it in the file only when it is set before the first table, so it
/// takes effect on the databases this call creates; on an older file it
/// changes nothing until a `VACUUM` (see [`run_migrations`]'s reclaiming
/// pass).
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opts = ConnectOptions::new(database_url);
    opts.sqlx_logging(false);
    // SeaORM records the parameterized statement; bound values remain separate.
    opts.record_stmt_in_spans(true);
    opts.map_sqlx_sqlite_opts(|sqlite| {
        sqlite
            .auto_vacuum(SqliteAutoVacuum::Incremental)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
    });
    // SQLx times every pool acquisition; at `trace` it reports the time as a
    // `sqlx::pool::acquire` event, which OpenTelemetry export turns into the
    // connection wait-time metric. SeaORM's spans include that wait without
    // separating it, and a transaction waits before its `begin` span opens.
    // No console filter short of `trace` shows these events.
    opts.map_sqlx_sqlite_pool_opts(|pool| pool.acquire_time_level(log::LevelFilter::Trace));
    #[cfg(feature = "postgres")]
    opts.map_sqlx_postgres_pool_opts(|pool| pool.acquire_time_level(log::LevelFilter::Trace));
    let db = Database::connect(opts).await?;
    info!("Connected to database");
    Ok(db)
}

/// Run all pending migrations on the given database connection.
///
/// Spanned as `startup.migrate`: it runs before any request, and without a
/// span of its own each of its statements would be a trace by itself.
#[tracing::instrument(name = "startup.migrate", skip_all)]
pub async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(db, None).await?;
    info!("Migrations applied successfully");
    reclaim_free_pages(db).await;
    optimize_statistics(db).await;
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
/// that calls it. Callers run it after their last bulk write: statistics
/// gathered before it describe the tables without its rows.
///
/// PostgreSQL keeps its own statistics: autovacuum re-analyzes a table once a
/// tenth of its rows have changed, which every import exceeds, within its
/// one-minute nap. Best effort: a failure leaves the previous statistics in
/// place.
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

/// Let SQLite re-analyze, at startup, the tables whose statistics have
/// drifted.
///
/// Imports refresh the statistics themselves ([`refresh_statistics`]), but
/// ordinary writes accumulate too — every edit adds an audit entry, and most
/// a record version — and a table whose recorded row count is far from its real
/// one gets planned as if it were still that size. `PRAGMA optimize` with
/// `0x10000` checks every table, not only the ones this connection queried,
/// and analyzes only those whose size changed markedly since their last
/// `ANALYZE`, so a start with current statistics costs nothing. Best effort.
async fn optimize_statistics(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }
    if db
        .execute_unprepared("PRAGMA optimize = 0x10002")
        .await
        .is_err()
    {
        warn!(
            error = "sqlite_optimize",
            "could not refresh drifted query planner statistics"
        );
    }
}

/// A transaction whose deletions SQLite overwrites with zeros.
///
/// SQLite only unlinks a deleted row: its bytes stay in the page it freed
/// until something overwrites them, readable by anyone who opens the file.
/// After a tree's purge that is a whole genealogy. `PRAGMA secure_delete`
/// zeroes them at delete time, so the cost is that of what is deleted, not
/// of the file. It is a property of the connection, not of the transaction:
/// the transaction is what keeps every statement on the connection it was
/// set on, and [`Self::finish`] puts the previous value back before that
/// connection returns to the pool, so other writes keep deleting at full
/// speed. It reaches neither the write-ahead log's older frames nor the
/// words FTS5 keeps after a delete: see [`erase_deleted_content`] and
/// `PersonSearchRepo::merge_index`. PostgreSQL has no equivalent; there
/// this is a plain transaction.
#[derive(Debug)]
pub struct ErasingTransaction {
    txn: DatabaseTransaction,
    /// `secure_delete` as it was before, on SQLite.
    previous: Option<i32>,
}

impl ErasingTransaction {
    /// Open the transaction on `db`'s writer.
    pub async fn begin(db: &DatabaseConnection) -> Result<Self, OxidGeneError> {
        let txn = db.begin().await.map_err(crate::repo::db_err)?;
        let previous = match txn.get_database_backend() {
            DatabaseBackend::Sqlite => Some(set_secure_delete(&txn, 1).await?),
            _ => None,
        };
        Ok(Self { txn, previous })
    }

    /// The transaction to delete through.
    pub fn connection(&self) -> &DatabaseTransaction {
        &self.txn
    }

    /// Restore `secure_delete`, then commit when `outcome` — what was done
    /// through [`Self::connection`] — succeeded, or roll back.
    pub async fn finish<T>(self, outcome: Result<T, OxidGeneError>) -> Result<T, OxidGeneError> {
        if let Some(previous) = self.previous {
            // Not transactional: restored before a rollback as well.
            set_secure_delete(&self.txn, previous).await?;
        }
        let value = outcome?;
        self.txn.commit().await.map_err(crate::repo::db_err)?;
        Ok(value)
    }
}

/// Set `secure_delete` on `txn`'s connection to `value`; the value it had.
async fn set_secure_delete(txn: &DatabaseTransaction, value: i32) -> Result<i32, OxidGeneError> {
    let previous = txn
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA secure_delete",
        ))
        .await
        .map_err(crate::repo::db_err)?
        .and_then(|row| row.try_get::<i32>("", "secure_delete").ok())
        .unwrap_or(0);
    txn.execute_unprepared(&format!("PRAGMA secure_delete = {value}"))
        .await
        .map_err(crate::repo::db_err)?;
    Ok(previous)
}

/// Erase from the SQLite file what rows deleted in an [`ErasingTransaction`]
/// leave behind, and give the pages they freed back to the file system.
///
/// The transaction zeroed the deleted bytes in their pages, but the
/// write-ahead log can still hold earlier versions of those pages, in frames a
/// checkpoint copied without erasing: a `TRUNCATE` checkpoint empties the
/// log file. Before it, `PRAGMA incremental_vacuum` moves the free pages to
/// the end of the file and cuts them off, which the checkpoint then
/// applies — a cost proportional to the pages freed, where a `VACUUM`
/// rewrites the whole file. That takes `auto_vacuum=INCREMENTAL`, which
/// [`connect`] sets on the databases it creates; in an older file the
/// vacuum does nothing and the zeroed pages stay, as free pages.
///
/// Run after `PersonSearchRepo::merge_index`, so that the pages the merge
/// frees go too. Best effort: on failure the free pages stay until the next
/// purge or the next start's reclaiming pass. PostgreSQL reuses the space of
/// deleted rows through autovacuum and has no equivalent short of
/// `VACUUM FULL`.
pub async fn erase_deleted_content(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }
    for step in [
        "PRAGMA incremental_vacuum",
        "PRAGMA wal_checkpoint(TRUNCATE)",
    ] {
        if db.execute_unprepared(step).await.is_err() {
            warn!(
                error = "sqlite_erase",
                "could not release the purged pages of the database file"
            );
            return;
        }
    }
}

/// Number of free 4 KiB pages past which a SQLite file is worth rewriting.
/// 5,000 pages is about 20 MB, above the churn of ordinary use.
const VACUUM_THRESHOLD_PAGES: i64 = 5_000;

/// Reclaim unused SQLite space at startup, outside transactions.
///
/// A file in incremental auto-vacuum mode gives its free pages back with
/// `PRAGMA incremental_vacuum`, at the cost of those pages alone. An older
/// file is rewritten by `VACUUM`, and only past a free-page threshold so
/// that ordinary starts do not copy it; as [`connect`] set
/// `auto_vacuum=INCREMENTAL` on the connection, that rewrite also switches
/// the file to incremental mode.
async fn reclaim_free_pages(db: &DatabaseConnection) {
    if db.get_database_backend() != DatabaseBackend::Sqlite {
        return;
    }
    if is_incremental(db).await {
        if db
            .execute_unprepared("PRAGMA incremental_vacuum")
            .await
            .is_err()
        {
            warn_vacuum_failed();
        }
        return;
    }
    let Some(free_pages) = free_page_count(db).await else {
        return;
    };
    if free_pages < VACUUM_THRESHOLD_PAGES {
        return;
    }
    info!(free_pages, "reclaiming free database pages (VACUUM)");
    vacuum(db).await;
}

/// Whether the SQLite file is in incremental auto-vacuum mode (2).
async fn is_incremental(db: &DatabaseConnection) -> bool {
    matches!(
        db.query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA auto_vacuum",
        ))
        .await,
        Ok(Some(row)) if row.try_get::<i32>("", "auto_vacuum").ok() == Some(2)
    )
}

/// The number of free pages in the SQLite file, or `None` when it cannot be
/// read.
async fn free_page_count(db: &DatabaseConnection) -> Option<i64> {
    match db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "PRAGMA freelist_count",
        ))
        .await
    {
        Ok(Some(row)) => Some(row.try_get::<i32>("", "freelist_count").unwrap_or(0) as i64),
        _ => None,
    }
}

/// Rewrite the SQLite file without its free pages, logging the outcome.
async fn vacuum(db: &DatabaseConnection) {
    match db
        .execute_raw(Statement::from_string(DatabaseBackend::Sqlite, "VACUUM"))
        .await
    {
        Ok(_) => info!("database file compacted"),
        Err(_) => warn_vacuum_failed(),
    }
}

/// Not fatal: the database is correct, just larger than it needs to be.
///
/// Its own function because each `tracing` macro expands to several
/// branches, and two of them in one function exceed the complexity limit.
fn warn_vacuum_failed() {
    warn!(
        error = "sqlite_vacuum",
        "could not reclaim free pages; database file stays at its current size"
    );
}

/// Roll back all migrations on the given database connection.
pub async fn rollback_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::down(db, None).await?;
    info!("Migrations rolled back successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::field::{Field, Visit};
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};

    use super::*;

    /// The `acquired_after_secs` of every pool timing event.
    #[derive(Clone, Default)]
    struct AcquireTimes(Arc<Mutex<Vec<f64>>>);

    impl Visit for AcquireTimes {
        fn record_f64(&mut self, field: &Field, value: f64) {
            if field.name() == "acquired_after_secs" {
                self.0.lock().expect("capture lock").push(value);
            }
        }

        fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
    }

    impl<S: tracing::Subscriber> Layer<S> for AcquireTimes {
        fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
            if event.metadata().target() == "sqlx::pool::acquire" {
                event.record(&mut self.clone());
            }
        }
    }

    #[tokio::test]
    async fn every_pool_acquisition_reports_its_wait() {
        let times = AcquireTimes::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(times.clone()));

        let db = connect("sqlite::memory:")
            .await
            .expect("in-memory database");
        db.execute_raw(Statement::from_string(DatabaseBackend::Sqlite, "SELECT 1"))
            .await
            .expect("query runs");

        assert!(
            !times.0.lock().expect("capture lock").is_empty(),
            "no acquisition timing event"
        );
    }
}
