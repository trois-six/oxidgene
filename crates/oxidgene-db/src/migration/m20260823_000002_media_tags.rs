//! Legacy JSON storage for free-form media tags.
//!
//! The JSON array lives on the media row. Pages never inherit a copy: a
//! multi-page document has one set of tags by construction, so changing it
//! cannot leave page metadata inconsistent. The following migration moves the
//! values into `media_tag` rows for independent edits.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(
                        ColumnDef::new(Media::Tags)
                            .text()
                            .not_null()
                            .default("[]")
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .drop_column(Media::Tags)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Tags,
}
