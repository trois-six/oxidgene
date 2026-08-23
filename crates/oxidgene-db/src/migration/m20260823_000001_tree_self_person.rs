//! Store the person who represents the current user in a tree.

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
                    .add_column(ColumnDef::new(Tree::SelfPersonId).uuid().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .drop_column(Tree::SelfPersonId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    SelfPersonId,
}
