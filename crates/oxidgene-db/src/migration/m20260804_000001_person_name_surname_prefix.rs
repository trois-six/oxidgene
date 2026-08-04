//! Split the surname particle out of `person_name.surname` into its own
//! `surname_prefix` column, and add `sort_order`.
//!
//! `surname_prefix` is GEDCOM's `SPFX`: the particle preceding the surname
//! ("de la" in "de la Cruz"). It was previously glued into `surname`, which
//! made "de la Cruz" and "Cruz" two unrelated entries in the surname
//! dictionary and filed the former under D with no way to file it under C.
//! `ged_io` has parsed and written `SPFX` all along — OxidGene simply dropped
//! it on import and never emitted it on export.
//!
//! The backfill re-splits every existing surname with
//! [`split_surname_particle`], the same function the UI now uses at data-entry
//! time. Display is unaffected: `PersonName::display_name` rejoins the two
//! parts, so a row that read "de la Cruz" still reads "de la Cruz".
//!
//! `down()` glues the particle back onto `surname` and drops both columns, so
//! it is lossless.

use sea_orm::{ConnectionTrait, Statement, TryGetable, Value};
use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::{integer, string_null};
use uuid::Uuid;

use oxidgene_core::types::{join_surname_particle, split_surname_particle};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum PersonName {
    Table,
    SurnamePrefix,
    SortOrder,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PersonName::Table)
                    .add_column(string_null(PersonName::SurnamePrefix))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(PersonName::Table)
                    .add_column(integer(PersonName::SortOrder).default(0))
                    .to_owned(),
            )
            .await?;

        let conn = manager.get_connection();
        let backend = conn.get_database_backend();

        let rows = conn
            .query_all(Statement::from_string(
                backend,
                "SELECT id, surname FROM person_name \
                 WHERE surname IS NOT NULL AND surname <> ''",
            ))
            .await?;

        for row in rows {
            // `person_name.id` is a native `uuid` column on PostgreSQL, so it
            // has to be read and re-bound as a `Uuid` — reading it as a
            // `String` works on SQLite and fails on PG.
            let id = Uuid::try_get(&row, "", "id")?;
            let surname = String::try_get(&row, "", "surname")?;

            let (particle, root) = split_surname_particle(&surname);
            let Some(particle) = particle else { continue };

            let sql = match backend {
                sea_orm::DatabaseBackend::Sqlite => {
                    "UPDATE person_name SET surname = ?, surname_prefix = ? WHERE id = ?"
                }
                _ => "UPDATE person_name SET surname = $1, surname_prefix = $2 WHERE id = $3",
            };
            conn.execute(Statement::from_sql_and_values(
                backend,
                sql,
                [Value::from(root), Value::from(particle), Value::from(id)],
            ))
            .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        let backend = conn.get_database_backend();

        let rows = conn
            .query_all(Statement::from_string(
                backend,
                "SELECT id, surname, surname_prefix FROM person_name \
                 WHERE surname_prefix IS NOT NULL AND surname_prefix <> ''",
            ))
            .await?;

        for row in rows {
            let id = Uuid::try_get(&row, "", "id")?;
            let surname = Option::<String>::try_get(&row, "", "surname")?.unwrap_or_default();
            let particle = String::try_get(&row, "", "surname_prefix")?;

            let joined = join_surname_particle(Some(&particle), &surname);

            let sql = match backend {
                sea_orm::DatabaseBackend::Sqlite => {
                    "UPDATE person_name SET surname = ? WHERE id = ?"
                }
                _ => "UPDATE person_name SET surname = $1 WHERE id = $2",
            };
            conn.execute(Statement::from_sql_and_values(
                backend,
                sql,
                [Value::from(joined), Value::from(id)],
            ))
            .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(PersonName::Table)
                    .drop_column(PersonName::SurnamePrefix)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(PersonName::Table)
                    .drop_column(PersonName::SortOrder)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
