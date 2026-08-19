//! Move "which image represents this person" onto the person.
//!
//! # Why it moved
//!
//! It lived on `media_link.is_profile`, which could only ever name a whole
//! media file. But a person is very often identified *inside* a larger
//! photograph — a group portrait, a wedding party, a class photo — and that
//! region is already a first-class thing here: a `vignette`, stored as
//! coordinates on the one scan and served as its own image. There was no way
//! to say "her portrait is that face in the group photo", which is exactly the
//! portrait most people in an old family archive have.
//!
//! The alternative was a second `is_profile`, on `vignette`. That spreads the
//! invariant — at most one portrait per person — across two tables, where it
//! can no longer be established in a single statement: clearing the media
//! links and setting a vignette flag are two writes, and a failure between
//! them leaves a person with two portraits. The whole reason `is_profile` was
//! set and cleared in one statement was to prevent precisely that.
//!
//! A pointer on `person` makes the invariant structural instead of enforced:
//! there is one place to look, one row to write, and "media or vignette, never
//! both" is a check on a single row rather than a transaction spanning two
//! tables.
//!
//! # What is dropped
//!
//! `media_link.is_profile`, backfilled first so no existing choice is lost.
//! Note that the read projection never consulted it anyway — `person_denorm`
//! picked the media with the lowest `sort_order` — so a person's stored
//! portrait and the portrait their pedigree card drew could already disagree.
//! That is fixed by the same move.
//!
//! No foreign keys: SQLite cannot add one through ALTER TABLE, which is the
//! same reason `media.place_id` has none. Enforced at the ORM layer, and a
//! dangling pointer resolves to "no portrait" rather than to an error.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One ADD COLUMN per ALTER TABLE — SQLite accepts no more.
        for column in [Person::PortraitMediaId, Person::PortraitVignetteId] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Person::Table)
                        .add_column(uuid_null(column))
                        .to_owned(),
                )
                .await?;
        }

        // Carry the existing choices over before the column holding them goes.
        let db = manager.get_connection();
        let backend = db.get_database_backend();
        db.execute(sea_orm::Statement::from_string(
            backend,
            "UPDATE person SET portrait_media_id = (
                 SELECT ml.media_id FROM media_link ml
                 WHERE ml.person_id = person.id AND ml.is_profile = true
                 LIMIT 1
             )",
        ))
        .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(MediaLink::Table)
                    .drop_column(MediaLink::IsProfile)
                    .to_owned(),
            )
            .await?;

        // Answering "who does this scan represent" walks from the media, and
        // deleting a media has to find the people pointing at it.
        manager
            .create_index(
                Index::create()
                    .name("idx_person_portrait_media_id")
                    .table(Person::Table)
                    .col(Person::PortraitMediaId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_portrait_vignette_id")
                    .table(Person::Table)
                    .col(Person::PortraitVignetteId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MediaLink::Table)
                    .add_column(
                        ColumnDef::new(MediaLink::IsProfile)
                            .boolean()
                            .not_null()
                            .default(false)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        let backend = db.get_database_backend();
        // Only whole-media portraits can travel back; a vignette portrait has
        // nowhere to go in the old shape, and is dropped rather than
        // misrepresented as its containing scan.
        db.execute(sea_orm::Statement::from_string(
            backend,
            "UPDATE media_link SET is_profile = true
             WHERE EXISTS (
                 SELECT 1 FROM person p
                 WHERE p.id = media_link.person_id
                   AND p.portrait_media_id = media_link.media_id
             )",
        ))
        .await?;

        for index in [
            "idx_person_portrait_vignette_id",
            "idx_person_portrait_media_id",
        ] {
            manager
                .drop_index(Index::drop().name(index).table(Person::Table).to_owned())
                .await?;
        }
        for column in [Person::PortraitVignetteId, Person::PortraitMediaId] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Person::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Person {
    Table,
    PortraitMediaId,
    PortraitVignetteId,
}

#[derive(DeriveIden)]
enum MediaLink {
    Table,
    IsProfile,
}
