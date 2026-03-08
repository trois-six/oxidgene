//! Migration: add `sosa_root_person_id` column to the `tree` table.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .add_column(ColumnDef::new(Tree::SosaRootPersonId).uuid().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .drop_column(Tree::SosaRootPersonId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    SosaRootPersonId,
}
