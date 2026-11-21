//! Database errors carry the kind the driver names, on SQLite and, under
//! `just test-postgres`, PostgreSQL: each test provokes a real failure and
//! reads its kind, never its text. All data is fictitious.

use chrono::Utc;
use oxidgene_core::enums::{Calendar, DateQualifier, EventType, Sex};
use oxidgene_core::error::{DatabaseErrorKind, OxidGeneError};
use oxidgene_db::entities::tree;
use oxidgene_db::repo::{
    EventDetails, EventRepo, FamilyRepo, PersonRepo, TreeChanges, TreeRepo, connect, db_err,
};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, Set, TransactionTrait,
};
use uuid::Uuid;

use crate::common::setup_db;

/// The kind of the database error `result` failed with.
fn kind<T: std::fmt::Debug>(result: Result<T, OxidGeneError>) -> DatabaseErrorKind {
    match result {
        Err(OxidGeneError::Database(error)) => error.kind(),
        other => panic!("expected a database error, got {other:?}"),
    }
}

async fn tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "Fictional tree".into(), None)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn a_refused_row_is_reported_with_the_kind_of_its_constraint() {
    let db = setup_db().await;
    let tree_id = tree(&db).await;

    // The same primary key again.
    assert_eq!(
        kind(TreeRepo::create(&db, tree_id, "Fictional tree".into(), None).await),
        DatabaseErrorKind::UniqueViolation
    );

    // A person of a tree that does not exist.
    assert_eq!(
        kind(PersonRepo::create(&db, Uuid::now_v7(), Uuid::now_v7(), Sex::Unknown).await),
        DatabaseErrorKind::ForeignKeyViolation
    );

    // An event owned by both a person and a family.
    let (person, family) = (Uuid::now_v7(), Uuid::now_v7());
    PersonRepo::create(&db, person, tree_id, Sex::Unknown)
        .await
        .unwrap();
    FamilyRepo::create(&db, family, tree_id).await.unwrap();
    assert_eq!(
        kind(
            EventRepo::create(
                &db,
                Uuid::now_v7(),
                tree_id,
                EventType::Other,
                None,
                None,
                None,
                Some(person),
                Some(family),
                None,
                DateQualifier::Exact,
                None,
                Calendar::Gregorian,
                EventDetails::default(),
            )
            .await
        ),
        DatabaseErrorKind::CheckViolation
    );

    // A tree without its required name.
    let nameless = tree::ActiveModel {
        id: Set(Uuid::now_v7()),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        ..Default::default()
    };
    assert_eq!(
        kind(nameless.insert(&db).await.map_err(db_err)),
        DatabaseErrorKind::NotNullViolation
    );

    // A failed statement leaves the kind of an ordinary failure.
    assert_eq!(
        kind(
            db.execute_unprepared("SELECT no_such_column FROM tree")
                .await
                .map_err(db_err)
        ),
        DatabaseErrorKind::Execution
    );
}

/// A write that cannot take the lock another writer holds, once its wait
/// has run out, is `Busy` on both backends.
#[tokio::test]
async fn a_write_refused_by_another_writers_lock_is_busy() {
    match setup_db().await {
        db if db.get_database_backend() == DatabaseBackend::Sqlite => sqlite_busy().await,
        db => postgres_busy(&db).await,
    }
}

/// Two connections to one SQLite file: the second cannot write while the
/// first holds the write lock.
async fn sqlite_busy() {
    let directory = std::env::temp_dir().join(format!("oxidgene-busy-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).expect("directory");
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("fixture.db").display()
    );
    let first = connect(&url).await.expect("first connection");
    oxidgene_db::repo::run_migrations(&first)
        .await
        .expect("migrations");
    let tree_id = tree(&first).await;
    let second = connect(&url).await.expect("second connection");
    // Do not wait for the lock: the test is about the error, not the wait.
    second
        .execute_unprepared("PRAGMA busy_timeout = 0")
        .await
        .expect("busy timeout");

    let holder = first.begin().await.expect("begin");
    holder
        .execute_unprepared("UPDATE tree SET name = 'Held'")
        .await
        .expect("the first writer takes the lock");
    let refused = TreeRepo::update(
        &second,
        tree_id,
        TreeChanges {
            name: Some("Refused".into()),
            ..TreeChanges::default()
        },
    )
    .await;
    assert_eq!(kind(refused), DatabaseErrorKind::Busy);
    holder.rollback().await.expect("rollback");

    drop((first, second));
    let _ = std::fs::remove_dir_all(&directory);
}

/// Two transactions on one PostgreSQL row: the second gives up at its
/// `lock_timeout`.
async fn postgres_busy(db: &DatabaseConnection) {
    let tree_id = tree(db).await;
    let holder = db.begin().await.expect("begin");
    holder
        .execute_unprepared("UPDATE tree SET name = 'Held'")
        .await
        .expect("the first writer locks the row");

    let waiter = db.begin().await.expect("begin");
    waiter
        .execute_unprepared("SET LOCAL lock_timeout = '100ms'")
        .await
        .expect("lock timeout");
    let refused = TreeRepo::update(
        &waiter,
        tree_id,
        TreeChanges {
            name: Some("Refused".into()),
            ..TreeChanges::default()
        },
    )
    .await;
    assert_eq!(kind(refused), DatabaseErrorKind::Busy);
    waiter.rollback().await.expect("rollback");
    holder.rollback().await.expect("rollback");
}
