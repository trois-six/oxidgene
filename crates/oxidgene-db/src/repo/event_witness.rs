//! Repository for `EventWitness` junction table (create/list/delete only).

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::EventWitness;
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::event_witness::{self, Column, Entity};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;

/// Repository for event–witness links.
pub struct EventWitnessRepo;

impl EventWitnessRepo {
    /// List witnesses on an event, ordered by `sort_order`.
    pub async fn list_by_event(
        db: &impl ConnectionTrait,
        event_id: Uuid,
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::EventId.eq(event_id))
            .order_by_asc(Column::SortOrder)
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List witnesses for multiple events, ordered by `sort_order`.
    pub async fn list_by_events(
        db: &impl ConnectionTrait,
        event_ids: &[Uuid],
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        in_chunks(event_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::EventId.is_in(chunk))
                .order_by_asc(Column::SortOrder)
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Get one event–witness link.
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<EventWitness, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "EventWitness",
                id,
            })
    }

    /// Every witness link of a tree's events, in one query: the tree's
    /// events by their `(tree_id, id)` index, then their witnesses by theirs.
    pub async fn list_by_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        use crate::entities::event;
        let models = Entity::find()
            .inner_join(event::Entity)
            .filter(event::Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// List the events a person witnesses.
    pub async fn list_by_person(
        db: &impl ConnectionTrait,
        person_id: Uuid,
    ) -> Result<Vec<EventWitness>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::PersonId.eq(person_id))
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(models.into_iter().map(into_domain).collect())
    }

    /// Create an event–witness link. A person witnesses an event once per
    /// relation — a godparent may also be a witness of the same baptism — and
    /// a missing relation counts as one: the same link again is a conflict.
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        event_id: Uuid,
        person_id: Uuid,
        relation: Option<String>,
        sort_order: i32,
    ) -> Result<EventWitness, OxidGeneError> {
        let same_relation = match relation.as_deref() {
            None | Some("") => Condition::any()
                .add(Column::Relation.is_null())
                .add(Column::Relation.eq("")),
            Some(relation) => Condition::all().add(Column::Relation.eq(relation)),
        };
        let already = Entity::find()
            .filter(Column::EventId.eq(event_id))
            .filter(Column::PersonId.eq(person_id))
            .filter(same_relation)
            .one(db)
            .await
            .map_err(db_err)?;
        if already.is_some() {
            return Err(OxidGeneError::Conflict(
                "the person already witnesses the event in this relation".to_string(),
            ));
        }
        let tree_id = crate::repo::tree_of::<crate::entities::event::Entity>(
            db,
            event_id,
            crate::entities::event::Column::TreeId,
            "Event",
        )
        .await?;
        let model = event_witness::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            event_id: Set(event_id),
            person_id: Set(person_id),
            relation: Set(relation),
            sort_order: Set(sort_order),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        Ok(into_domain(result))
    }

    /// Hard-delete an event–witness link.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let result = Entity::delete_by_id(id).exec(db).await.map_err(db_err)?;
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
