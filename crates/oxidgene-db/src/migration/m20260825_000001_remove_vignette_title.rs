//! Remove the unused vignette label.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Vignette::Table)
                    .drop_column(Vignette::Title)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Vignette::Table)
                    .add_column(ColumnDef::new(Vignette::Title).string().null())
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Vignette {
    Table,
    Title,
}
