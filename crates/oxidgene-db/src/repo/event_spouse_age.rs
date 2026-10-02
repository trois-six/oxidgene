//! Repository for the ages a family event's record gives for its spouses
//! (GEDCOM `HUSB.AGE` / `WIFE.AGE`).
//!
//! A row is keyed by the spouse's membership of the family (`family_spouse`),
//! not by the person: a merge that moves the membership to the kept person
//! keeps the age with it. Reads and writes speak of persons.

use std::collections::{HashMap, HashSet};

use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Event, SpouseAge};
use sea_orm::entity::prelude::*;
use sea_orm::{ConnectionTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::event_spouse_age::{self, Column, Entity};
use crate::entities::family_spouse;
use crate::repo::batch::in_chunks;
use crate::repo::db_err;

/// Repository for family events' spouse ages.
pub struct EventSpouseAgeRepo;

impl EventSpouseAgeRepo {
    /// The spouse ages of each of `event_ids`, in the order of the family's
    /// spouses.
    pub async fn by_events(
        db: &impl ConnectionTrait,
        event_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<SpouseAge>>, OxidGeneError> {
        let rows: Vec<(event_spouse_age::Model, Option<family_spouse::Model>)> =
            in_chunks(event_ids, |chunk| async move {
                Entity::find()
                    .filter(Column::EventId.is_in(chunk))
                    .find_also_related(family_spouse::Entity)
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?;
        let mut by_event: HashMap<Uuid, Vec<(i32, SpouseAge)>> = HashMap::new();
        for (row, spouse) in rows {
            let Some(spouse) = spouse else { continue };
            by_event.entry(row.event_id).or_default().push((
                spouse.sort_order,
                SpouseAge {
                    person_id: spouse.person_id,
                    age: row.age,
                },
            ));
        }
        Ok(by_event
            .into_iter()
            .map(|(event_id, mut ages)| {
                ages.sort_by_key(|(order, age)| (*order, age.person_id));
                (event_id, ages.into_iter().map(|(_, age)| age).collect())
            })
            .collect())
    }

    /// `events` with the spouse ages of the family events among them.
    pub async fn attach(
        db: &impl ConnectionTrait,
        mut events: Vec<Event>,
    ) -> Result<Vec<Event>, OxidGeneError> {
        let family_events: Vec<Uuid> = events
            .iter()
            .filter(|e| e.family_id.is_some())
            .map(|e| e.id)
            .collect();
        if family_events.is_empty() {
            return Ok(events);
        }
        let mut ages = Self::by_events(db, &family_events).await?;
        for event in &mut events {
            if let Some(found) = ages.remove(&event.id) {
                event.spouse_ages = found;
            }
        }
        Ok(events)
    }

    /// Replaces the spouse ages of family event `event_id` of `family_id`
    /// with `ages`, whose ages must already be canonical. A person who is
    /// not a spouse of the family, or named twice, is a validation error.
    pub async fn replace(
        db: &impl ConnectionTrait,
        event_id: Uuid,
        family_id: Uuid,
        ages: &[SpouseAge],
    ) -> Result<(), OxidGeneError> {
        let memberships: HashMap<Uuid, Uuid> = family_spouse::Entity::find()
            .filter(family_spouse::Column::FamilyId.eq(family_id))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|s| (s.person_id, s.id))
            .collect();
        let mut seen = HashSet::new();
        let mut rows = Vec::with_capacity(ages.len());
        for age in ages {
            let Some(&family_spouse_id) = memberships.get(&age.person_id) else {
                return Err(OxidGeneError::Validation(format!(
                    "spouse_ages: {} is not a spouse of the event's family",
                    age.person_id
                )));
            };
            if !seen.insert(age.person_id) {
                return Err(OxidGeneError::Validation(format!(
                    "spouse_ages: {} is named twice",
                    age.person_id
                )));
            }
            rows.push(event_spouse_age::ActiveModel {
                id: Set(Uuid::now_v7()),
                event_id: Set(event_id),
                family_spouse_id: Set(family_spouse_id),
                age: Set(age.age.clone()),
            });
        }
        Entity::delete_many()
            .filter(Column::EventId.eq(event_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        if !rows.is_empty() {
            Entity::insert_many(rows).exec(db).await.map_err(db_err)?;
        }
        Ok(())
    }
}
