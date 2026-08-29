//! Durable import and export jobs shared by web workers and the desktop backend.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(BackgroundJob::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BackgroundJob::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BackgroundJob::TreeId).uuid().not_null())
                    // Kept equal to tree_id while active and cleared at the terminal
                    // transition. UNIQUE therefore permits many historical NULLs but
                    // only one queued or running operation per tree.
                    .col(ColumnDef::new(BackgroundJob::ActiveTreeId).uuid().null())
                    .col(
                        ColumnDef::new(BackgroundJob::Kind)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Format)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Status)
                            .string_len(16)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Phase)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(BackgroundJob::SourceKey).string().null())
                    .col(ColumnDef::new(BackgroundJob::ArtifactKey).string().null())
                    .col(
                        ColumnDef::new(BackgroundJob::OriginalFilename)
                            .string()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::MergeOccupations)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::MergeNames)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Done)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Total)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::Attempt)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(BackgroundJob::LeaseOwner).string().null())
                    .col(
                        ColumnDef::new(BackgroundJob::LeaseUntil)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::CancelRequested)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(BackgroundJob::ResultJson).text().null())
                    .col(ColumnDef::new(BackgroundJob::ErrorCode).string().null())
                    .col(
                        ColumnDef::new(BackgroundJob::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::StartedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(BackgroundJob::FinishedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_background_job_tree")
                            .from(BackgroundJob::Table, BackgroundJob::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_background_job_active_tree")
                            .from(BackgroundJob::Table, BackgroundJob::ActiveTreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_background_job_active_tree")
                    .table(BackgroundJob::Table)
                    .col(BackgroundJob::ActiveTreeId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_background_job_claim")
                    .table(BackgroundJob::Table)
                    .col(BackgroundJob::Status)
                    .col(BackgroundJob::LeaseUntil)
                    .col(BackgroundJob::CreatedAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(BackgroundJob::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum BackgroundJob {
    Table,
    Id,
    TreeId,
    ActiveTreeId,
    Kind,
    Format,
    Status,
    Phase,
    SourceKey,
    ArtifactKey,
    OriginalFilename,
    MergeOccupations,
    MergeNames,
    Done,
    Total,
    Attempt,
    LeaseOwner,
    LeaseUntil,
    CancelRequested,
    ResultJson,
    ErrorCode,
    CreatedAt,
    UpdatedAt,
    StartedAt,
    FinishedAt,
}
