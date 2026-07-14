//! Aggregated "dictionary" queries: distinct values entered across a tree
//! (family names, occupations) or existing entities (sources, places) paired
//! with how many persons/events reference them, plus drill-down lookups
//! resolving a value back to the persons that carry it.

use oxidgene_core::enums::EventType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Place, Source};
use sea_orm::QueryFilter;
use sea_orm::entity::prelude::*;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::entities::{citation, event, media, person, person_name, place, sea_enums, source};

/// A distinct free-text value (surname, occupation label) plus the number of
/// persons carrying it.
#[derive(Debug, Clone)]
pub struct DictionaryValueEntry {
    pub value: String,
    pub count: i64,
}

pub struct DictionaryRepo;

impl DictionaryRepo {
    /// Distinct surnames across all persons in a tree, with the number of
    /// persons carrying each (as entered — no accent-folding/normalization).
    pub async fn family_names(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
        let person_ids: Vec<Uuid> = person::Entity::find()
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .into_iter()
            .map(|p| p.id)
            .collect();

        if person_ids.is_empty() {
            return Ok(Vec::new());
        }

        let names = person_name::Entity::find()
            .filter(person_name::Column::PersonId.is_in(person_ids))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        // Group by person, not by row: a person with two `PersonName` entries
        // sharing the same surname (e.g. birth + nickname) must count once.
        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for n in names {
            if let Some(surname) = trimmed(n.surname.as_deref()) {
                per_value.entry(surname).or_default().insert(n.person_id);
            }
        }
        Ok(sorted_entries(per_value))
    }

    /// Distinct occupation labels (`Event.description` for `Occupation`
    /// events) across a tree, with the number of persons holding each.
    pub async fn occupations(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
        let events = event::Entity::find()
            .filter(event::Column::TreeId.eq(tree_id))
            .filter(event::Column::DeletedAt.is_null())
            .filter(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Occupation)))
            .filter(event::Column::PersonId.is_not_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        // Group by person: the same label recorded on two occupation events
        // for one person (e.g. at different life stages) must count once.
        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for e in events {
            if let (Some(label), Some(pid)) = (trimmed(e.description.as_deref()), e.person_id) {
                per_value.entry(label).or_default().insert(pid);
            }
        }
        Ok(sorted_entries(per_value))
    }

    /// All sources in a tree paired with their citation count.
    pub async fn sources_with_usage(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<(Source, i64)>, OxidGeneError> {
        let sources = source::Entity::find()
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let source_ids: Vec<Uuid> = sources.iter().map(|s| s.id).collect();
        let mut counts: HashMap<Uuid, i64> = HashMap::new();
        if !source_ids.is_empty() {
            let citations = citation::Entity::find()
                .filter(citation::Column::SourceId.is_in(source_ids))
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            for c in citations {
                *counts.entry(c.source_id).or_insert(0) += 1;
            }
        }

        let mut out: Vec<(Source, i64)> = sources
            .into_iter()
            .map(|m| {
                let count = counts.get(&m.id).copied().unwrap_or(0);
                (into_source(m), count)
            })
            .collect();
        out.sort_by_key(|(a, _)| a.title.to_lowercase());
        Ok(out)
    }

    /// All places in a tree paired with their usage count (events + media
    /// referencing them).
    pub async fn places_with_usage(
        db: &DatabaseConnection,
        tree_id: Uuid,
    ) -> Result<Vec<(Place, i64)>, OxidGeneError> {
        let places = place::Entity::find()
            .filter(place::Column::TreeId.eq(tree_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let place_ids: Vec<Uuid> = places.iter().map(|p| p.id).collect();
        let mut counts: HashMap<Uuid, i64> = HashMap::new();
        if !place_ids.is_empty() {
            let events = event::Entity::find()
                .filter(event::Column::PlaceId.is_in(place_ids.clone()))
                .filter(event::Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            for e in events {
                if let Some(pid) = e.place_id {
                    *counts.entry(pid).or_insert(0) += 1;
                }
            }

            let medias = media::Entity::find()
                .filter(media::Column::PlaceId.is_in(place_ids))
                .filter(media::Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            for m in medias {
                if let Some(pid) = m.place_id {
                    *counts.entry(pid).or_insert(0) += 1;
                }
            }
        }

        let mut out: Vec<(Place, i64)> = places
            .into_iter()
            .map(|m| {
                let count = counts.get(&m.id).copied().unwrap_or(0);
                (into_place(m), count)
            })
            .collect();
        out.sort_by_key(|(a, _)| a.name.to_lowercase());
        Ok(out)
    }

    /// Distinct persons cited by a given source (via a direct person
    /// citation, or via the person of a cited individual event).
    pub async fn source_usage_person_ids(
        db: &DatabaseConnection,
        source_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let citations = citation::Entity::find()
            .filter(citation::Column::SourceId.eq(source_id))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let mut event_ids = Vec::new();
        let mut person_ids: Vec<Uuid> = Vec::new();
        for c in &citations {
            if let Some(pid) = c.person_id {
                person_ids.push(pid);
            } else if let Some(eid) = c.event_id {
                event_ids.push(eid);
            }
        }

        if !event_ids.is_empty() {
            let events = event::Entity::find()
                .filter(event::Column::Id.is_in(event_ids))
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;
            person_ids.extend(events.into_iter().filter_map(|e| e.person_id));
        }

        Ok(dedup(person_ids))
    }

    /// Distinct persons with an individual event at a given place.
    pub async fn place_usage_person_ids(
        db: &DatabaseConnection,
        place_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let events = event::Entity::find()
            .filter(event::Column::PlaceId.eq(place_id))
            .filter(event::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(dedup(
            events.into_iter().filter_map(|e| e.person_id).collect(),
        ))
    }

    /// Distinct persons holding a given occupation label in a tree.
    pub async fn occupation_usage_person_ids(
        db: &DatabaseConnection,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let events = event::Entity::find()
            .filter(event::Column::TreeId.eq(tree_id))
            .filter(event::Column::DeletedAt.is_null())
            .filter(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Occupation)))
            .filter(event::Column::Description.eq(value))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(dedup(
            events.into_iter().filter_map(|e| e.person_id).collect(),
        ))
    }
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn sorted_entries(per_value: HashMap<String, HashSet<Uuid>>) -> Vec<DictionaryValueEntry> {
    let mut out: Vec<DictionaryValueEntry> = per_value
        .into_iter()
        .map(|(value, ids)| DictionaryValueEntry {
            value,
            count: ids.len() as i64,
        })
        .collect();
    out.sort_by_key(|a| a.value.to_lowercase());
    out
}

fn dedup(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort();
    ids.dedup();
    ids
}

fn into_source(m: source::Model) -> Source {
    Source {
        id: m.id,
        tree_id: m.tree_id,
        title: m.title,
        author: m.author,
        publisher: m.publisher,
        abbreviation: m.abbreviation,
        repository_name: m.repository_name,
        created_at: m.created_at,
        updated_at: m.updated_at,
        deleted_at: m.deleted_at,
    }
}

fn into_place(m: place::Model) -> Place {
    Place {
        id: m.id,
        tree_id: m.tree_id,
        name: m.name,
        latitude: m.latitude,
        longitude: m.longitude,
        created_at: m.created_at,
        updated_at: m.updated_at,
    }
}
