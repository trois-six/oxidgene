//! Migration: data-model groundwork for the person edit modal spec.
//!
//! Adds `person.privacy`, structured date qualifier/calendar/witnesses/cause
//! columns on `event`, and `is_profile` + date/place columns on `media`
//! and `media_link`.
//!
//! Each `ADD COLUMN` is a separate statement — SQLite does not support
//! multiple alter options in a single ALTER TABLE call.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ── person.privacy ──
        manager
            .alter_table(
                Table::alter()
                    .table(Person::Table)
                    .add_column(
                        ColumnDef::new(Person::Privacy)
                            .string_len(10)
                            .not_null()
                            .default("default"),
                    )
                    .to_owned(),
            )
            .await?;

        // ── event columns (one per statement for SQLite) ──
        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .add_column(
                        ColumnDef::new(Event::DateQualifier)
                            .string_len(10)
                            .not_null()
                            .default("exact"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .add_column(ColumnDef::new(Event::DateValue2).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .add_column(
                        ColumnDef::new(Event::Calendar)
                            .string_len(20)
                            .not_null()
                            .default("gregorian"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .add_column(ColumnDef::new(Event::Witnesses).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .add_column(ColumnDef::new(Event::Cause).string().null())
                    .to_owned(),
            )
            .await?;

        // ── media columns (one per statement for SQLite) ──
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(ColumnDef::new(Media::DateValue).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(ColumnDef::new(Media::DateSort).date().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(ColumnDef::new(Media::PlaceId).uuid().null())
                    .to_owned(),
            )
            .await?;

        // Note: SQLite does not support ADD FOREIGN KEY via ALTER TABLE.
        // The media → place FK is enforced at the ORM layer only.

        // ── media_link.is_profile ──
        manager
            .alter_table(
                Table::alter()
                    .table(MediaLink::Table)
                    .add_column(
                        ColumnDef::new(MediaLink::IsProfile)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MediaLink::Table)
                    .drop_column(MediaLink::IsProfile)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .drop_column(Media::DateValue)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .drop_column(Media::DateSort)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .drop_column(Media::PlaceId)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .drop_column(Event::DateQualifier)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .drop_column(Event::DateValue2)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .drop_column(Event::Calendar)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .drop_column(Event::Witnesses)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Event::Table)
                    .drop_column(Event::Cause)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Person::Table)
                    .drop_column(Person::Privacy)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Privacy,
}

#[derive(DeriveIden)]
enum Event {
    Table,
    DateQualifier,
    DateValue2,
    Calendar,
    Witnesses,
    Cause,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    DateValue,
    DateSort,
    PlaceId,
}

#[derive(DeriveIden)]
enum MediaLink {
    Table,
    IsProfile,
}
