//! The identity of a SQLite file's schema, its integrity check and its
//! copy (docs/architecture.md §8.4). All data is fictitious.

use oxidgene_db::repo::{
    MigrationError, SchemaState, TreeRepo, check_integrity, connect, run_migrations, schema_digest,
    schema_state, snapshot_into,
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use uuid::Uuid;

use crate::common::setup_sqlite_db;

/// A directory of its own, and the URL of a database file in it.
fn file_database() -> (std::path::PathBuf, String) {
    let directory = std::env::temp_dir().join(format!("oxidgene-safety-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&directory).expect("directory");
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.join("fixture.db").display()
    );
    (directory, url)
}

async fn recorded_digest(db: &DatabaseConnection) -> String {
    db.query_one_raw(sea_orm::Statement::from_string(
        sea_orm::DbBackend::Sqlite,
        "SELECT value FROM schema_meta WHERE key = 'schema_digest'",
    ))
    .await
    .expect("query")
    .expect("a recorded digest")
    .try_get_by_index::<String>(0)
    .expect("a value")
}

/// The digest is the schema's alone: two databases this build creates have
/// the same, which is the one each recorded, and rows change nothing to it.
#[tokio::test]
async fn a_database_records_the_digest_of_the_schema_it_was_created_with() {
    let first = setup_sqlite_db().await;
    let second = setup_sqlite_db().await;
    let digest = schema_digest(&first).await.expect("digest");
    assert_eq!(digest.len(), 64);
    assert_eq!(digest, schema_digest(&second).await.expect("digest"));
    assert_eq!(recorded_digest(&first).await, digest);

    TreeRepo::create(&first, Uuid::now_v7(), "Fictional tree".into(), None)
        .await
        .expect("tree");
    first
        .execute_unprepared("ANALYZE")
        .await
        .expect("statistics");
    assert_eq!(schema_digest(&first).await.expect("digest"), digest);

    // Any change to the schema changes it.
    second
        .execute_unprepared("CREATE INDEX idx_fixture ON place (name)")
        .await
        .expect("an index");
    assert_ne!(schema_digest(&second).await.expect("digest"), digest);
}

/// A file this build created opens again; one that recorded another digest,
/// or none, is refused before anything is written to it.
#[tokio::test]
async fn a_file_created_with_another_schema_is_refused() {
    let (directory, url) = file_database();
    let db = connect(&url).await.expect("connect");
    assert_eq!(schema_state(&db).await.expect("state"), SchemaState::Fresh);
    run_migrations(&db).await.expect("migrations");
    db.close().await.expect("close");

    // The same build, again: nothing to apply.
    let db = connect(&url).await.expect("connect");
    assert_eq!(
        schema_state(&db).await.expect("state"),
        SchemaState::Existing { pending: false }
    );
    run_migrations(&db).await.expect("the file opens again");

    // As an earlier edit of the initial migration left it.
    db.execute_unprepared("UPDATE schema_meta SET value = 'another' WHERE key = 'schema_digest'")
        .await
        .expect("another digest");
    assert!(matches!(
        run_migrations(&db).await,
        Err(MigrationError::SchemaMismatch)
    ));
    // As a build that recorded nothing left it.
    db.execute_unprepared("DROP TABLE schema_meta")
        .await
        .expect("no record");
    let refused = run_migrations(&db).await;
    assert!(
        matches!(refused, Err(MigrationError::SchemaMismatch)),
        "{refused:?}"
    );
    assert_eq!(
        refused.expect_err("refused").to_string(),
        "the database was created with another schema than this version's"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&directory);
}

/// A migrated database is sound; a row naming a row that does not exist is
/// found, as a count that names nothing.
#[tokio::test]
async fn the_integrity_check_counts_what_it_finds() {
    let db = setup_sqlite_db().await;
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&db, tree_id, "Fictional tree".into(), None)
        .await
        .expect("tree");
    let report = check_integrity(&db).await.expect("check");
    assert!(report.is_sound(), "{report:?}");

    // As a tool that does not enforce the keys leaves it.
    db.execute_unprepared(&format!(
        "PRAGMA foreign_keys = OFF; \
         INSERT INTO sosa_ancestor (person_id, tree_id) VALUES ('{}', '{tree_id}'); \
         PRAGMA foreign_keys = ON",
        Uuid::now_v7()
    ))
    .await
    .expect("an ancestor who is no person");
    let report = check_integrity(&db).await.expect("check");
    assert_eq!((report.damaged, report.dangling), (0, 1));
    assert!(!report.is_sound());
}

/// A copy is a database of its own: it opens with this build, holds what
/// was committed, and the original goes on.
#[tokio::test]
async fn a_copy_of_the_file_opens_and_holds_what_was_committed() {
    let (directory, url) = file_database();
    let db = connect(&url).await.expect("connect");
    run_migrations(&db).await.expect("migrations");
    let tree_id = Uuid::now_v7();
    TreeRepo::create(&db, tree_id, "Fictional tree".into(), None)
        .await
        .expect("tree");

    let copy = directory.join("copy's.db");
    snapshot_into(&db, &copy).await.expect("a copy");
    assert!(
        snapshot_into(&db, &copy).await.is_err(),
        "a copy never replaces a file"
    );
    TreeRepo::create(&db, Uuid::now_v7(), "Written after".into(), None)
        .await
        .expect("the original goes on");

    let restored = connect(&format!("sqlite://{}?mode=rw", copy.display()))
        .await
        .expect("the copy opens");
    run_migrations(&restored)
        .await
        .expect("the copy holds this build's schema");
    assert!(check_integrity(&restored).await.expect("check").is_sound());
    assert_eq!(
        TreeRepo::get(&restored, tree_id).await.expect("tree").name,
        "Fictional tree"
    );

    drop((db, restored));
    let _ = std::fs::remove_dir_all(&directory);
}
