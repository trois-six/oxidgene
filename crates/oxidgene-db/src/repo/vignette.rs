//! Repository for `Vignette` entities — crop rectangles on a media file.
//!
//! No soft delete here. A vignette is a coordinate annotation, not a record
//! anyone cites; deleting one throws away four integers, and keeping tombstones
//! for it would only make "which crops are on this page" a filtered query.

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Vignette;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::person;
use crate::entities::vignette::{self, ActiveModel, Column, Entity};

/// The rectangle and attribution a vignette records.
#[derive(Debug, Clone)]
pub struct VignetteInput {
    pub media_id: Uuid,
    pub page: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
}

/// Fields a caller may change on an existing vignette.
///
/// Every field is a `Some`-means-change patch: re-cropping moves the
/// rectangle, and clearing an attribution sends `Some(None)`.
#[derive(Debug, Clone, Default)]
pub struct VignettePatch {
    pub page: Option<i32>,
    pub rect: Option<(i32, i32, i32, i32)>,
    pub person_id: Option<Option<Uuid>>,
    pub event_id: Option<Option<Uuid>>,
}

/// Repository for vignette CRUD operations.
pub struct VignetteRepo;

impl VignetteRepo {
    /// Every vignette on a set of media files.
    pub async fn list_for_medias(
        db: &impl ConnectionTrait,
        media_ids: &[Uuid],
    ) -> Result<Vec<Vignette>, OxidGeneError> {
        if media_ids.is_empty() {
            return Ok(Vec::new());
        }
        let models = Entity::find()
            .filter(Column::MediaId.is_in(media_ids.to_vec()))
            .order_by_asc(Column::MediaId)
            .order_by_asc(Column::Page)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Every vignette on a media file, oldest first.
    pub async fn list_for_media(
        db: &impl ConnectionTrait,
        media_id: Uuid,
    ) -> Result<Vec<Vignette>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::MediaId.eq(media_id))
            // Page order first so a document reads front to back; `id` is a
            // UUID v7, so within a page it breaks ties by creation time.
            .order_by_asc(Column::Page)
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Every vignette attributed to a person, across all media.
    pub async fn list_for_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<Vignette>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Every vignette standing as evidence for an event.
    pub async fn list_for_event(
        db: &impl ConnectionTrait,
        event_id: Uuid,
    ) -> Result<Vec<Vignette>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::EventId.eq(event_id))
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a vignette by ID.
    /// Several by id, in one query. Missing ids are simply absent from the
    /// result — a portrait pointing at a deleted crop is "no portrait", not an
    /// error a reader can act on.
    pub async fn get_many(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Vignette>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let models = Entity::find()
            .filter(Column::Id.is_in(ids.to_vec()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Vignette, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Vignette",
                id,
            })
    }

    /// Create a vignette.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        input: VignetteInput,
    ) -> Result<Vignette, OxidGeneError> {
        let now = Utc::now();
        let model = vignette::ActiveModel {
            id: Set(id),
            media_id: Set(input.media_id),
            page: Set(input.page),
            x: Set(input.x),
            y: Set(input.y),
            width: Set(input.width),
            height: Set(input.height),
            person_id: Set(input.person_id),
            event_id: Set(input.event_id),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Apply a patch to an existing vignette.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        patch: VignettePatch,
    ) -> Result<Vignette, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Vignette",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(page) = patch.page {
            active.page = Set(page);
        }
        if let Some((x, y, width, height)) = patch.rect {
            active.x = Set(x);
            active.y = Set(y);
            active.width = Set(width);
            active.height = Set(height);
        }
        if let Some(person_id) = patch.person_id {
            active.person_id = Set(person_id);
        }
        if let Some(event_id) = patch.event_id {
            active.event_id = Set(event_id);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Delete a vignette outright.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        person::Entity::update_many()
            .col_expr(
                person::Column::PortraitVignetteId,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(person::Column::PortraitVignetteId.eq(id))
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Vignette",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(v: vignette::Model) -> Vignette {
    Vignette {
        id: v.id,
        media_id: v.media_id,
        page: v.page,
        x: v.x,
        y: v.y,
        width: v.width,
        height: v.height,
        person_id: v.person_id,
        event_id: v.event_id,
        created_at: v.created_at,
        updated_at: v.updated_at,
    }
}
