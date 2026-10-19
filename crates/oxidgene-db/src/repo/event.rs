//! Repository for `Event` entities (CRUD with soft delete, type/person/family filters).

use chrono::{NaiveDate, Utc};
use oxidgene_core::enums::{Calendar, DateQualifier, EventType};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Event, RecordKind};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::event::{self, ActiveModel, Column, Entity};
use crate::entities::sea_enums;
use crate::repo::batch::in_chunks;
use crate::repo::db_err;
use crate::repo::event_spouse_age::EventSpouseAgeRepo;
use crate::repo::pagination::{PaginationParams, paginate};
use crate::repo::record_tag::RecordTagRepo;

/// Optional filters for listing events.
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub event_type: Option<EventType>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
}

/// An event's less common details, GEDCOM's event detail structure: the
/// cause, the age the record gives (already in canonical form), the
/// responsible agency, the classification an attribute's value carries and
/// the family an individual event names without belonging to it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventDetails {
    pub cause: Option<String>,
    pub age: Option<String>,
    pub agency: Option<String>,
    pub classification: Option<String>,
    pub related_family_id: Option<Uuid>,
}

/// The [`EventDetails`] an update changes: `None` keeps a field and
/// `Some(None)` clears it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventDetailsPatch {
    pub cause: Option<Option<String>>,
    pub age: Option<Option<String>>,
    pub agency: Option<Option<String>>,
    pub classification: Option<Option<String>>,
    pub related_family_id: Option<Option<Uuid>>,
}

/// Writes the detail fields `details` changes into `active`; a field it
/// leaves `None` keeps its stored value.
fn apply_details(active: &mut ActiveModel, details: EventDetailsPatch) {
    if let Some(cause) = details.cause {
        active.cause = Set(cause);
    }
    if let Some(age) = details.age {
        active.age = Set(age);
    }
    if let Some(agency) = details.agency {
        active.agency = Set(agency);
    }
    if let Some(classification) = details.classification {
        active.classification = Set(classification);
    }
    if let Some(related_family_id) = details.related_family_id {
        active.related_family_id = Set(related_family_id);
    }
}

/// Repository for event CRUD operations.
pub struct EventRepo;

impl EventRepo {
    /// List events in a tree with optional filters and pagination.
    pub async fn list(
        db: &impl ConnectionTrait,
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

        let mut page = paginate(db, query, Column::Id, params, |m| (m.id, into_domain(m))).await?;
        let events = page.edges.iter().map(|edge| edge.node.clone()).collect();
        for (edge, event) in page.edges.iter_mut().zip(hydrate(db, events).await?) {
            edge.node = event;
        }
        Ok(page)
    }

