//! Event writes and witnesses: the same steps whether REST or GraphQL asked.
//!
//! Each write checks that every record it names belongs to the tree, derives
//! the event's sort date, rewrites the projections of the persons it shows
//! on, and records the change, in one transaction. A witness is addressed
//! through its event and must belong to it.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{Connection, Event, EventWitness};
use oxidgene_core::{Calendar, DateQualifier, EventType};
use oxidgene_db::repo::{EventFilter, EventRepo, EventWitnessRepo, PaginationParams};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::Deserialize;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::service::event_date;
use crate::service::history::Change;
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// An event to create: what happened, when, where, and to whom — a person or
/// a family.
#[derive(Debug, Deserialize)]
pub struct NewEvent {
    pub event_type: EventType,
    pub date_value: Option<String>,
    #[serde(default)]
    pub date_qualifier: DateQualifier,
    #[serde(default)]
    pub date_value2: Option<String>,
    #[serde(default)]
    pub calendar: Calendar,
    #[serde(default)]
    pub cause: Option<String>,
    pub place_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
}

/// The fields an event update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one. The owner of an event never changes.
#[derive(Debug, Default, Deserialize)]
pub struct EventPatch {
    pub event_type: Option<EventType>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value: Option<Option<String>>,
    pub date_qualifier: Option<DateQualifier>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value2: Option<Option<String>>,
    pub calendar: Option<Calendar>,
    #[serde(default, deserialize_with = "double_option")]
    pub cause: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub place_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
}

/// A witness to add to an event.
#[derive(Debug, Deserialize)]
pub struct NewWitness {
    pub person_id: Uuid,
    pub relation: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
}

/// The events of `tree_id`, narrowed by `filter`, a page at a time. A person
/// or family filter naming a record of another tree is not found.
pub async fn list_events(
    db: &DatabaseConnection,
    tree_id: Uuid,
    filter: &EventFilter,
    params: &PaginationParams,
) -> Result<Connection<Event>, OxidGeneError> {
    for (resource, id) in [
        (TreeResource::Person, filter.person_id),
        (TreeResource::Family, filter.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(db, tree_id, resource, id).await?;
        }
    }
    EventRepo::list(db, tree_id, filter, params).await
}

/// Create an event in `tree_id`.
pub async fn create_event(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: NewEvent,
) -> Result<Event, OxidGeneError> {
    // Derived here, never taken from the request — see `service::event_date`.
    let date_sort = event_date::derive(new.calendar, new.date_value.as_deref());
    let txn = begin_tx(db).await?;
    for (resource, id) in [
        (TreeResource::Place, new.place_id),
        (TreeResource::Person, new.person_id),
        (TreeResource::Family, new.family_id),
    ] {
        if let Some(id) = id {
            require_tree_resource(&txn, tree_id, resource, id).await?;
        }
    }
    let id = Uuid::now_v7();
    let event = EventRepo::create(
        &txn,
        id,
        tree_id,
        new.event_type,
        new.date_value,
        date_sort,
        new.place_id,
        new.person_id,
        new.family_id,
        new.description,
        new.date_qualifier,
        new.date_value2,
        new.calendar,
        new.cause,
    )
    .await?;
    // A person's event or a family's.
    let affected =
        invalidation::affected_persons_for_event(&txn, new.person_id, new.family_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::create(tree_id, AuditEntity::Event, id)
        .event(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(event)
}

/// Update event `id` of `tree_id`. A place of another tree is not found.
pub async fn update_event(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
    patch: EventPatch,
) -> Result<Event, OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Event, id).await?;
    if let Some(Some(place_id)) = patch.place_id {
        require_tree_resource(&txn, tree_id, TreeResource::Place, place_id).await?;
    }
    // Derived from the patched state, reading whichever half the patch leaves
    // alone off the stored event — see `service::event_date`.
    let stored = EventRepo::get(&txn, id).await?;
    let date_sort = Some(event_date::derive_patch(
        stored.calendar,
        stored.date_value.as_deref(),
        patch.calendar,
        patch.date_value.as_ref().map(Option::as_deref),
    ));
    let event = EventRepo::update(
        &txn,
        id,
        patch.event_type,
        patch.date_value,
        date_sort,
        patch.place_id,
        patch.description,
        patch.date_qualifier,
        patch.date_value2,
        patch.calendar,
        patch.cause,
    )
    .await?;
    let affected =
        invalidation::affected_persons_for_event(&txn, event.person_id, event.family_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::update(tree_id, AuditEntity::Event, id)
        .event(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(event)
}

/// Delete event `id` of `tree_id` (a soft delete).
pub async fn delete_event(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Event, id).await?;
    let event = EventRepo::get(&txn, id).await?;
    EventRepo::delete(&txn, id).await?;
    let affected =
        invalidation::affected_persons_for_event(&txn, event.person_id, event.family_id).await?;
    profiles
        .invalidate_for_mutation(&txn, tree_id, &affected)
        .await?;
    Change::delete(tree_id, AuditEntity::Event, id)
        .event(id)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}

/// The witnesses of event `event_id` of `tree_id`.
pub async fn list_witnesses(
    db: &DatabaseConnection,
    tree_id: Uuid,
    event_id: Uuid,
) -> Result<Vec<EventWitness>, OxidGeneError> {
    require_tree_resource(db, tree_id, TreeResource::Event, event_id).await?;
    EventWitnessRepo::list_by_event(db, event_id).await
}

/// Add a witness to event `event_id` of `tree_id`.
pub async fn add_witness(
    db: &DatabaseConnection,
    tree_id: Uuid,
    event_id: Uuid,
    new: NewWitness,
) -> Result<EventWitness, OxidGeneError> {
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Event, event_id).await?;
    require_tree_resource(&txn, tree_id, TreeResource::Person, new.person_id).await?;
    let witness = EventWitnessRepo::create(
        &txn,
        id,
        event_id,
        new.person_id,
        new.relation,
        new.sort_order,
    )
    .await?;
    Change::create(tree_id, AuditEntity::EventWitness, id)
        .event(event_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await?;
    Ok(witness)
}

/// Remove witness `witness_id` of `tree_id`.
///
/// `event_id`, when given, is the event the caller addresses the witness
/// through; a witness of another event is then not found.
pub async fn remove_witness(
    db: &DatabaseConnection,
    tree_id: Uuid,
    event_id: Option<Uuid>,
    witness_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    require_tree_resource(&txn, tree_id, TreeResource::EventWitness, witness_id).await?;
    let witness = EventWitnessRepo::get(&txn, witness_id).await?;
    if event_id.is_some_and(|event_id| event_id != witness.event_id) {
        return Err(OxidGeneError::NotFound {
            entity: "EventWitness",
            id: witness_id,
        });
    }
    EventWitnessRepo::delete(&txn, witness_id).await?;
    Change::delete(tree_id, AuditEntity::EventWitness, witness_id)
        .event(witness.event_id)
        .record(&txn)
        .await?;
    commit_tx(txn).await
}
