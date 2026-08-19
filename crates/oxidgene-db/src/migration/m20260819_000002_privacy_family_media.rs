//! Give a couple and a document the same privacy field a person has.
//!
//! `person.privacy` has existed since the initial schema; `family` and `media`
//! never had one, so there was no way to say "this union is private" or "do not
//! publish this scan" — and a tree published with a living couple's marriage,
//! or with a photograph of living children, has no way to withhold either.
//!
//! **Nothing enforces this yet.** Privacy is meaningful only against a viewer,
//! and there are no viewers until authentication lands (EPIC F in the roadmap's
//! numbering, deferred). What this migration buys is that the *intent* is
//! recorded now: a user classifying their tree today does not have to do it
//! again when enforcement arrives, and the enforcement work becomes a read-path
//! change rather than a schema change plus a data-entry campaign.
//!
//! Same column shape and default as `person.privacy`, so one enum, one picker
//! and one set of translations serve all three.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Written out rather than looped: the two table idents are distinct
        // types, so an array of pairs has no common type to be.
        manager
            .alter_table(
                Table::alter()
                    .table(Family::Table)
                    .add_column(privacy_column(Family::Privacy))
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(privacy_column(Media::Privacy))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .drop_column(Media::Privacy)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Family::Table)
                    .drop_column(Family::Privacy)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

/// The same shape and default as `person.privacy`, so one enum, one picker and
/// one set of translations serve all three.
fn privacy_column<T: IntoIden>(column: T) -> ColumnDef {
    ColumnDef::new(column)
        .string_len(10)
        .not_null()
        .default("default")
        .to_owned()
}

#[derive(DeriveIden)]
enum Family {
    Table,
    Privacy,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Privacy,
}