    /// List all events in a tree without pagination (excludes soft-deleted).
    pub async fn list_all(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<Event>, OxidGeneError> {
        let models = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        hydrate(db, models.into_iter().map(into_domain).collect()).await
    }

    /// List all events attached to any of the given persons.
    pub async fn list_by_persons(
        db: &impl ConnectionTrait,
        person_ids: &[Uuid],
    ) -> Result<Vec<Event>, OxidGeneError> {
        let events = in_chunks(person_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::PersonId.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await?;
        hydrate(db, events).await
    }

    /// List all events attached to any of the given families (excludes soft-deleted).
    pub async fn list_by_families(
        db: &impl ConnectionTrait,
        family_ids: &[Uuid],
    ) -> Result<Vec<Event>, OxidGeneError> {
        let events = in_chunks(family_ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::FamilyId.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await?;
        hydrate(db, events).await
    }

    /// Get a single event by ID (excludes soft-deleted).
    pub async fn get(db: &impl ConnectionTrait, id: Uuid) -> Result<Event, OxidGeneError> {
        let event = find_live(db, id).await.map(into_domain)?;
        hydrate_one(db, event).await
    }

    /// The live events among `ids`, one read per bounded slice of ids. Their
    /// tags and spouse ages are not filled: this serves the labels and owners
    /// of many records at once. A missing or soft-deleted id is absent.
    pub async fn get_many_untagged(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
    ) -> Result<Vec<Event>, OxidGeneError> {
        in_chunks(ids, |chunk| async move {
            let models = Entity::find()
                .filter(Column::Id.is_in(chunk))
                .filter(Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)?;
            Ok(models.into_iter().map(into_domain).collect())
        })
        .await
    }

    /// Create a new event.
    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per column the statement writes, already validated by the calling service"
    )]
    pub async fn create(
        db: &impl ConnectionTrait,
        id: Uuid,
        tree_id: Uuid,
        event_type: EventType,
        date_value: Option<String>,
        date_sort: Option<NaiveDate>,
        place_id: Option<Uuid>,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
        description: Option<String>,
        date_qualifier: DateQualifier,
        date_value2: Option<String>,
        calendar: Calendar,
        details: EventDetails,
    ) -> Result<Event, OxidGeneError> {
        let now = Utc::now();
        let model = event::ActiveModel {
            id: Set(id),
            tree_id: Set(tree_id),
            event_type: Set(sea_enums::EventType::from(event_type)),
            date_value: Set(date_value),
            date_sort: Set(date_sort),
            date_qualifier: Set(sea_enums::DateQualifier::from(date_qualifier)),
            date_value2: Set(date_value2),
            calendar: Set(sea_enums::Calendar::from(calendar)),
            cause: Set(details.cause),
            age: Set(details.age),
            agency: Set(details.agency),
            classification: Set(details.classification),
            related_family_id: Set(details.related_family_id),
            place_id: Set(place_id),
            person_id: Set(person_id),
            family_id: Set(family_id),
            description: Set(description),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
        };
        let result = model.insert(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Update an existing event.
    #[expect(
        clippy::too_many_arguments,
        reason = "one parameter per column the statement writes, already validated by the calling service"
    )]
    pub async fn update(
        db: &impl ConnectionTrait,
        id: Uuid,
        event_type: Option<EventType>,
        date_value: Option<Option<String>>,
        date_sort: Option<Option<NaiveDate>>,
        place_id: Option<Option<Uuid>>,
        description: Option<Option<String>>,
        date_qualifier: Option<DateQualifier>,
        date_value2: Option<Option<String>>,
        calendar: Option<Calendar>,
        details: EventDetailsPatch,
    ) -> Result<Event, OxidGeneError> {
        let existing = find_live(db, id).await?;

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
        if let Some(dq) = date_qualifier {
            active.date_qualifier = Set(sea_enums::DateQualifier::from(dq));
        }
        if let Some(dv2) = date_value2 {
            active.date_value2 = Set(dv2);
        }
        if let Some(cal) = calendar {
            active.calendar = Set(sea_enums::Calendar::from(cal));
        }
        apply_details(&mut active, details);
        active.updated_at = Set(Utc::now());

        let result = active.update(db).await.map_err(db_err)?;
        hydrate_one(db, into_domain(result)).await
    }

    /// Soft-delete an event.
    pub async fn delete(db: &impl ConnectionTrait, id: Uuid) -> Result<(), OxidGeneError> {
        let existing = find_live(db, id).await?;

        let mut active: ActiveModel = existing.into_active_model();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(db).await.map_err(db_err)?;
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
        date_qualifier: m.date_qualifier.into(),
        date_value2: m.date_value2,
        calendar: m.calendar.into(),
        cause: m.cause,
        age: m.age,
        agency: m.agency,
        classification: m.classification,
        spouse_ages: Vec::new(),
        place_id: m.place_id,
        person_id: m.person_id,
        family_id: m.family_id,
        related_family_id: m.related_family_id,
        description: m.description,
        tags: Vec::new(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

/// Fill the derived parts of `events` in one grouped read each: the spouse
/// ages of the family events and the tags of every event.
async fn hydrate(
    db: &impl ConnectionTrait,
    events: Vec<Event>,
) -> Result<Vec<Event>, OxidGeneError> {
    let mut events = EventSpouseAgeRepo::attach(db, events).await?;
    let ids: Vec<Uuid> = events.iter().map(|event| event.id).collect();
    let mut tags = RecordTagRepo::tags_of(db, RecordKind::Event, &ids).await?;
    for event in &mut events {
        event.tags = tags.remove(&event.id).unwrap_or_default();
    }
    Ok(events)
}

/// [`hydrate`] for one event.
async fn hydrate_one(db: &impl ConnectionTrait, event: Event) -> Result<Event, OxidGeneError> {
    let mut events = hydrate(db, vec![event]).await?;
    Ok(events.remove(0))
}

/// Row `id`, unless it is missing or soft-deleted.
async fn find_live(
    db: &impl ConnectionTrait,
    id: Uuid,
) -> Result<<Entity as EntityTrait>::Model, OxidGeneError> {
    crate::repo::find_live::<Entity>(db, id, Column::DeletedAt, "Event").await
}
