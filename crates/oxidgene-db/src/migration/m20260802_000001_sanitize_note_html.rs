//! Sanitize note bodies that were stored before notes were rendered as HTML.
//!
//! Notes used to be displayed as escaped plain text, so whatever an import or
//! an API client put in `note.text` was harmless. Now the UI renders it as
//! HTML, which makes every pre-existing row a potential stored-XSS carrier —
//! and OxidGene imports files it did not author (GEDCOM, GeneWeb `.gw`).
//! [`crate::html::sanitize_note_html`] guards the write paths from here on;
//! this pass cleans what is already there.
//!
//! Rows are read and rewritten in Rust rather than fixed with an SQL
//! expression: the allowlist lives in the sanitizer, and only an HTML parser
//! can apply it correctly.
//!
//! There is no meaningful `down()`. The original markup is not kept anywhere,
//! so the reverse migration is a no-op rather than a lie — re-importing the
//! source file is the way back.

use sea_orm::{ConnectionTrait, Statement, TryGetable, Value};
use sea_orm_migration::prelude::*;

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
            let id = String::try_get(&row, "", "id")?;
            let text = String::try_get(&row, "", "text")?;

            let clean = sanitize_note_html(&text);
            if clean == text {
                continue;
            }

            let sql = match backend {
                sea_orm::DatabaseBackend::Sqlite => "UPDATE note SET text = ? WHERE id = ?",
                _ => "UPDATE note SET text = $1 WHERE id = $2",
            };
            conn.execute(Statement::from_sql_and_values(
                backend,
                sql,
                [Value::from(clean), Value::from(id)],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
