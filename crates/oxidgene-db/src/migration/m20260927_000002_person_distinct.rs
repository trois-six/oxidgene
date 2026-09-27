//! Record that two same-named persons are different people.
//!
//! Saving a person whose name another person of the tree already bears asks
//! whether they are the same individual. Answering "no" has to be remembered,
//! or a father and a son sharing a name would ask the question again on every
//! edit of either. `person_distinct` holds those answers, one row per pair.
//!
//! A pair is stored once, with `person_id < other_person_id`, so the unique
//! index is what makes recording the same answer twice a no-op. Both columns
//! cascade from `person`, and `tree_id` from `tree`, so neither a purged person
//! nor a purged tree leaves a row behind.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PersonDistinct::Table)
                    .if_not_exists()
                    .col(uuid(PersonDistinct::Id).primary_key())
                    .col(uuid(PersonDistinct::TreeId))
                    .col(uuid(PersonDistinct::PersonId))
                    .col(uuid(PersonDistinct::OtherPersonId))
                    .col(timestamp_with_time_zone(PersonDistinct::CreatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_distinct_tree")
                            .from(PersonDistinct::Table, PersonDistinct::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_distinct_person")
                            .from(PersonDistinct::Table, PersonDistinct::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_distinct_other_person")
                            .from(PersonDistinct::Table, PersonDistinct::OtherPersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        // Leads with `person_id`, so it also answers "who is this person
        // distinct from" for the lower id of each pair.
        manager
            .create_index(
                Index::create()
                    .name("idx_person_distinct_pair")
                    .table(PersonDistinct::Table)
                    .col(PersonDistinct::PersonId)
                    .col(PersonDistinct::OtherPersonId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_distinct_other_person_id")
                    .table(PersonDistinct::Table)
                    .col(PersonDistinct::OtherPersonId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PersonDistinct::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum PersonDistinct {
    Table,
    Id,
    TreeId,
    PersonId,
    OtherPersonId,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Id,
}
