//! Repository for `Citation` entities (CRUD, no soft delete).

use chrono::Utc;
use oxidgene_core::enums::Confidence;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Citation, Connection};
use sea_orm::entity::prelude::*;
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, IntoActiveModel, JoinType, QueryFilter, QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::citation::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::entities::source;
use crate::repo::pagination::{PaginationParams, paginate};

/// Optional entity filters for listing citations.
#[derive(Debug, Clone, Default)]
pub struct CitationFilter {
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
}

/// Repository for citation operations.
pub struct CitationRepo;

impl CitationRepo {
    /// List citations in a tree with optional entity filters and pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: &CitationFilter,
        params: &PaginationParams,
    ) -> Result<Connection<Citation>, OxidGeneError> {
        let mut query = Entity::find()
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null());

        if let Some(person_id) = filter.person_id {
            query = query.filter(Column::PersonId.eq(person_id));
        }
        if let Some(event_id) = filter.event_id {
            query = query.filter(Column::EventId.eq(event_id));
        }
        if let Some(family_id) = filter.family_id {
            query = query.filter(Column::FamilyId.eq(family_id));
        }
        if let Some(source_id) = filter.source_id {
            query = query.filter(Column::SourceId.eq(source_id));
        }

        paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
    }

    /// List all citations belonging to sources in one tree.
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .join(JoinType::InnerJoin, citation::Relation::Source.def())
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List citations directly linked to one person.
    pub async fn list_by_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List citations for a given source.
    pub async fn list_by_source(
        db: &impl ConnectionTrait,
        source_id: Uuid,
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::SourceId.eq(source_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List citations for multiple sources.
    pub async fn list_by_sources(
        db: &impl ConnectionTrait,
        source_ids: &[Uuid],
    ) -> Result<Vec<Citation>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::SourceId.is_in(source_ids.iter().copied()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single citation by ID.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Citation, OxidGeneError> {
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
        db: &impl ConnectionTrait,
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
    /// Updates a citation in place. `source_id` can be repointed at another
    /// source: a citation carries which record backs a fact, and correcting
    /// that record is an edit of the same citation — deleting and recreating
    /// it instead would strand the row every reference to it points at.
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        source_id: Option<Uuid>,
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
        if let Some(source_id) = source_id {
            active.source_id = Set(source_id);
        }
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
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
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
