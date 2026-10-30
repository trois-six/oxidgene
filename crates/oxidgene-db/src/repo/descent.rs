//! The rows the descent of a family name is built from
//! (`docs/ui-dictionary.md` §7.2): its carriers, the families they are a
//! spouse or a child of, the members of those families, and the dates the
//! lines show.
//!
//! A fixed number of grouped reads, whatever the size of the name: each one
//! takes the ids the previous ones found, by indexed columns, a bounded slice
//! of ids at a time (`in_chunks`).

use std::collections::HashMap;

use oxidgene_core::enums::{EventType, Sex};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Event, FamilyChild, FamilySpouse, PersonName};
use sea_orm::entity::prelude::*;
use sea_orm::{Condition, ConnectionTrait, JoinType, QueryFilter, QuerySelect};
use uuid::Uuid;

use super::DictionaryRepo;
use super::batch::in_chunks;
use super::db_err;
use super::dictionary::{is_spelled, root_candidates};
use crate::entities::{event, family, family_child, family_spouse, person, person_name, sea_enums};
use oxidgene_core::collections::sorted_unique;

/// What [`DictionaryRepo::load_family_name_descent`] reads, every row of a
/// live person and a live family.
#[derive(Debug, Clone, Default)]
pub struct DescentRows {
    /// The name rows spelled as the family name, whatever their type.
    pub carrier_names: Vec<PersonName>,
    /// The spouses of every family a carrier is a spouse or a child of.
    pub spouses: Vec<FamilySpouse>,
    /// The children of every family a carrier is a spouse of, and the
    /// carriers' own links to the families they are a child of.
    pub children: Vec<FamilyChild>,
    /// The sex of every live person those rows name.
    pub sexes: HashMap<Uuid, Sex>,
    /// Every name of the carriers and of their spouses.
    pub names: Vec<PersonName>,
    /// The births, baptisms, deaths and burials of the carriers and their
    /// spouses, and the marriages of the carriers' families.
    pub events: Vec<Event>,
}

impl DictionaryRepo {
    /// The rows the descent of the family name `value` of `tree_id` is built
    /// from: nine grouped reads, whatever the number of its carriers.
    ///
    /// `value` is matched like [`DictionaryRepo::family_name_usage_person_ids`]
    /// matches it: exactly, as the dictionary lists it, particle included.
    pub async fn load_family_name_descent(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        value: &str,
    ) -> Result<DescentRows, OxidGeneError> {
        let value = value.trim();
        let carrier_names: Vec<PersonName> = person_name::Entity::find()
            .join(JoinType::InnerJoin, person_name::Relation::Person.def())
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(person_name::Column::Surname.is_in(root_candidates(value)))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .filter(|n| is_spelled(n, value))
            .map(super::person_name::into_domain)
            .collect();
        let carriers = sorted_unique(carrier_names.iter().map(|n| n.person_id));

        let own_spouse_rows = spouse_rows(db, &carriers, family_spouse::Column::PersonId).await?;
        let own_child_rows = child_rows(db, &carriers, family_child::Column::PersonId).await?;
        let unions = sorted_unique(own_spouse_rows.iter().map(|s| s.family_id));
        let families = sorted_unique(
            unions
                .iter()
                .copied()
                .chain(own_child_rows.iter().map(|c| c.family_id)),
        );
        let spouses = spouse_rows(db, &families, family_spouse::Column::FamilyId).await?;
        let mut children = child_rows(db, &unions, family_child::Column::FamilyId).await?;
        children.extend(own_child_rows);

        let shown = sorted_unique(
            carriers.iter().copied().chain(
                spouses
                    .iter()
                    .filter(|s| unions.binary_search(&s.family_id).is_ok())
                    .map(|s| s.person_id),
            ),
        );
        let named = sorted_unique(
            shown
                .iter()
                .copied()
                .chain(spouses.iter().map(|s| s.person_id))
                .chain(children.iter().map(|c| c.person_id)),
        );
        let sexes = live_sexes(db, &named).await?;
        let names = in_chunks(&shown, |chunk| async move {
            person_name::Entity::find()
                .filter(person_name::Column::PersonId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?
        .into_iter()
        .map(super::person_name::into_domain)
        .collect();
        let mut events = vital_events(db, &shown).await?;
        events.extend(marriages(db, &unions).await?);
        Ok(DescentRows {
            carrier_names,
            spouses: spouses
                .into_iter()
                .map(super::family_spouse::into_domain)
                .collect(),
            children: children
                .into_iter()
                .map(super::family_child::into_domain)
                .collect(),
            sexes,
            names,
            events,
        })
    }
}

/// The spouse rows of live families whose `column` is one of `ids`.
async fn spouse_rows(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
    column: family_spouse::Column,
) -> Result<Vec<family_spouse::Model>, OxidGeneError> {
    in_chunks(ids, |chunk| async move {
        family_spouse::Entity::find()
            .join(JoinType::InnerJoin, family_spouse::Relation::Family.def())
            .filter(column.is_in(chunk))
            .filter(family::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)
    })
    .await
}

/// The child rows of live families whose `column` is one of `ids`.
async fn child_rows(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
    column: family_child::Column,
) -> Result<Vec<family_child::Model>, OxidGeneError> {
    in_chunks(ids, |chunk| async move {
        family_child::Entity::find()
            .join(JoinType::InnerJoin, family_child::Relation::Family.def())
            .filter(column.is_in(chunk))
            .filter(family::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)
    })
    .await
}

/// The sex of each live person among `ids`.
async fn live_sexes(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Sex>, OxidGeneError> {
    let rows: Vec<(Uuid, sea_enums::Sex)> = in_chunks(ids, |chunk| async move {
        person::Entity::find()
            .select_only()
            .column(person::Column::Id)
            .column(person::Column::Sex)
            .filter(person::Column::Id.is_in(chunk))
            .filter(person::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok(rows.into_iter().map(|(id, sex)| (id, sex.into())).collect())
}

/// The live births, baptisms, deaths and burials of `persons`.
async fn vital_events(
    db: &impl ConnectionTrait,
    persons: &[Uuid],
) -> Result<Vec<Event>, OxidGeneError> {
    let types = [
        EventType::Birth,
        EventType::Baptism,
        EventType::Death,
        EventType::Burial,
    ];
    let rows = in_chunks(persons, |chunk| async move {
        event::Entity::find()
            .filter(event::Column::PersonId.is_in(chunk))
            .filter(event::Column::DeletedAt.is_null())
            .filter(types.into_iter().fold(Condition::any(), |any, kind| {
                any.add(event::Column::EventType.eq(sea_enums::EventType::from(kind)))
            }))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok(rows.into_iter().map(super::event::into_domain).collect())
}

/// The live marriages of `families`.
async fn marriages(
    db: &impl ConnectionTrait,
    families: &[Uuid],
) -> Result<Vec<Event>, OxidGeneError> {
    let rows = in_chunks(families, |chunk| async move {
        event::Entity::find()
            .filter(event::Column::FamilyId.is_in(chunk))
            .filter(event::Column::DeletedAt.is_null())
            .filter(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Marriage)))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok(rows.into_iter().map(super::event::into_domain).collect())
}
