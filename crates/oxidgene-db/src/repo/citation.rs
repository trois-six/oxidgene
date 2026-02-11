//! Repository for `Citation` entities (CRUD, no soft delete).

use chrono::Utc;
use oxidgene_core::enums::Confidence;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Citation;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::citation::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;

/// Repository for citation operations.
pub struct CitationRepo;

impl CitationRepo {
    /// List citations for a given source.
    pub async fn list_by_source(
        db: &DatabaseConnection,
        source_id: Uuid,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::SourceId.eq(source_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single citation by ID.
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Citation, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            })
    }

    /// Create a new citation.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        source_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        page: Option<String>,
        confidence: Confidence,
        text: Option<String>,
    ) -> Result<Citation, OxidGeneError> {
        let now = Utc::now();
        let model = citation::ActiveModel {
            id: Set(id),
            source_id: Set(source_id),
            person_id: Set(person_id),
            event_id: Set(event_id),
            family_id: Set(family_id),
            page: Set(page),
            confidence: Set(sea_enums::Confidence::from(confidence)),
            text: Set(text),
            created_at: Set(now),
            updated_at: Set(now),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Update a citation.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        page: Option<Option<String>>,
        confidence: Option<Confidence>,
        text: Option<Option<String>>,
    ) -> Result<Citation, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(page) = page {
            active.page = Set(page);
        }
        if let Some(confidence) = confidence {
            active.confidence = Set(sea_enums::Confidence::from(confidence));
        }
        if let Some(text) = text {
            active.text = Set(text);
        }
        active.updated_at = Set(Utc::now());

        let result = active
            .update(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete a citation.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Citation",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: citation::Model) -> Citation {
    Citation {
        id: m.id,
        source_id: m.source_id,
        person_id: m.person_id,
        event_id: m.event_id,
        family_id: m.family_id,
        page: m.page,
        confidence: m.confidence.into(),
        text: m.text,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
