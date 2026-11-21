//! Verify the consolidated schema on SQLite and, explicitly, PostgreSQL.

use oxidgene_db::Migrator;
use oxidgene_db::repo::{connect, rollback_migrations, run_migrations};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use sea_orm_migration::{MigratorTrait, SchemaManager};

const TABLES: &[&str] = &[
    "tree",
    "person",
    "person_name",
    "family",
    "family_spouse",
    "family_child",
    "place",
    "event",
    "event_witness",
    "event_spouse_age",
    "source",
    "repository",
    "source_repository",
    "citation",
    "media",
    "media_link",
    "note",
    "vignette",
    "tag",
    "record_tag",
    "background_job",
    "person_search",
    "person_search_token",
    "person_denorm",
    "person_distinct",
    "audit_entry",
    "record_version",
    "gedcom_kept",
];

#[tokio::test]
async fn test_migrate_up_and_down_sqlite() {
    let db = connect("sqlite::memory:")
        .await
        .expect("Failed to connect to in-memory SQLite");

    assert_migration_lifecycle(&db).await;
}

/// The same migration run, against a real PostgreSQL.
///
/// `OXIDGENE_TEST_DATABASE_URL` must name an empty disposable database.
/// Run it with, for example:
///
/// ```text
/// OXIDGENE_TEST_DATABASE_URL=postgres://fixture:fixture@localhost/fixture_test \
///   cargo nextest run -p oxidgene-db --features postgres --test integration \
///     --run-ignored only -E 'test(/^migration_test::/)'
/// ```
#[tokio::test]
#[cfg(feature = "postgres")]
#[ignore = "needs an empty disposable PostgreSQL database: run by `just test-postgres`"]
async fn test_migrate_up_and_down_postgres() {
    let url = std::env::var("OXIDGENE_TEST_DATABASE_URL")
        .expect("set OXIDGENE_TEST_DATABASE_URL to an empty disposable PostgreSQL database");
    let db = connect(&url).await.expect("connect to PostgreSQL");
    assert_eq!(
        db.get_database_backend(),
        sea_orm::DatabaseBackend::Postgres
    );
    assert_migration_lifecycle(&db).await;
}

async fn assert_migration_lifecycle(db: &DatabaseConnection) {
    assert_no_table(db, "exists").await;

    // The one consolidated migration creates the whole current schema, and
    // running the migrator again finds nothing left to apply.
    run_migrations(db).await.expect("Migration up failed");
    assert_current_schema(db).await;
    run_migrations(db).await.expect("Repeated migration failed");
    assert_current_schema(db).await;

    rollback_migrations(db)
        .await
        .expect("Migration down failed");
    assert_no_table(db, "remains").await;
    assert!(
        Migrator::get_applied_migrations(db)
            .await
            .unwrap()
            .is_empty()
    );

    run_migrations(db).await.expect("Re-migration up failed");
    assert_current_schema(db).await;

    rollback_migrations(db).await.expect("cleanup failed");
}

/// None of the schema's tables exists; `state` names the failure.
async fn assert_no_table(db: &DatabaseConnection, state: &str) {
    let manager = SchemaManager::new(db);
    for table in TABLES.iter().chain(&["schema_meta"]) {
        assert!(!manager.has_table(*table).await.unwrap(), "{table} {state}");
    }
}

/// Run one statement.
async fn execute(db: &DatabaseConnection, sql: &str) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_string(db.get_database_backend(), sql))
        .await
        .map(drop)
}

/// The first row of a query that must return one.
async fn query_one(db: &DatabaseConnection, sql: &str) -> sea_orm::QueryResult {
    db.query_one_raw(Statement::from_string(db.get_database_backend(), sql))
        .await
        .unwrap()
        .unwrap()
}

async fn assert_current_schema(db: &DatabaseConnection) {
    assert_applied_migrations(db).await;
    assert_tables_and_columns(db).await;
    assert_indexes(db).await;
    assert_optional_defaults(db).await;
    assert_person_search(db).await;
}

