//! Move media tags from a JSON value into independently mutable rows.

use sea_orm::{DbBackend, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(MediaTag::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MediaTag::MediaId).uuid().not_null())
                    .col(ColumnDef::new(MediaTag::NormalizedTag).string().not_null())
                    .col(ColumnDef::new(MediaTag::Tag).string().not_null())
                    .col(
                        ColumnDef::new(MediaTag::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(MediaTag::MediaId)
                            .col(MediaTag::NormalizedTag),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_tag_media")
                            .from(MediaTag::Table, MediaTag::MediaId)
                            .to(Media::Table, Media::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        let backend = manager.get_database_backend();
        let sql = match backend {
            DbBackend::Sqlite => {
                r#"
                INSERT OR IGNORE INTO media_tag (media_id, normalized_tag, tag, created_at)
                SELECT media.id, lower(json_each.value), json_each.value, media.updated_at
                FROM media, json_each(media.tags)
                WHERE trim(json_each.value) <> ''
            "#
            }
            DbBackend::Postgres => {
                r#"
                INSERT INTO media_tag (media_id, normalized_tag, tag, created_at)
                SELECT media.id, lower(value), value, media.updated_at
                FROM media CROSS JOIN LATERAL jsonb_array_elements_text(media.tags::jsonb) AS value
                WHERE btrim(value) <> ''
                ON CONFLICT (media_id, normalized_tag) DO NOTHING
            "#
            }
            _ => return Ok(()),
        };
        manager
            .get_connection()
            .execute_raw(Statement::from_string(backend, sql.to_string()))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MediaTag::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum MediaTag {
    Table,
    MediaId,
    NormalizedTag,
    Tag,
    CreatedAt,
}
