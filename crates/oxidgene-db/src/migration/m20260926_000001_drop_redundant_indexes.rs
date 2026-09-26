//! Drop the single-column `tree_id` indexes that a composite index already
//! covers.
//!
//! `person_denorm`, `media` and `event` each carried both `(tree_id)` and a
//! composite index that starts with `tree_id`. Both SQLite and PostgreSQL
//! answer a `tree_id = ?` lookup from the leading column of the composite, so
//! the narrow index served no read the wide one could not — while every insert
//! still paid to maintain it. An import writes all three tables row by row, and
//! each mutation rewrites projections in `person_denorm`.
//!
//! On SQLite it then gathers planner statistics, which no installed database
//! had: without them the remaining composites looked as selective as a primary
//! key, and `tree_id = ? AND person_id IN (…)` read the whole tree instead of
//! one row per id. Imports refresh them from then on
//! (`repo::refresh_statistics`).

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend};

#[derive(DeriveMigrationName)]
pub struct Migration;

/// `(redundant index, its table, the column it indexed)`.
const REDUNDANT: [(&str, &str, &str); 3] = [
    ("idx_person_denorm_tree_id", "person_denorm", "tree_id"),
    ("idx_media_tree_id", "media", "tree_id"),
    ("idx_event_tree_id", "event", "tree_id"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, _) in REDUNDANT {
            manager
                .drop_index(Index::drop().name(name).table(Alias::new(table)).to_owned())
                .await?;
        }
        let db = manager.get_connection();
        if db.get_database_backend() == DbBackend::Sqlite {
            db.execute_unprepared("ANALYZE").await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, table, column) in REDUNDANT {
            manager
                .create_index(
                    Index::create()
                        .name(name)
                        .table(Alias::new(table))
                        .col(Alias::new(column))
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}
