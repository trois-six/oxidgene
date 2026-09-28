//! Let a tree turn off the suggestions its entry fields make.
//!
//! `tree.entry_suggestions` starts on, for existing trees as for new ones.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .add_column(boolean(Tree::EntrySuggestions).default(true))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .drop_column(Tree::EntrySuggestions)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    EntrySuggestions,
}
