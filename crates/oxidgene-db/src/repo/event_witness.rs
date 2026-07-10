//! Repository for `EventWitness` junction table (create/list/delete only).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::EventWitness;
use sea_orm::entity::prelude::*;
use sea_orm::{QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::event_witness::{self, Column, Entity};

/// Repository for event–witness links.
pub struct EventWitnessRepo;

impl EventWitnessRepo {
    /// List witnesses on an event, ordered by `sort_order`.
    pub async fn list_by_event(
        db: &DatabaseConnection,
        event_id: Uuid,
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::EventId.eq(event_id))
            .order_by_asc(Column::SortOrder)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List witnesses for multiple events, ordered by `sort_order`.
    pub async fn list_by_events(
        db: &DatabaseConnection,
        event_ids: &[Uuid],
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::EventId.is_in(event_ids.iter().copied()))
            .order_by_asc(Column::SortOrder)
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create an event–witness link.
    pub async fn create(
        db: &DatabaseConnection,
        id: Uuid,
        event_id: Uuid,
        person_id: Uuid,
        relation: Option<String>,
        sort_order: i32,
    ) -> Result<EventWitness, OxidGeneError> {
        let model = event_witness::ActiveModel {
            id: Set(id),
            event_id: Set(event_id),
            person_id: Set(person_id),
            relation: Set(relation),
            sort_order: Set(sort_order),
        };
        let result = model
            .insert(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        Ok(into_domain(result))
    }

    /// Hard-delete an event–witness link.
    pub async fn delete(db: &DatabaseConnection, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id)
            .exec(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "EventWitness",
                id,
            });
        }
        Ok(())
    }
}

fn into_domain(m: event_witness::Model) -> EventWitness {
    EventWitness {
        id: m.id,
        event_id: m.event_id,
        person_id: m.person_id,
        relation: m.relation,
        sort_order: m.sort_order,
    }
}
