//! Repository for `MediaLink` junction table (create/delete only).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Media, MediaLink};
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::media_link::{self, Column, Entity};

/// Which of a media link's four nullable targets to match on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaLinkTarget {
    Person,
    Family,
    Event,
    Source,
}

impl MediaLinkTarget {
    /// Parse the wire spelling used by REST query parameters and GraphQL.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "person" => Some(Self::Person),
            "family" => Some(Self::Family),
            "event" => Some(Self::Event),
            "source" => Some(Self::Source),
            _ => None,
        }
    }
}

/// Flat row for the bulk media-links query.
#[derive(Debug)]
pub struct MediaLinkRow {
    pub link_id: Uuid,
    pub entity_id: Uuid,
    /// `person` or `event` — which of the link's targets this row is about.
    pub entity_type: String,
    pub media_id: Uuid,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: String,
    /// Whether a thumbnail was generated for this media.
    pub has_thumbnail: bool,
}

/// Repository for media–entity links.
pub struct MediaLinkRepo;

impl MediaLinkRepo {
    /// Every media link in a tree, flat, with the media's display fields.
    ///
    /// Two shapes in one query: person links, which the pedigree canvas reads
    /// to put a portrait on each card, and event links, which the profile
    /// timeline reads to show what documents each event. Both callers filter
    /// on `entity_type`, and doing it in one round trip is what keeps a
    /// timeline of forty events from being forty requests.
    pub async fn list_for_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<MediaLinkRow>, OxidGeneError> {
        use sea_orm::DbBackend;
        use sea_orm::Statement;

        let backend = db.get_database_backend();
        // SQLite and PostgreSQL disagree only on the placeholder; the query is
        // otherwise identical, so it is written once and the marker swapped.
        let placeholders: [&str; 2] = match backend {
            DbBackend::Sqlite => ["?", "?"],
            _ => ["$1", "$2"],
        };
        let sql = format!(
            r#"
                SELECT ml.id AS link_id,
                       ml.person_id AS entity_id,
                       'person' AS entity_type,
                       ml.media_id,
                       m.file_path, m.file_name, m.mime_type, m.thumbnail_key
                FROM media_link ml
                INNER JOIN media m ON m.id = ml.media_id
                INNER JOIN person p ON p.id = ml.person_id
                WHERE p.tree_id = {}
                  AND p.deleted_at IS NULL
                  AND m.deleted_at IS NULL
                  AND ml.person_id IS NOT NULL
                UNION ALL
                SELECT ml.id AS link_id,
                       ml.event_id AS entity_id,
                       'event' AS entity_type,
                       ml.media_id,
                       m.file_path, m.file_name, m.mime_type, m.thumbnail_key
                FROM media_link ml
                INNER JOIN media m ON m.id = ml.media_id
                INNER JOIN event e ON e.id = ml.event_id
                WHERE e.tree_id = {}
                  AND e.deleted_at IS NULL
                  AND m.deleted_at IS NULL
                  AND ml.event_id IS NOT NULL
            "#,
            placeholders[0], placeholders[1]
        );

        let stmt =
            Statement::from_sql_and_values(backend, &sql, vec![tree_id.into(), tree_id.into()]);

        let query_results = db
            .query_all(stmt)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut rows = Vec::with_capacity(query_results.len());
        for row in query_results {
            let get_string = |name: &str| -> Result<String, OxidGeneError> {
                row.try_get("", name)
                    .map_err(|e| OxidGeneError::Database(e.to_string()))
            };
            rows.push(MediaLinkRow {
                link_id: row
                    .try_get("", "link_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                entity_id: row
                    .try_get("", "entity_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                entity_type: get_string("entity_type")?,
                media_id: row
                    .try_get("", "media_id")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?,
                file_path: get_string("file_path")?,
                file_name: get_string("file_name")?,
                mime_type: get_string("mime_type")?,
                // Absent means the server could not rasterise this file, which
                // is what the caller branches on to draw an icon instead.
                has_thumbnail: row
                    .try_get::<Option<String>>("", "thumbnail_key")
                    .map_err(|e| OxidGeneError::Database(e.to_string()))?
                    .is_some(),
            });
        }
        Ok(rows)
    }

    /// List links for a given media item.
    pub async fn list_by_media(
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
        media_ids: &[Uuid],
    ) -> Result<Vec<MediaLink>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::MediaId.is_in(media_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List all media links attached to a person.
    pub async fn list_by_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<MediaLink>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Every media attached to one entity, with the media itself.
    ///
    /// The gallery needs the link (for its id, order and profile flag) *and*
    /// the media (for its MIME type, title and whether a thumbnail exists) at
    /// once; fetching them separately would be two round trips to render one
    /// grid. `entity` is the column to match — `person_id`, `family_id`,
    /// `event_id` or `source_id`.
    pub async fn list_with_media(
        db: &impl ConnectionTrait,
        entity: MediaLinkTarget,
        entity_id: Uuid,
    ) -> Result<Vec<(MediaLink, Media)>, OxidGeneError> {
        let column = match entity {
            MediaLinkTarget::Person => Column::PersonId,
            MediaLinkTarget::Family => Column::FamilyId,
            MediaLinkTarget::Event => Column::EventId,
            MediaLinkTarget::Source => Column::SourceId,
        };
        let rows = Entity::find()
            .filter(column.eq(entity_id))
            .find_also_related(crate::entities::media::Entity)
            .order_by_asc(Column::SortOrder)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|(link, media)| {
                // A soft-deleted media keeps its links; the gallery should not
                // show it, and dropping it here beats every caller remembering.
                let media = media.filter(|m| m.deleted_at.is_none())?;
                Some((into_domain(link), crate::repo::media::into_domain(media)))
            })
            .collect())
    }

    /// Create a media link.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &impl ConnectionTrait,
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
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
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
