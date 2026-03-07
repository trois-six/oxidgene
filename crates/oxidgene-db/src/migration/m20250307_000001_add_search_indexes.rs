//! Add indexes on `person_name.surname` and `person_name.given_names` to
//! support efficient server-side person search.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Index on surname for prefix/contains search.
        manager
            .create_index(
                Index::create()
                    .name("idx_person_name_surname")
                    .table(PersonName::Table)
                    .col(PersonName::Surname)
                    .to_owned(),
            )
            .await?;

        // Index on given_names for prefix/contains search.
        manager
            .create_index(
                Index::create()
                    .name("idx_person_name_given_names")
                    .table(PersonName::Table)
                    .col(PersonName::GivenNames)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_person_name_surname")
                    .table(PersonName::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_person_name_given_names")
                    .table(PersonName::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Iden)]
enum PersonName {
    Table,
    Surname,
    GivenNames,
}
