//! Add missing indexes on `citation` and `media_link` foreign-key columns.
//!
//! Both tables only ever got an index on their first FK column
//! (`source_id` / `media_id`). `citation.person_id`/`event_id`/`family_id`
//! and `media_link.person_id`/`event_id`/`source_id`/`family_id` had none,
//! so `ON DELETE CASCADE` from `person`/`event`/`family`/`source` required a
//! full table scan of `citation`/`media_link` per deleted row to find
//! matches. Harmless for a single-row delete, but a hard-delete cascading
//! from `tree` (see `TreeRepo::delete`) fans out to tens of thousands of
//! person/event rows and made the scan cost quadratic.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Citation {
    Table,
    PersonId,
    EventId,
    FamilyId,
}

#[derive(DeriveIden)]
enum MediaLink {
    Table,
    PersonId,
    EventId,
    SourceId,
    FamilyId,
}

const INDEXES: &[(&str, &str)] = &[
    ("idx_citation_person_id", "citation"),
    ("idx_citation_event_id", "citation"),
    ("idx_citation_family_id", "citation"),
    ("idx_media_link_person_id", "media_link"),
    ("idx_media_link_event_id", "media_link"),
    ("idx_media_link_source_id", "media_link"),
    ("idx_media_link_family_id", "media_link"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_person_id")
                    .table(Citation::Table)
                    .col(Citation::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_event_id")
                    .table(Citation::Table)
                    .col(Citation::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_family_id")
                    .table(Citation::Table)
                    .col(Citation::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_person_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_event_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_source_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::SourceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_family_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::FamilyId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, table) in INDEXES {
            let table_ref = Alias::new(*table);
            manager
                .drop_index(Index::drop().name(*name).table(table_ref).to_owned())
                .await?;
        }
        Ok(())
    }
}
