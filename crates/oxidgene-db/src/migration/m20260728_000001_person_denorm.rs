//! Add the `person_denorm` table — the materialized person projection that
//! replaced the in-process `PersonCache` (and, with it, the whole
//! `oxidgene-cache` crate).
//!
//! One row per person holds the JSON-serialized
//! [`oxidgene_core::projection::PersonProfile`]: names, key life events with
//! their place names resolved, family links carrying the *other* members'
//! display names, and media/note counts. Everything the person detail page,
//! the person card and the pedigree node need, in a single row read.
//!
//! The payload is stored as JSON rather than as typed columns on purpose: it
//! embeds nested collections (other names, events, family links) that would
//! otherwise need their own denormalized tables, and it lets the projection
//! shape evolve without a migration per displayed field. Nothing queries
//! *inside* the payload — lookups are by `person_id` or `tree_id`, and text
//! search goes through `person_search_fts`.
//!
//! `person_id` carries `ON DELETE CASCADE`, so deleting a person (or a whole
//! tree, which cascades to persons) drops its projection automatically.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum PersonDenorm {
    Table,
    PersonId,
    TreeId,
    Payload,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PersonDenorm::Table)
                    .if_not_exists()
                    .col(uuid(PersonDenorm::PersonId).primary_key())
                    .col(uuid(PersonDenorm::TreeId))
                    .col(text(PersonDenorm::Payload))
                    .col(timestamp_with_time_zone(PersonDenorm::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_denorm_person")
                            .from(PersonDenorm::Table, PersonDenorm::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_denorm_tree")
                            .from(PersonDenorm::Table, PersonDenorm::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_person_denorm_tree_id")
                    .table(PersonDenorm::Table)
                    .col(PersonDenorm::TreeId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PersonDenorm::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
