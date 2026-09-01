use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BackgroundJob::Table)
                    .add_column(ColumnDef::new(BackgroundJob::TraceParent).string().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(BackgroundJob::Table)
                    .add_column(ColumnDef::new(BackgroundJob::TraceState).string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(BackgroundJob::Table)
                    .drop_column(BackgroundJob::TraceState)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(BackgroundJob::Table)
                    .drop_column(BackgroundJob::TraceParent)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum BackgroundJob {
    Table,
    TraceParent,
    TraceState,
}
