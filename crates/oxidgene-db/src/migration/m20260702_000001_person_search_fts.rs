//! Create the `person_search_fts` search table (Sprint E.6).
//!
//! On SQLite (desktop) this is a real FTS5 virtual table: name tokens, birth
//! year and death year are indexed for instant prefix matching; display fields
//! are stored UNINDEXED so a search is a single table read with no joins.
//!
//! On PostgreSQL (web) FTS5 is not available, so the same columns are created
//! as a plain table with a `tree_id` index; matching falls back to `LIKE` on
//! the pre-normalized token columns. Text normalization (lowercase + accent
//! folding) is done in Rust before insert, so both backends match identically.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();

        match manager.get_database_backend() {
            DbBackend::Sqlite => {
                conn.execute(Statement::from_string(
                    DbBackend::Sqlite,
                    r#"
                    CREATE VIRTUAL TABLE IF NOT EXISTS person_search_fts USING fts5(
                        surname,
                        given_names,
                        maiden_name,
                        birth_year,
                        death_year,
                        person_id UNINDEXED,
                        tree_id UNINDEXED,
                        sex UNINDEXED,
                        display_name UNINDEXED,
                        birth_place UNINDEXED,
                        date_sort UNINDEXED
                    )
                    "#
                    .to_owned(),
                ))
                .await?;
            }
            backend => {
                conn.execute(Statement::from_string(
                    backend,
                    r#"
                    CREATE TABLE IF NOT EXISTS person_search_fts (
                        person_id TEXT NOT NULL PRIMARY KEY,
                        tree_id TEXT NOT NULL,
                        surname TEXT NOT NULL DEFAULT '',
                        given_names TEXT NOT NULL DEFAULT '',
                        maiden_name TEXT,
                        birth_year TEXT,
                        death_year TEXT,
                        sex TEXT NOT NULL DEFAULT 'unknown',
                        display_name TEXT NOT NULL DEFAULT '',
                        birth_place TEXT,
                        date_sort TEXT
                    )
                    "#
                    .to_owned(),
                ))
                .await?;
                conn.execute(Statement::from_string(
                    backend,
                    "CREATE INDEX IF NOT EXISTS idx_person_search_fts_tree_id \
                     ON person_search_fts (tree_id)"
                        .to_owned(),
                ))
                .await?;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute(Statement::from_string(
            manager.get_database_backend(),
            "DROP TABLE IF EXISTS person_search_fts".to_owned(),
        ))
        .await?;
        Ok(())
    }
}
