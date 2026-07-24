//! Add original-cased `surname_display` / `given_names_display` columns to
//! `person_search_fts`.
//!
//! Search matching uses the normalized (lowercase, accent-folded) `surname`
//! / `given_names` columns, so the UI had no properly-cased surname/given-name
//! split to render and resorted to guessing one by splitting `display_name`
//! on whitespace — which breaks for compound surnames (e.g. Breton "LE
//! NADAN") and compound given names alike. These columns carry the original
//! casing so callers never need to guess.
//!
//! `person_search_fts` is a derived cache table (rebuilt on demand by
//! `CacheService::ensure_search_index` whenever empty for a tree), so this
//! migration just drops and recreates it rather than altering in place.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute(Statement::from_string(
            manager.get_database_backend(),
            "DROP TABLE IF EXISTS person_search_fts".to_owned(),
        ))
        .await?;

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
                        surname_display UNINDEXED,
                        given_names_display UNINDEXED,
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
                        surname_display TEXT NOT NULL DEFAULT '',
                        given_names_display TEXT NOT NULL DEFAULT '',
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
}
