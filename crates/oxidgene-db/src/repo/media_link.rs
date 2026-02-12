//! Repository for `MediaLink` junction table (create/delete only).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::MediaLink;
use sea_orm::entity::prelude::*;
use sea_orm::{QueryFilter, Set};
use uuid::Uuid;

use crate::entities::media_link::{self, Column, Entity};

/// Repository for media–entity links.
pub struct MediaLinkRepo;

impl MediaLinkRepo {
    /// List links for a given media item.
    pub async fn list_by_media(
        db: &DatabaseConnection,
        media_id: Uuid,
    ) -> Result<Vec<MediaLink>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::MediaId.eq(media_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List links for multiple media items.
    pub async fn list_by_medias(
        db: &DatabaseConnection,
        media_ids: &[Uuid],
    ) -> Result<Vec<MediaLink>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::MediaId.is_in(media_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create a media link.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        media_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        source_id: Option<Uuid>,
        family_id: Option<Uuid>,
        sort_order: i32,
    ) -> Result<MediaLink, OxidGeneError> {
        let model = media_link::ActiveModel {
            id: Set(id),
            media_id: Set(media_id),
            person_id: Set(person_id),
            event_id: Set(event_id),
            source_id: Set(source_id),
            family_id: Set(family_id),
            sort_order: Set(sort_order),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a media link.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "MediaLink",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: media_link::Model) -> MediaLink {
    MediaLink {
        id: m.id,
        media_id: m.media_id,
        person_id: m.person_id,
        event_id: m.event_id,
        source_id: m.source_id,
        family_id: m.family_id,
        sort_order: m.sort_order,
    }
}