/// The consolidated initial migration is the only one, and it ran.
async fn assert_applied_migrations(db: &DatabaseConnection) {
    let applied = Migrator::get_applied_migrations(db).await.unwrap();
    let applied: Vec<&str> = applied.iter().map(|m| m.name()).collect();
    assert_eq!(applied, vec!["m20250101_000001_initial"]);
}

/// Every table exists with its current columns, and the dropped ones are
/// gone.
async fn assert_tables_and_columns(db: &DatabaseConnection) {
    let backend = db.get_database_backend();
    let manager = SchemaManager::new(db);
    for table in TABLES {
        assert!(manager.has_table(*table).await.unwrap(), "missing {table}");
    }
    assert!(!manager.has_table("person_ancestry").await.unwrap());
    assert!(!manager.has_column("media", "is_document").await.unwrap());
    assert!(
        !manager
            .has_column("source", "repository_name")
            .await
            .unwrap()
    );

    for (table, columns) in [
        (
            "tree",
            "sosa_root_person_id, self_person_id, default_privacy, submitter_name, submitter_email, submitter_address, anomaly_thresholds, trusted_media_hosts",
        ),
        ("person", "privacy, portrait_media_id, portrait_vignette_id"),
        ("person_name", "tree_id, surname_prefix, sort_order"),
        ("family_spouse", "tree_id"),
        ("family_child", "tree_id"),
        ("event_witness", "tree_id"),
        ("event_spouse_age", "tree_id"),
        ("citation", "tree_id"),
        ("media_link", "tree_id"),
        ("access_scope_person", "tree_id, scope_id, person_id"),
        ("family", "privacy"),
        (
            "event",
            "date_qualifier, date_value2, calendar, cause, age, agency, classification, related_family_id",
        ),
        ("source", "agency"),
        ("note", "repository_id"),
        (
            "source_repository",
            "tree_id, source_id, repository_id, call_number, media_type, sort_order",
        ),
        (
            "media",
            "storage_key, sha256, thumbnail_key, card_key, thumbnail_url, width, height, page_count, parent_media_id, page_index, date_qualifier, date_value2, calendar, source_media_type, document_category, place_id, privacy",
        ),
        (
            "vignette",
            "id, tree_id, media_id, x, y, width, height, person_id, event_id, created_at, updated_at",
        ),
        (
            "tag",
            "id, tree_id, label, normalized_label, color, created_at, updated_at",
        ),
        (
            "record_tag",
            "id, tree_id, tag_id, person_id, family_id, event_id, place_id, source_id, citation_id, repository_id, media_id, note_id, created_at",
        ),
        (
            "gedcom_kept",
            "id, tree_id, entity_kind, entity_id, nodes, created_at",
        ),
        ("note", "media_id"),
        (
            "background_job",
            "active_tree_id, lease_owner, lease_until, trace_parent, trace_state",
        ),
        (
            "person_denorm",
            "person_id, tree_id, payload, schema_version, updated_at",
        ),
        (
            "person_distinct",
            "id, tree_id, person_id, other_person_id, created_at",
        ),
        (
            "audit_entry",
            "id, tree_id, occurred_at, category, action, entity, entity_id, subject, subject_id, label, details",
        ),
        (
            "record_version",
            "id, tree_id, audit_entry_id, record_type, record_id, version, deleted, snapshot, labels",
        ),
        (
            "person_search",
            "person_id, tree_id, surname_key, given_key, maiden_key, sex, birth_year, birth_qualifier, death_year, death_qualifier, date_sort, display_name, surname_display, given_names_display, birth_place, spouse_names, father_name, mother_name, children_count",
        ),
        ("person_search_token", "tree_id, token, person_id, fields"),
    ] {
        db.query_all_raw(Statement::from_string(
            backend,
            format!("SELECT {columns} FROM {table} WHERE 1 = 0"),
        ))
        .await
        .unwrap_or_else(|err| panic!("current {table} columns: {err}"));
    }
}

