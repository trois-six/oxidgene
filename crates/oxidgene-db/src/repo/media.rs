//! Repository for `Media` entities (CRUD with soft delete).

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Media};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::media::{self, ActiveModel, Column, Entity};
use crate::repo::pagination::{PaginationParams, paginate};

/// A file whose bytes are already in the media store, ready to be recorded.
#[derive(Debug, Clone)]
pub struct UploadedMedia {
    pub file_name: String,
    pub mime_type: String,
    pub storage_key: String,
    pub sha256: String,
    pub file_size: i64,
    pub thumbnail_key: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub page_count: i32,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Repository for media CRUD operations.
pub struct MediaRepo;

impl MediaRepo {
    /// List media in a tree with pagination (excludes soft-deleted).
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Media>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());
        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all media in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Media>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get multiple media items by ID (excludes soft-deleted).
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Media>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single media by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Media, OxidGeneError> {
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

    /// Create a media record that names a file without holding its bytes.
    ///
    /// This is the GEDCOM-import and metadata-only path: `file_path` is
    /// whatever the source said, and `storage_key` stays null until the file
    /// itself arrives. Use [`MediaRepo::create_uploaded`] when there are bytes.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &impl ConnectionTrait,
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
            storage_key: Set(None),
            sha256: Set(None),
            thumbnail_key: Set(None),
            width: Set(None),
            height: Set(None),
            page_count: Set(1),
            file_size: Set(file_size),
            title: Set(title),
            description: Set(description),
            date_value: Set(None),
            date_sort: Set(None),
            place_id: Set(None),
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

    /// Everything a media row records about a file whose bytes we hold.
    ///
    /// Grouped into a struct because passing eleven positional arguments —
    /// four of them `Option<i32>` — is a call site nobody can read.
    pub async fn create_uploaded(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        upload: UploadedMedia,
    ) -> Result<Media, OxidGeneError> {
        let now = Utc::now();
        let model = media::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            // An uploaded file has no foreign path to preserve, so `file_path`
            // carries the name a GEDCOM export should write out.
            file_path: Set(upload.file_name.clone()),
            file_name: Set(upload.file_name),
            mime_type: Set(upload.mime_type),
            storage_key: Set(Some(upload.storage_key)),
            sha256: Set(Some(upload.sha256)),
            thumbnail_key: Set(upload.thumbnail_key),
            width: Set(upload.width),
            height: Set(upload.height),
            page_count: Set(upload.page_count),
            file_size: Set(upload.file_size),
            title: Set(upload.title),
            description: Set(upload.description),
            date_value: Set(None),
            date_sort: Set(None),
            place_id: Set(None),
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

    /// Attach stored bytes to a record that had none.
    ///
    /// The path a GEDCOM import left in `file_path` is kept: it is what the
    /// export has to write back, and now it also documents where the file came
    /// from before we had a copy.
    pub async fn attach_file(
        db: &impl ConnectionTrait,
        id: Uuid,
        upload: UploadedMedia,
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
        active.mime_type = Set(upload.mime_type);
        active.storage_key = Set(Some(upload.storage_key));
        active.sha256 = Set(Some(upload.sha256));
        active.thumbnail_key = Set(upload.thumbnail_key);
        active.width = Set(upload.width);
        active.height = Set(upload.height);
        active.page_count = Set(upload.page_count);
        active.file_size = Set(upload.file_size);
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Find a tree's media record for a given content digest, if one exists.
    ///
    /// Lets an import that re-runs over an archive skip files it has already
    /// ingested, without comparing bytes it would have to fetch first.
    pub async fn find_by_sha256(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        sha256: &str,
    ) -> Result<Option<Media>, OxidGeneError> {
        let model = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::Sha256.eq(sha256))
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(model.map(into_domain))
    }

    /// Update a media record.
    pub async fn update(
        db: &impl ConnectionTrait,
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
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
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
        storage_key: m.storage_key,
        sha256: m.sha256,
        thumbnail_key: m.thumbnail_key,
        width: m.width,
        height: m.height,
        page_count: m.page_count,
        file_size: m.file_size,
        title: m.title,
        description: m.description,
        date_value: m.date_value,
        date_sort: m.date_sort,
        place_id: m.place_id,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
