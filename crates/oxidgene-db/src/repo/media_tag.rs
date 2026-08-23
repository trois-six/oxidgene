//! Atomic create/delete operations for media tags.

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::media_tag::{self, Column, Entity};

/// Repository for tags belonging to a media row.
pub struct MediaTagRepo;

impl MediaTagRepo {
    /// Return tag rows for the requested media, in creation order.
    pub async fn list_for_media_ids(
        db: &impl ConnectionTrait,
        media_ids: &[Uuid],
    ) -> Result<Vec<media_tag::Model>, OxidGeneError> {
        if media_ids.is_empty() {
            return Ok(Vec::new());
        }
        Entity::find()
            .filter(Column::MediaId.is_in(media_ids.iter().copied()))
            .order_by_asc(Column::CreatedAt)
            .all(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))
    }

    /// Add one tag. The compound primary key makes another editor adding the
    /// same tag a harmless no-op.
    pub async fn create(
        db: &impl ConnectionTrait,
        media_id: Uuid,
        tag: String,
        normalized_tag: String,
    ) -> Result<(), OxidGeneError> {
        let model = media_tag::ActiveModel {
            media_id: Set(media_id),
            normalized_tag: Set(normalized_tag),
            tag: Set(tag),
            created_at: Set(Utc::now()),
        };
        match Entity::insert(model)
            .on_conflict(
                OnConflict::columns([Column::MediaId, Column::NormalizedTag])
                    .do_nothing()
                    .to_owned(),
            )
            .exec(db)
            .await
        {
            Ok(_) | Err(sea_orm::DbErr::RecordNotInserted) => Ok(()),
            Err(error) => Err(OxidGeneError::Database(error.to_string())),
        }
    }

    /// Delete one tag. A duplicate removal is harmless.
    pub async fn delete(
        db: &impl ConnectionTrait,
        media_id: Uuid,
        normalized_tag: &str,
    ) -> Result<(), OxidGeneError> {
        Entity::delete_many()
            .filter(Column::MediaId.eq(media_id))
            .filter(Column::NormalizedTag.eq(normalized_tag))
            .exec(db)
            .await
            .map_err(|error| OxidGeneError::Database(error.to_string()))?;
        Ok(())
    }
}