/// The indexes the queries rely on exist, and the redundant ones are gone.
async fn assert_indexes(db: &DatabaseConnection) {
    let manager = SchemaManager::new(db);
    for (table, index) in [
        ("background_job", "idx_background_job_active_tree"),
        ("background_job", "idx_background_job_claim"),
        ("person_denorm", "idx_person_denorm_tree_schema_version"),
        // A page of projections, of invitations and of share links, and an
        // entry's versions, each in the order it pages in.
        ("person_denorm", "idx_person_denorm_tree_person"),
        ("tree_invitation", "idx_tree_invitation_tree_id"),
        ("share_link", "idx_share_link_tree_id"),
        ("record_version", "idx_record_version_audit_entry"),
        ("media", "idx_media_tree_sha256"),
        ("media", "idx_media_parent_page"),
        ("person", "idx_person_portrait_media_id"),
        ("person", "idx_person_portrait_vignette_id"),
        ("note", "idx_note_media_id"),
        ("family_child", "idx_family_child_person_id"),
        ("family_spouse", "idx_family_spouse_person_id"),
        ("person_distinct", "idx_person_distinct_pair"),
        ("person_distinct", "idx_person_distinct_other_person_id"),
        ("audit_entry", "idx_audit_entry_tree"),
        ("audit_entry", "idx_audit_entry_tree_category"),
        ("audit_entry", "idx_audit_entry_tree_subject"),
        ("record_version", "idx_record_version_record"),
        ("record_version", "idx_record_version_audit_entry"),
        ("record_version", "idx_record_version_tree"),
        // One row per rule (docs/data-model.md, Integrity constraints).
        ("family_spouse", "uq_family_spouse_family_person"),
        ("family_child", "uq_family_child_family_person"),
        ("event_witness", "uq_event_witness_event_person_relation"),
        ("person_name", "uq_person_name_primary"),
        // `(tree_id, id)`: the target of the composite foreign keys, and a
        // page of the tree's records in id order.
        ("person", "uq_person_tree_id"),
        ("family", "uq_family_tree_id"),
        ("family_spouse", "uq_family_spouse_tree_id"),
        ("event", "uq_event_tree_id"),
        ("place", "uq_place_tree_id"),
        ("source", "uq_source_tree_id"),
        ("repository", "uq_repository_tree_id"),
        ("citation", "uq_citation_tree_id"),
        ("media", "uq_media_tree_id"),
        ("note", "uq_note_tree_id"),
        ("access_scope", "uq_access_scope_tree_id"),
        ("person_name", "idx_person_name_tree_surname"),
        // The person search's name order, its birth years and a person's
        // words (docs/data-model.md §4.3).
        ("person_search", "idx_person_search_tree_name"),
        ("person_search", "idx_person_search_tree_birth_year"),
        ("person_search_token", "idx_person_search_token_person"),
    ] {
        assert!(
            manager.has_index(table, index).await.unwrap(),
            "missing {index}"
        );
    }
    // Never created: the composite indexes above, which lead with `tree_id`,
    // serve every read a `tree_id` index of its own would; the others serve
    // no read at all.
    for (table, index) in [
        ("person_denorm", "idx_person_denorm_tree_id"),
        ("media", "idx_media_tree_id"),
        ("event", "idx_event_tree_id"),
        // Replaced by the unique `(tree_id, id)` and `(family_id, person_id)`
        // indexes, which lead with the same column.
        ("person", "idx_person_tree_id"),
        ("family", "idx_family_tree_id"),
        ("place", "idx_place_tree_id"),
        ("source", "idx_source_tree_id"),
        ("repository", "idx_repository_tree_id"),
        ("note", "idx_note_tree_id"),
        ("access_scope", "idx_access_scope_tree_id"),
        ("family_spouse", "idx_family_spouse_family_id"),
        ("family_child", "idx_family_child_family_id"),
        ("event_witness", "idx_event_witness_event_id"),
        // The surname index of every tree at once.
        ("person_name", "idx_person_name_surname"),
        // No read filters given names by equality or range.
        ("person_name", "idx_person_name_given_names"),
        // Its only range read is the media library's year filter, which
        // drives better from the tree's media.
        ("event", "idx_event_date_sort"),
    ] {
        assert!(
            !manager.has_index(table, index).await.unwrap(),
            "redundant {index}"
        );
    }
}

