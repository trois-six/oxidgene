//! Drop the `person_ancestry` closure table, and index the last unindexed
//! foreign keys.
//!
//! The closure table stored every (ancestor, descendant, depth) triple so that
//! traversal was a single indexed read. Measured on a real 10k-person tree it
//! held 364k rows and, with its four indexes, took 73.5 MB — 62 % of the whole
//! database — to encode 15 704 parent-child edges, and it had to be rebuilt
//! whenever a person was re-parented.
//!
//! `AncestryRepo` now answers the same two queries with a recursive CTE over
//! the family links, about 12x faster than reading the table it replaces
//! (13 ms against 160 ms for a depth-10 pedigree). Results are identical:
//! checked against the real table on the 200 deepest pedigrees of a
//! 15k-person database, same ancestor sets at the same depths, no exceptions.
//! Both report the *shortest* distance for an ancestor reachable by several
//! paths, so pedigree implex behaves exactly as before.
//!
//! The two `event.place_id` / `note.*` index sets are unrelated to that, but
//! they are the last foreign keys in the schema with no index behind them —
//! `ON DELETE SET NULL` from `place` and `ON DELETE CASCADE` from
//! `person`/`event`/`family`/`source` each had to scan the child table per
//! deleted parent row. Measured impact on tree deletion was below noise, so
//! this is tidiness rather than a fix, and it completes the sweep begun in
//! `m20260724_000002`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum PersonAncestry {
    Table,
}

#[derive(DeriveIden)]
enum Event {
    Table,
    PlaceId,
}

#[derive(DeriveIden)]
enum Note {
    Table,
    PersonId,
    EventId,
    FamilyId,
    SourceId,
}

const INDEXES: &[(&str, &str)] = &[
    ("idx_event_place_id", "event"),
    ("idx_note_person_id", "note"),
    ("idx_note_event_id", "note"),
    ("idx_note_family_id", "note"),
    ("idx_note_source_id", "note"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PersonAncestry::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_event_place_id")
                    .table(Event::Table)
                    .col(Event::PlaceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_person_id")
                    .table(Note::Table)
                    .col(Note::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_event_id")
                    .table(Note::Table)
                    .col(Note::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_family_id")
                    .table(Note::Table)
                    .col(Note::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_source_id")
                    .table(Note::Table)
                    .col(Note::SourceId)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    /// Recreates the table empty. Its contents were derived from the family
    /// links, so there is nothing to restore — rolling back and then rolling
    /// forward again simply loses a cache that no code reads any more.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for (name, table) in INDEXES {
            manager
                .drop_index(
                    Index::drop()
                        .name(*name)
                        .table(Alias::new(*table))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .get_connection()
            .execute_unprepared(
                r#"
                CREATE TABLE IF NOT EXISTS person_ancestry (
                    id             uuid NOT NULL PRIMARY KEY,
                    tree_id        uuid NOT NULL,
                    ancestor_id    uuid NOT NULL,
                    descendant_id  uuid NOT NULL,
                    depth          integer NOT NULL
                )
                "#,
            )
            .await?;
        Ok(())
    }
}
