//! Repository for `Media` entities (CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Media};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::media::{self, ActiveModel, Column, Entity};
use crate::repo::pagination::{PaginationParams, paginate};

/// Repository for media CRUD operations.
pub struct MediaRepo;

impl MediaRepo {
    /// List media in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &DatabaseConnection,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Media>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// Get a single media by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Media, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })
    }

    /// Create a new media record.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        file_name: String,
        mime_type: String,
        file_path: String,
        file_size: i64,
        title: Option<String>,
        description: Option<String>,
    ) -> Result<Media, OxidGeneError> {
        let now = Utc::now();
        let model = media::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            file_name: Set(file_name),
            mime_type: Set(mime_type),
            file_path: Set(file_path),
            file_size: Set(file_size),
            title: Set(title),
            description: Set(description),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a media record.
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        title: Option<Option<String>>,
        description: Option<Option<String>>,
    ) -> Result<Media, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(title) = title {
            active.title = Set(title);
        }
        if let Some(description) = description {
            active.description = Set(description);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Soft-delete a media record.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Media",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(())
    }
}

fn into_domain(m: media::Model) -> Media {
    Media {
        id: m.id,
        tree_id: m.tree_id,
        file_name: m.file_name,
        mime_type: m.mime_type,
        file_path: m.file_path,
        file_size: m.file_size,
        title: m.title,
        description: m.description,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
