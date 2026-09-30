//! Event creation: the same steps whether REST or GraphQL asked.
//!
//! The write checks that every record the event names belongs to the tree,
//! derives its sort date, rewrites the projections of the persons it shows
//! on, and records the change, in one transaction.

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::Event;
use oxidgene_db::repo::EventRepo;
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};
use crate::rest::dto::CreateEventRequest;
use crate::rest::state::{TreeResource, begin_tx, commit_tx, require_tree_resource};
use crate::service::event_date;
use crate::service::history::Change;

/// Create an event in `tree_id`.
pub async fn create_event(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
    new: CreateEventRequest,
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
