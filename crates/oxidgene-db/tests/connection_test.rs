//! The PostgreSQL pool's session limits and the migrations of processes
//! starting together. These need PostgreSQL: on SQLite they end at once.
//! All data is fictitious.

use std::time::Duration;

use oxidgene_core::error::{DatabaseErrorKind, OxidGeneError};
use oxidgene_db::repo::{
    DatabaseSettings, TreeRepo, begin_tree_write, connect_with, run_migrations,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use uuid::Uuid;

use crate::common::fresh_postgres_schema;

/// What `SHOW parameter` answers on a connection of `db`.
async fn shown(db: &DatabaseConnection, parameter: &str) -> String {
    db.query_one_raw(Statement::from_string(
        DbBackend::Postgres,
        format!("SHOW {parameter}"),
    ))
    .await
    .expect("SHOW")
    .expect("a row")
    .try_get_by_index::<String>(0)
    .expect("a value")
}

/// The settings a process connects with are the session's, on every
/// connection of the pool, beside the options the URL already carried.
#[tokio::test]
async fn a_postgres_session_takes_the_limits_it_was_connected_with() {
    let Some(url) = fresh_postgres_schema().await else {
        return;
    };
    let settings = DatabaseSettings {
        max_connections: 2,
        acquire_timeout: Duration::from_secs(5),
        statement_timeout: Some(Duration::from_secs(7)),
        lock_timeout: Some(Duration::from_millis(1500)),
        idle_in_transaction_timeout: None,
    };
    let db = connect_with(&url, &settings).await.expect("connect");
    // Two statements at once: both connections of the pool.
    let (first, second) = tokio::join!(
        shown(&db, "statement_timeout"),
        shown(&db, "statement_timeout")
    );
    assert_eq!((first.as_str(), second.as_str()), ("7s", "7s"));
    assert_eq!(shown(&db, "lock_timeout").await, "1500ms");
    assert_eq!(shown(&db, "idle_in_transaction_session_timeout").await, "0");
    assert!(
        shown(&db, "search_path").await.starts_with("test_"),
        "the URL's own options are kept"
    );
}

/// A writer that waits for a tree past its lock timeout is refused as
/// busy, which the API answers as retryable.
#[tokio::test]
async fn a_writer_waiting_past_the_lock_timeout_is_refused_as_busy() {
    let Some(url) = fresh_postgres_schema().await else {
        return;
    };
    let patient = connect_with(&url, &DatabaseSettings::worker())
        .await
        .expect("connect");
    run_migrations(&patient).await.expect("migrations");
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&patient, tree_id, "Fictional tree".into(), None)
        .await
        .expect("tree");
    let hasty = connect_with(
        &url,
        &DatabaseSettings {
            lock_timeout: Some(Duration::from_millis(200)),
            ..DatabaseSettings::server()
        },
    )
    .await
    .expect("connect");

    let holder = begin_tree_write(&patient, tree_id)
        .await
        .expect("the tree's lock");
    let refused = begin_tree_write(&hasty, tree_id).await;
    assert!(
        matches!(&refused, Err(OxidGeneError::Database(error)) if error.kind() == DatabaseErrorKind::Busy),
        "{:?}",
        refused.map(drop)
    );
    holder.rollback().await.expect("rollback");
    begin_tree_write(&hasty, tree_id)
        .await
        .expect("the tree is free again")
        .rollback()
        .await
        .expect("rollback");
}

/// Replicas starting together each migrate: the first does, the others
/// wait for it and find nothing left to apply.
#[tokio::test]
async fn processes_starting_together_migrate_one_after_the_other() {
    let Some(url) = fresh_postgres_schema().await else {
        return;
    };
    let starts: Vec<_> = (0..4)
        .map(|_| {
            let url = url.clone();
            tokio::spawn(async move {
                let db = connect_with(&url, &DatabaseSettings::server())
                    .await
                    .expect("connect");
                run_migrations(&db).await
            })
        })
        .collect();
    for start in starts {
        start.await.expect("joined").expect("migrated");
    }

    let db = connect_with(&url, &DatabaseSettings::server())
        .await
        .expect("connect");
    let applied = db
        .query_one_raw(Statement::from_string(
            DbBackend::Postgres,
            "SELECT COUNT(*) FROM seaql_migrations",
        ))
        .await
        .expect("count")
        .expect("a row")
        .try_get_by_index::<i64>(0)
        .expect("a count");
    assert_eq!(applied, 1, "the one migration, applied once");
}

/// SQLite takes the settings too, and keeps its single writer.
#[tokio::test]
async fn a_sqlite_database_connects_with_any_settings() {
    let db = connect_with(
        "sqlite::memory:",
        &DatabaseSettings {
            max_connections: 8,
            ..DatabaseSettings::worker()
        },
    )
    .await
    .expect("connect");
    run_migrations(&db).await.expect("migrations");
    // An in-memory database shared by two connections would be two
    // databases: the tree written is the tree read.
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&db, tree_id, "Fictional tree".into(), None)
        .await
        .expect("tree");
    let (first, second) = tokio::join!(TreeRepo::get(&db, tree_id), TreeRepo::get(&db, tree_id));
    assert!(first.is_ok() && second.is_ok());
}
