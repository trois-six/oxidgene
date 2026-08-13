//! Sprint F.1 — give media somewhere to keep its bytes, and let a crop out of
//! a scan be a first-class thing.
//!
//! # Why `file_path` was not enough
//!
//! `media.file_path` holds whatever a GEDCOM `OBJE.FILE` tag said — a path on
//! the machine of whoever produced the file, sometimes relative, sometimes
//! `D:\Photos\grandpere.jpg`. It is the value we must round-trip back out on
//! export, so it cannot double as "where OxidGene put the bytes". This
//! migration adds `storage_key` for the second job and leaves `file_path`
//! alone for the first. A row imported from GEDCOM keeps its path and has a
//! null key, which is precisely the "we know about this file but do not have
//! it" state the UI needs to distinguish.
//!
//! # Vignettes
//!
//! A single register page carries entries for four families. Copying the scan
//! four times and cropping each is what every other genealogy tool makes you
//! do; a `vignette` is a rectangle recorded against the one stored scan
//! instead, so re-scanning at higher resolution does not orphan the crops.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SQLite accepts exactly one ADD COLUMN per ALTER TABLE, so each
        // column is its own statement rather than one batched alter.
        for column in [
            ColumnDef::new(Media::StorageKey).string().null().to_owned(),
            ColumnDef::new(Media::Sha256).string().null().to_owned(),
            ColumnDef::new(Media::ThumbnailKey)
                .string()
                .null()
                .to_owned(),
            ColumnDef::new(Media::Width).integer().null().to_owned(),
            ColumnDef::new(Media::Height).integer().null().to_owned(),
            ColumnDef::new(Media::PageCount)
                .integer()
                .not_null()
                .default(1)
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Media::Table)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }

        // Answers "do we already hold these bytes for this tree?" — the lookup
        // an import does once per file when re-running against a tree that
        // already has half the archive.
        manager
            .create_index(
                Index::create()
                    .name("idx_media_tree_sha256")
                    .table(Media::Table)
                    .col(Media::TreeId)
                    .col(Media::Sha256)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Vignette::Table)
                    .if_not_exists()
                    .col(uuid(Vignette::Id).primary_key())
                    .col(uuid(Vignette::MediaId))
                    // Which page of a multi-page document, zero-based. Always
                    // 0 for a photo.
                    .col(integer(Vignette::Page).default(0))
                    // Crop rectangle in the source image's own pixels, so it
                    // survives any change to how the page is displayed.
                    .col(integer(Vignette::X))
                    .col(integer(Vignette::Y))
                    .col(integer(Vignette::Width))
                    .col(integer(Vignette::Height))
                    .col(string_null(Vignette::Title))
                    // What the crop is of. Both null is fine: a named region
                    // of a page is worth keeping before it is attributed.
                    .col(uuid_null(Vignette::PersonId))
                    .col(uuid_null(Vignette::EventId))
                    .col(timestamp_with_time_zone(Vignette::CreatedAt))
                    .col(timestamp_with_time_zone(Vignette::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_media")
                            .from(Vignette::Table, Vignette::MediaId)
                            .to(Media::Table, Media::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_person")
                            .from(Vignette::Table, Vignette::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_event")
                            .from(Vignette::Table, Vignette::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        for (name, column) in [
            ("idx_vignette_media_id", Vignette::MediaId),
            ("idx_vignette_person_id", Vignette::PersonId),
            ("idx_vignette_event_id", Vignette::EventId),
        ] {
            manager
                .create_index(
                    Index::create()
                        .name(name)
                        .table(Vignette::Table)
                        .col(column)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Vignette::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_media_tree_sha256")
                    .table(Media::Table)
                    .to_owned(),
            )
            .await?;
        for column in [
            Media::PageCount,
            Media::Height,
            Media::Width,
            Media::ThumbnailKey,
            Media::Sha256,
            Media::StorageKey,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Media::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    TreeId,
    StorageKey,
    Sha256,
    ThumbnailKey,
    Width,
    Height,
    PageCount,
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Event {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Vignette {
    Table,
    Id,
    MediaId,
    Page,
    X,
    Y,
    Width,
    Height,
    Title,
    PersonId,
    EventId,
    CreatedAt,
    UpdatedAt,
}
