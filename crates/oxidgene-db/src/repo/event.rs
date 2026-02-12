//! Repository for `Event` entities (CRUD with soft delete, type/person/family filters).

use chrono::{NaiveDate, Utc};
use oxidgene_core::enums::EventType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Event};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::event::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::repo::pagination::{PaginationParams, paginate};

/// Optional filters for listing events.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub event_type: Option<EventType>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
}

/// Repository for event CRUD operations.
pub struct EventRepo;

impl EventRepo {
    /// List events in a tree with optional filters and pagination.
    pub async fn list(
        db: &DatabaseConnection,
        tree_id: Uuid,
        filter: &EventFilter,
        params: &PaginationParams,
    ) -> Result<Connection<Event>, OxidGeneError> {
        let mut query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null());

        if let Some(ref et) = filter.event_type {
            query = query.filter(Column::EventType.eq(sea_enums::EventType::from(*et)));
        }
        if let Some(pid) = filter.person_id {
            query = query.filter(Column::PersonId.eq(pid));
        }
        if let Some(fid) = filter.family_id {
            query = query.filter(Column::FamilyId.eq(fid));
        }

        paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await
    }

    /// List all events in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<Event>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Get a single event by ID (excludes soft-deleted).
    pub async fn get(db: &DatabaseConnection, id: Uuid) -> Result<Event, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "Event",
                id,
            })
    }

    /// Create a new event.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        tree_id: Uuid,
        event_type: EventType,
        date_value: Option<String>,
        date_sort: Option<NaiveDate>,
        place_id: Option<Uuid>,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
        description: Option<String>,
    ) -> Result<Event, OxidGeneError> {
        let now = Utc::now();
        let model = event::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            event_type: Set(sea_enums::EventType::from(event_type)),
            date_value: Set(date_value),
            date_sort: Set(date_sort),
            place_id: Set(place_id),
            person_id: Set(person_id),
            family_id: Set(family_id),
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

    /// Update an existing event.
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        db: &DatabaseConnection,
        id: Uuid,
        event_type: Option<EventType>,
        date_value: Option<Option<String>>,
        date_sort: Option<Option<NaiveDate>>,
        place_id: Option<Option<Uuid>>,
        description: Option<Option<String>>,
    ) -> Result<Event, OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Event",
                id,
            })?;

        let mut active: ActiveModel = existing.into_active_model();
        if let Some(event_type) = event_type {
            active.event_type = Set(sea_enums::EventType::from(event_type));
        }
        if let Some(date_value) = date_value {
            active.date_value = Set(date_value);
        }
        if let Some(date_sort) = date_sort {
            active.date_sort = Set(date_sort);
        }
        if let Some(place_id) = place_id {
            active.place_id = Set(place_id);
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

    /// Soft-delete an event.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = Entity::find_by_id(id)
            .filter(Column::DeletedAt.is_null())
            .one(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .ok_or(OxidGeneError::NotFound {
                entity: "Event",
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

fn into_domain(m: event::Model) -> Event {
    Event {
        id: m.id,
        tree_id: m.tree_id,
        event_type: m.event_type.into(),
        date_value: m.date_value,
        date_sort: m.date_sort,
        place_id: m.place_id,
        person_id: m.person_id,
        family_id: m.family_id,
        description: m.description,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}
