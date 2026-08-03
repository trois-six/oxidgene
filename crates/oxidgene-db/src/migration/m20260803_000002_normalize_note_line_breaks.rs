//! Canonicalise line breaks in note bodies stored before they were normalized.
//!
//! Notes reach the database spelling the same break three ways — `\n` from
//! GEDCOM `CONT` lines, `<br>` (usually `<br>` *and* a newline) from GeneWeb
//! `.gw`, `\n` again from the app's own textarea — which rendered as HTML gave
//! no break, a double break and no break for text the author meant
//! identically. [`crate::html::sanitize_note_html`] now folds all three into a
//! single `\n` on write; this pass folds what is already there.
//!
//! Sanitizing and normalizing are the same call, so rows written since
//! `m20260802_000001_sanitize_note_html` are re-cleaned here too — the
//! sanitizer is idempotent, so that costs a comparison and changes nothing.
//!
//! There is no meaningful `down()`, for the same reason as in that migration:
//! the original spelling is not kept anywhere.

use sea_orm::{ConnectionTrait, Statement, TryGetable, Value};
use sea_orm_migration::prelude::*;
use uuid::Uuid;

use crate::html::sanitize_note_html;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();

        let rows = conn
            .query_all(Statement::from_string(
                backend,
                "SELECT id, text FROM note WHERE text IS NOT NULL AND text <> ''",
            ))
            .await?;

        for row in rows {
            // `note.id` is a native `uuid` column on PostgreSQL, so it has to be
            // read and re-bound as a `Uuid` — reading it as a `String` works on
            // SQLite and fails on PG.
            let id = Uuid::try_get(&row, "", "id")?;
            let text = String::try_get(&row, "", "text")?;

            let normalized = sanitize_note_html(&text);
            if normalized == text {
                continue;
            }

            let sql = match backend {
                sea_orm::DatabaseBackend::Sqlite => "UPDATE note SET text = ? WHERE id = ?",
                _ => "UPDATE note SET text = $1 WHERE id = $2",
            };
            conn.execute(Statement::from_sql_and_values(
                backend,
                sql,
                [Value::from(normalized), Value::from(id)],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
