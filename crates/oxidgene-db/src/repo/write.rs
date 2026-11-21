//! Write transactions: the one way a transaction that writes begins.
//!
//! A transaction that reads, then writes what it read, must not read a state
//! another writer is about to replace. Each backend needs its own start for
//! that, and a plain `BEGIN` gives it on neither:
//!
//! - **SQLite.** A deferred transaction takes the write lock at its first
//!   write. Had another connection — the desktop application beside an MCP
//!   session, two processes on one file — written since its first read, its
//!   snapshot is stale and SQLite refuses the write at once with
//!   `SQLITE_BUSY`, whatever the busy timeout: there is nothing to wait for.
//!   `BEGIN IMMEDIATE` takes the write lock first, waiting for it up to the
//!   busy timeout ([`SQLITE_BUSY_TIMEOUT`](super::SQLITE_BUSY_TIMEOUT)), so
//!   what the transaction reads is what it writes over.
//! - **PostgreSQL.** Under `READ COMMITTED` every statement sees what was
//!   committed when it started: a transaction that read a record, then
//!   waited for another writer of the tree at its first write, has read the
//!   state before that writer's commit — the state its history would store
//!   as replaced, and the one its checks ran on. Taking the tree's lock
//!   first makes it wait before it reads.
//!
//! [`begin_tree_write`] is what a write to a tree calls; [`begin_write`] is
//! for the few writes that belong to no tree, or to one being created.
//! A writer that cannot start in time is refused with a database error of
//! kind `busy` (`oxidgene_core::error::DatabaseErrorKind::Busy`), which the
//! API answers as retryable.

use oxidgene_core::error::OxidGeneError;
use sea_orm::{
    DatabaseBackend, DatabaseConnection, DatabaseTransaction, SqliteTransactionMode,
    TransactionOptions, TransactionTrait,
};
use uuid::Uuid;

use super::{RedactionRepo, db_err};

/// Begin a transaction that writes, to no tree in particular: under SQLite
/// it holds the write lock from its first statement (`BEGIN IMMEDIATE`).
///
/// A dropped transaction rolls back, so any `?` after it undoes the whole
/// write as a unit.
pub async fn begin_write(db: &DatabaseConnection) -> Result<DatabaseTransaction, OxidGeneError> {
    let sqlite_transaction_mode = (db.get_database_backend() == DatabaseBackend::Sqlite)
        .then_some(SqliteTransactionMode::Immediate);
    db.begin_with_options(TransactionOptions {
        sqlite_transaction_mode,
        ..TransactionOptions::default()
    })
    .await
    .map_err(db_err)
}

/// Begin a transaction that writes tree `tree_id`: [`begin_write`], then,
/// under PostgreSQL, the tree's lock (`RedactionRepo::lock_tree`), held
/// until the transaction ends. Two writers of one tree run one after the
/// other from their first statement, and each reads the state the other
/// committed; writers of different trees do not wait for each other.
///
/// The lock is the tree's `projection_state` row, the first of the three
/// every writer of a tree takes in one order (docs/data-model.md §4.1).
pub async fn begin_tree_write(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<DatabaseTransaction, OxidGeneError> {
    let txn = begin_write(db).await?;
    RedactionRepo::lock_tree(&txn, tree_id).await?;
    Ok(txn)
}
