//! Repository for `MediaLink` junction table (create/delete only).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::MediaLink;
use sea_orm::entity::prelude::*;
use sea_orm::{QueryFilter, Set};
use uuid::Uuid;

use crate::entities::media_link::{self, Column, Entity};

/// Flat row for the bulk media-links query.
#[derive(Debug)]
pub struct MediaLinkRow {
    pub entity_id: Uuid,
    pub entity_type: String,
    pub media_id: Uuid,
    pub file_path: String,
    pub file_name: String,
}

/// Repository for media–entity links.
pub struct MediaLinkRepo;

impl MediaLinkRepo {
    /// List all media links for persons belonging to a tree, joining media
    /// to return file path and file name alongside the linked entity.
    pub async fn list_for_tree(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<MediaLinkRow>, OxidGeneError> {
        use sea_orm::DbBackend;
        use sea_orm::Statement;

        // Use backend-appropriate parameter placeholder.
        let backend = db.get_database_backend();
        let (sql, values): (&str, &[sea_orm::Value]) = match backend {
            DbBackend::Sqlite => (
                r#"
                    SELECT ml.person_id, ml.media_id, m.file_path, m.file_name
                    FROM media_link ml
                    INNER JOIN media m ON m.id = ml.media_id
                    INNER JOIN person p ON p.id = ml.person_id
                    WHERE p.tree_id = ?
                      AND p.deleted_at IS NULL
                      AND m.deleted_at IS NULL
                      AND ml.person_id IS NOT NULL
                "#,
                &[tree_id.into()],
            ),
            _ => (
                r#"
                    SELECT ml.person_id, ml.media_id, m.file_path, m.file_name
                    FROM media_link ml
                    INNER JOIN media m ON m.id = ml.media_id
                    INNER JOIN person p ON p.id = ml.person_id
                    WHERE p.tree_id = $1
                      AND p.deleted_at IS NULL
                      AND m.deleted_at IS NULL
                      AND ml.person_id IS NOT NULL
                "#,
                &[tree_id.into()],
            ),
        };

        let stmt = Statement::from_sql_and_values(backend, sql, values.to_vec());

        let query_results = db
            .query_all(stmt)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut rows = Vec::new();
        for row in query_results {
            let person_id: Uuid = row
                .try_get("", "person_id")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            let media_id: Uuid = row
                .try_get("", "media_id")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            let file_path: String = row
                .try_get("", "file_path")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            let file_name: String = row
                .try_get("", "file_name")
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            rows.push(MediaLinkRow {
                entity_id: person_id,
                entity_type: "person".to_string(),
                media_id,
                file_path,
                file_name,
            });
        }
        Ok(rows)
    }

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
