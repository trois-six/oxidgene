//! Key media tags by the application's one folding.
//!
//! A tag's `normalized_tag` was its lowercase spelling, so "Église" and
//! "Eglise" were two tags while search, the dictionaries and the interface
//! read them as one word. Keys are now `oxidgene_core::search::fold_words`:
//! every row is re-keyed, and where two tags of one media fold to the same
//! key the first created is kept, with its spelling. `down` re-keys by the
//! lowercase spelling again; tags merged on the way up stay merged.

use std::collections::HashSet;

use oxidgene_core::search::fold_words;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set,
};

use crate::entities::media_tag;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rekey(manager.get_connection(), fold_words).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rekey(manager.get_connection(), str::to_lowercase).await
    }
}

/// Re-keys every tag by `key` of its spelling, the earliest of each media's
/// tags sharing a key kept. Deletes run before inserts, so a new key never
/// meets the old row it replaces.
pub async fn rekey(db: &impl ConnectionTrait, key: fn(&str) -> String) -> Result<(), DbErr> {
    let rows = media_tag::Entity::find()
        .order_by_asc(media_tag::Column::CreatedAt)
        .order_by_asc(media_tag::Column::MediaId)
        .all(db)
        .await?;
    let mut seen = HashSet::new();
    let mut rekeyed = Vec::new();
    for row in rows {
        let new_key = key(row.tag.trim());
        let kept = !new_key.is_empty() && seen.insert((row.media_id, new_key.clone()));
        if kept && new_key == row.normalized_tag {
            continue;
        }
        media_tag::Entity::delete_many()
            .filter(media_tag::Column::MediaId.eq(row.media_id))
            .filter(media_tag::Column::NormalizedTag.eq(row.normalized_tag.clone()))
            .exec(db)
            .await?;
        if kept {
            rekeyed.push(media_tag::Model {
                normalized_tag: new_key,
                ..row
            });
        }
    }
    for row in rekeyed {
        media_tag::ActiveModel {
            media_id: Set(row.media_id),
            normalized_tag: Set(row.normalized_tag),
            tag: Set(row.tag),
            created_at: Set(row.created_at),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}