/// A job's trace context is optional, and a new projection row starts at
/// schema version 0.
async fn assert_optional_defaults(db: &DatabaseConnection) {
    execute(
        db,
        "INSERT INTO tree (id, name, created_at, updated_at) \
         VALUES ('00000000-0000-7000-8000-000000000003', 'Fixture', \
         CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .await
    .unwrap();
    execute(
        db,
        "INSERT INTO background_job (id, tree_id, kind, format, status, phase, created_at, updated_at) \
         VALUES ('00000000-0000-7000-8000-000000000001', \
         '00000000-0000-7000-8000-000000000003', 'import', 'gedcom', 'queued', \
         'staging', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .await
    .expect("trace context must be optional");
    let job = query_one(db, "SELECT trace_parent, trace_state FROM background_job").await;
    assert_eq!(
        job.try_get::<Option<String>>("", "trace_parent").unwrap(),
        None
    );
    assert_eq!(
        job.try_get::<Option<String>>("", "trace_state").unwrap(),
        None
    );
    execute(
        db,
        "INSERT INTO person (id, tree_id, sex, created_at, updated_at) \
         VALUES ('00000000-0000-7000-8000-000000000002', \
         '00000000-0000-7000-8000-000000000003', 'unknown', \
         CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
    )
    .await
    .unwrap();
    execute(
        db,
        "INSERT INTO person_denorm (person_id, tree_id, payload, updated_at) \
         VALUES ('00000000-0000-7000-8000-000000000002', \
         '00000000-0000-7000-8000-000000000003', '{}', CURRENT_TIMESTAMP)",
    )
    .await
    .unwrap();
    let projection = query_one(db, "SELECT schema_version FROM person_denorm").await;
    assert_eq!(projection.try_get::<i32>("", "schema_version").unwrap(), 0);
    execute(db, "DELETE FROM tree").await.unwrap();
}

/// The search tables find a row by a word prefix, the same statement on
/// both backends, and compare their words byte by byte: an upper-case
/// letter sorts before every lower-case one, which a linguistic collation
/// would interleave.
async fn assert_person_search(db: &DatabaseConnection) {
    let tree = "'00000000-0000-7000-8000-000000000003'";
    let person = "'00000000-0000-7000-8000-000000000002'";
    execute(
        db,
        &format!("INSERT INTO tree (id, name, created_at, updated_at) VALUES ({tree}, 'Fixture', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"),
    )
    .await
    .unwrap();
    execute(
        db,
        &format!(
            "INSERT INTO person (id, tree_id, sex, created_at, updated_at) \
             VALUES ({person}, {tree}, 'unknown', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)"
        ),
    )
    .await
    .unwrap();
    execute(
        db,
        &format!(
            "INSERT INTO person_search (person_id, tree_id, surname_key, given_key, sex, \
             birth_qualifier, death_qualifier, display_name, surname_display, \
             given_names_display, spouse_names, children_count) \
             VALUES ({person}, {tree}, 'fixture', 'example', 'unknown', 'exact', 'exact', \
             'Example Fixture', 'Fixture', 'Example', '', 0)"
        ),
    )
    .await
    .unwrap();
    for token in ["fixture", "Zed", "example"] {
        execute(
            db,
            &format!(
                "INSERT INTO person_search_token (tree_id, token, person_id, fields) \
                 VALUES ({tree}, '{token}', {person}, 1)"
            ),
        )
        .await
        .unwrap();
    }
    let tokens = |condition: &str| {
        let sql = format!("SELECT token FROM person_search_token WHERE {condition} ORDER BY token");
        async move {
            db.query_all_raw(Statement::from_string(db.get_database_backend(), sql))
                .await
                .unwrap()
                .iter()
                .map(|row| row.try_get::<String>("", "token").unwrap())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        tokens("token >= 'fix' AND token < 'fiy'").await,
        ["fixture"]
    );
    assert_eq!(tokens("1 = 1").await, ["Zed", "example", "fixture"]);
    // The words go with their person.
    execute(db, "DELETE FROM person").await.unwrap();
    assert!(tokens("1 = 1").await.is_empty());
    execute(db, "DELETE FROM tree").await.unwrap();
}
