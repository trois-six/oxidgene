//! Widen `person_search_fts` with each person's close relatives.
//!
//! A search hit used to carry only the person's own names and years, so the UI
//! had to make a second round trip to say who someone was married to. The
//! spouse, parent and children figures now live on the search row itself.
//!
//! The table is recreated rather than altered: on SQLite it is an FTS5 virtual
//! table, and `ALTER TABLE … ADD COLUMN` does not apply to those. PostgreSQL
//! could take an `ALTER`, but recreating on both keeps one code path. This is
//! safe because the table is a pure projection — `ProfileService::ensure_materialized`
//! sees the empty row count and rebuilds it on the next read. The accompanying
//! `PROJECTION_SCHEMA_VERSION` bump forces that rebuild even on a tree whose
//! `person_denorm` rows are otherwise intact.
//!
//! The relative columns are all UNINDEXED: free-text search must keep matching
//! the person themselves, so that searching "Pierre" does not return everyone
//! married to a Pierre. The structured `spouse_*` / `father_*` / `mother_*`
//! filters read the normalized columns directly.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        recreate(manager, WITH_RELATIVES).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        recreate(manager, WITHOUT_RELATIVES).await
    }
}

/// Column lists for the two shapes, as `(sqlite, postgres)` fragments.
struct Shape {
    sqlite: &'static str,
    postgres: &'static str,
}

const WITH_RELATIVES: Shape = Shape {
    sqlite: "
        surname,
        given_names,
        maiden_name,
        birth_year,
        death_year,
        person_id UNINDEXED,
        tree_id UNINDEXED,
        sex UNINDEXED,
        display_name UNINDEXED,
        surname_display UNINDEXED,
        given_names_display UNINDEXED,
        birth_place UNINDEXED,
        date_sort UNINDEXED,
        birth_qualifier UNINDEXED,
        death_qualifier UNINDEXED,
        spouse_names UNINDEXED,
        spouse_surnames UNINDEXED,
        spouse_given_names UNINDEXED,
        father_name UNINDEXED,
        father_surname UNINDEXED,
        father_given_names UNINDEXED,
        mother_name UNINDEXED,
        mother_surname UNINDEXED,
        mother_given_names UNINDEXED,
        children_count UNINDEXED
    ",
    postgres: "
        person_id TEXT NOT NULL PRIMARY KEY,
        tree_id TEXT NOT NULL,
        surname TEXT NOT NULL DEFAULT '',
        given_names TEXT NOT NULL DEFAULT '',
        maiden_name TEXT,
        birth_year TEXT,
        death_year TEXT,
        sex TEXT NOT NULL DEFAULT 'unknown',
        display_name TEXT NOT NULL DEFAULT '',
        surname_display TEXT NOT NULL DEFAULT '',
        given_names_display TEXT NOT NULL DEFAULT '',
        birth_place TEXT,
        date_sort TEXT,
        birth_qualifier TEXT NOT NULL DEFAULT 'exact',
        death_qualifier TEXT NOT NULL DEFAULT 'exact',
        spouse_names TEXT NOT NULL DEFAULT '',
        spouse_surnames TEXT NOT NULL DEFAULT '',
        spouse_given_names TEXT NOT NULL DEFAULT '',
        father_name TEXT,
        father_surname TEXT,
        father_given_names TEXT,
        mother_name TEXT,
        mother_surname TEXT,
        mother_given_names TEXT,
        children_count TEXT NOT NULL DEFAULT '0'
    ",
};

const WITHOUT_RELATIVES: Shape = Shape {
    sqlite: "
        surname,
        given_names,
        maiden_name,
        birth_year,
        death_year,
        person_id UNINDEXED,
        tree_id UNINDEXED,
        sex UNINDEXED,
        display_name UNINDEXED,
        surname_display UNINDEXED,
        given_names_display UNINDEXED,
        birth_place UNINDEXED,
        date_sort UNINDEXED
    ",
    postgres: "
        person_id TEXT NOT NULL PRIMARY KEY,
        tree_id TEXT NOT NULL,
        surname TEXT NOT NULL DEFAULT '',
        given_names TEXT NOT NULL DEFAULT '',
        maiden_name TEXT,
        birth_year TEXT,
        death_year TEXT,
        sex TEXT NOT NULL DEFAULT 'unknown',
        display_name TEXT NOT NULL DEFAULT '',
        surname_display TEXT NOT NULL DEFAULT '',
        given_names_display TEXT NOT NULL DEFAULT '',
        birth_place TEXT,
        date_sort TEXT
    ",
};

async fn recreate(manager: &SchemaManager<'_>, shape: Shape) -> Result<(), DbErr> {
    let conn = manager.get_connection();
    let backend = manager.get_database_backend();

    // Dropped rather than migrated: every row is derived data, and leaving the
    // table empty is the signal the projection layer already watches for.
    conn.execute_raw(Statement::from_string(
        backend,
        "DROP TABLE IF EXISTS person_search_fts".to_owned(),
    ))
    .await?;

    match backend {
        DbBackend::Sqlite => {
            conn.execute_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!(
                    "CREATE VIRTUAL TABLE person_search_fts USING fts5({})",
                    shape.sqlite
                ),
            ))
            .await?;
        }
        backend => {
            conn.execute_raw(Statement::from_string(
                backend,
                format!("CREATE TABLE person_search_fts ({})", shape.postgres),
            ))
            .await?;
            conn.execute_raw(Statement::from_string(
                backend,
                "CREATE INDEX idx_person_search_fts_tree_id \
                 ON person_search_fts (tree_id)"
                    .to_owned(),
            ))
            .await?;
        }
    }

    Ok(())
}
