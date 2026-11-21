//! Write transactions across two connections to one SQLite file: the
//! desktop application and an MCP session of the same database
//! (docs/mcp.md §4.5). All data is fictitious.

use std::time::{Duration, Instant};

use oxidgene_core::error::{DatabaseErrorKind, OxidGeneError};
use oxidgene_db::repo::{TreeChanges, TreeRepo, begin_write, connect, db_err, run_migrations};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use uuid::Uuid;

/// Two connections to one migrated SQLite file holding a tree, as two
/// processes open it; the directory to remove once done.
async fn two_connections() -> (
    DatabaseConnection,
    DatabaseConnection,
    Uuid,
    std::path::PathBuf,
) {
    let directory = std::env::temp_dir().join(format!("oxidgene-write-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).expect("directory");
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("fixture.db").display()
    );
    let first = connect(&url).await.expect("first connection");
    run_migrations(&first).await.expect("migrations");
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&first, tree_id, "Fictional tree".into(), None)
        .await
        .expect("tree");
    let second = connect(&url).await.expect("second connection");
    (first, second, tree_id, directory)
}

async fn rename(db: &impl ConnectionTrait, tree_id: Uuid, name: &str) -> Result<(), OxidGeneError> {
    TreeRepo::update(
        db,
        tree_id,
        TreeChanges {
            name: Some(name.to_owned()),
            ..TreeChanges::default()
        },
    )
    .await
    .map(drop)
}

/// Why a write transaction must take the write lock when it begins: a
/// deferred one that has read cannot wait for the lock once another
/// connection has written, because its snapshot is stale. SQLite refuses it
/// at once, whatever the busy timeout.
#[tokio::test]
async fn a_deferred_transaction_that_read_cannot_write_after_another_writer() {
    let (first, second, tree_id, directory) = two_connections().await;

    let deferred = first.begin().await.expect("a deferred transaction");
    TreeRepo::get(&deferred, tree_id).await.expect("a read");
    rename(&second, tree_id, "Renamed elsewhere")
        .await
        .expect("the other connection writes");

    let started = Instant::now();
    let refused = rename(&deferred, tree_id, "Renamed here").await;
    assert!(
        matches!(&refused, Err(OxidGeneError::Database(error)) if error.kind() == DatabaseErrorKind::Busy),
        "{refused:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "refused at once, not after the busy timeout: {:?}",
        started.elapsed()
    );
    deferred.rollback().await.expect("rollback");

    drop((first, second));
    let _ = std::fs::remove_dir_all(&directory);
}

/// [`begin_write`] takes the write lock first: a second writer waits for it
/// instead of failing, and what the transaction reads is the committed
/// state it will write over.
#[tokio::test]
async fn a_write_transaction_makes_the_other_writer_wait() {
    let (first, second, tree_id, directory) = two_connections().await;

    let holder = begin_write(&first).await.expect("the write lock");
    TreeRepo::get(&holder, tree_id).await.expect("a read");

    let waiter = tokio::spawn({
        let second = second.clone();
        async move {
            let txn = begin_write(&second).await?;
            let seen = TreeRepo::get(&txn, tree_id).await?.name;
            rename(&txn, tree_id, "Second writer").await?;
            txn.commit().await.map_err(db_err)?;
            Ok::<_, OxidGeneError>(seen)
        }
    });
    // Long enough for the second writer to be waiting on the lock.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!waiter.is_finished(), "the second writer waits");

    rename(&holder, tree_id, "First writer")
        .await
        .expect("the holder writes");
    holder.commit().await.expect("commit");

    let seen = waiter
        .await
        .expect("joined")
        .expect("the second writer goes on");
    assert_eq!(seen, "First writer", "it read the committed state");
    assert_eq!(
        TreeRepo::get(&first, tree_id).await.expect("tree").name,
        "Second writer"
    );

    drop((first, second));
    let _ = std::fs::remove_dir_all(&directory);
}

/// The wait has an end: a writer that cannot take the lock within the busy
/// timeout is refused with `Busy`, which the error contract answers as
/// retryable.
#[tokio::test]
async fn a_writer_waiting_past_the_busy_timeout_is_refused_as_busy() {
    let (first, second, _tree_id, directory) = two_connections().await;
    second
        .execute_unprepared("PRAGMA busy_timeout = 100")
        .await
        .expect("a short busy timeout");

    let holder = begin_write(&first).await.expect("the write lock");
    let refused = begin_write(&second).await;
    assert!(
        matches!(&refused, Err(OxidGeneError::Database(error)) if error.kind() == DatabaseErrorKind::Busy),
        "{:?}",
        refused.map(drop)
    );
    holder.rollback().await.expect("rollback");

    drop((first, second));
    let _ = std::fs::remove_dir_all(&directory);
}
