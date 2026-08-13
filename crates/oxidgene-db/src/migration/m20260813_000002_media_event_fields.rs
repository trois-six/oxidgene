//! Sprint F.3 — give a media the same date a fact has, and let a note hang
//! off one.
//!
//! # Why `date_value` alone was never enough
//!
//! `media` has carried `date_value` and `date_sort` since the initial schema,
//! and neither could hold what a genealogical date actually is. "Around 1890",
//! "between 1914 and 1918" and "11 ventôse an IV" are the ordinary cases for a
//! photograph or a scan, and expressing them needs the same three columns an
//! `event` has: which qualifier frames the date, the second date a range
//! needs, and which calendar the value was written in. Without them the date
//! editor — one widget, used everywhere — could not be pointed at a media at
//! all, so the field was unreachable from the UI.
//!
//! The columns mirror `event`'s exactly, defaults included, so `date_sort` is
//! derived by the same code on both and a photograph sorts against the events
//! it sits between.
//!
//! # Why `note` gained a `media_id`
//!
//! A note about a document — "the left-hand column is water-damaged", "this is
//! the 1872 copy, not the original" — had nowhere to live: `note` could point
//! at a person, an event, a family or a source, and a media is none of those.
//! The alternative was to overload `media.description`, which is the caption
//! shown under a tile; conflating "what this is" with "what to know about it"
//! means one of the two is always in the wrong place.
//!
//! Deliberately **no** `source_id` on the media side: a media *is* a source
//! document. Asking which source backs a scan of a parish register is asking
//! the scan to cite itself.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One ADD COLUMN per ALTER TABLE — SQLite accepts no more.
        for column in [
            // Same widths and defaults as `event`, so the two are read and
            // written by the same code.
            ColumnDef::new(Media::DateQualifier)
                .string_len(10)
                .not_null()
                .default("exact")
                .to_owned(),
            ColumnDef::new(Media::DateValue2).string().null().to_owned(),
            ColumnDef::new(Media::Calendar)
                .string_len(20)
                .not_null()
                .default("gregorian")
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

        // No FK: SQLite cannot attach one through ALTER TABLE, which is the
        // same reason `media.place_id` has none. Enforced at the ORM layer.
        manager
            .alter_table(
                Table::alter()
                    .table(Note::Table)
                    .add_column(uuid_null(Note::MediaId))
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_note_media_id")
                    .table(Note::Table)
                    .col(Note::MediaId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_note_media_id")
                    .table(Note::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Note::Table)
                    .drop_column(Note::MediaId)
                    .to_owned(),
            )
            .await?;
        for column in [Media::Calendar, Media::DateValue2, Media::DateQualifier] {
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
    DateQualifier,
    DateValue2,
    Calendar,
}

#[derive(DeriveIden)]
enum Note {
    Table,
    MediaId,
}
