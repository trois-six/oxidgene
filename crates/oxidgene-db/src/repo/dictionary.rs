//! Aggregated "dictionary" queries: distinct values entered across a tree
//! (family names, occupations) or existing entities (sources, places) paired
//! with how many persons/events reference them, plus drill-down lookups
//! resolving a value back to the persons that carry it.

use oxidgene_core::enums::EventType;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Place, Source, year_from_date};
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

/// A person's name (split given/surname) plus birth/death years, resolved in
/// bulk for a dictionary usage drill-down list.
#[derive(Debug, Clone)]
pub struct PersonUsageEntry {
    pub person_id: Uuid,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub birth_year: Option<i32>,
    pub death_year: Option<i32>,
}

/// Above this many sources matching a prefix, the Sources tab's smart
/// drill-down (see `DictionaryRepo::resolve_source_drill_down` and
/// ui-dictionary.md §8) shows further branch choices instead of the final
/// flat list.
pub const SOURCE_DRILL_THRESHOLD: i64 = 250;

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

    /// All sources in a tree whose title starts with `prefix` (case- and
    /// accent-insensitive on case only), paired with their citation count.
    /// Used by the Sources tab's smart drill-down once a prefix narrows the
    /// set to <= 250 sources (see `source_group_counts` below and
    /// ui-dictionary.md §8). An empty prefix returns every source, same as
    /// `sources_with_usage`.
    pub async fn sources_with_usage_by_prefix(
        db: &DatabaseConnection,
        tree_id: Uuid,
        prefix: &str,
    ) -> Result<Vec<(Source, i64)>, OxidGeneError> {
        let all = Self::sources_with_usage(db, tree_id).await?;
        if prefix.is_empty() {
            return Ok(all);
        }
        let prefix_upper = prefix.to_uppercase();
        Ok(all
            .into_iter()
            .filter(|(s, _)| s.title.to_uppercase().starts_with(&prefix_upper))
            .collect())
    }

    /// Groups a tree's sources whose title starts with `prefix` by the next
    /// character after `prefix`, returning `(group_label, count)` pairs —
    /// `group_label` is always `prefix` extended by exactly one more
    /// (uppercased) character. Only groups that actually occur are
    /// returned, so the frontend never has to guess which letters/prefixes
    /// are populated in this tree.
    ///
    /// Drives the Sources tab's smart drill-down: the caller keeps
    /// requesting one level deeper (passing the clicked group label back as
    /// `prefix`) until a group's count drops to <= 250, at which point it
    /// switches to `sources_with_usage_by_prefix` for the final flat list.
    /// See ui-dictionary.md §8.
    pub async fn source_group_counts(
        db: &DatabaseConnection,
        tree_id: Uuid,
        prefix: &str,
    ) -> Result<Vec<(String, i64)>, OxidGeneError> {
        let sources = source::Entity::find()
            .filter(source::Column::TreeId.eq(tree_id))
            .filter(source::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        let prefix_upper = prefix.to_uppercase();
        let prefix_len = prefix_upper.chars().count();

        let mut counts: HashMap<String, i64> = HashMap::new();
        for s in sources {
            let title_upper = s.title.to_uppercase();
            if !title_upper.starts_with(&prefix_upper) {
                continue;
            }
            let group: String = if title_upper.chars().count() > prefix_len {
                title_upper.chars().take(prefix_len + 1).collect()
            } else {
                // Title is no longer than the prefix itself (rare) — keep
                // it grouped under the prefix rather than dropping it.
                title_upper.clone()
            };
            *counts.entry(group).or_insert(0) += 1;
        }

        let mut out: Vec<(String, i64)> = counts.into_iter().collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Resolves the Sources tab's smart drill-down starting from `prefix`:
    /// repeatedly extends the prefix while `source_group_counts` reports
    /// exactly one possible next character, skipping "forced" steps that
    /// offer no real choice (e.g. a single town's records nested under a
    /// department that otherwise branches many ways). Stops at whichever
    /// comes first — a genuine branch point (more than one possible next
    /// character) or a prefix whose count has dropped to <= `threshold`.
    ///
    /// Returns `(resolved_prefix, total, groups)`: `resolved_prefix` may be
    /// longer than the input `prefix` (every auto-skipped character is
    /// folded in); `groups` is empty when `total <= threshold` — the caller
    /// should then fetch the final flat list via
    /// `sources_with_usage_by_prefix(resolved_prefix)` instead of rendering
    /// another drill-down level. See ui-dictionary.md §8.10.
    pub async fn resolve_source_drill_down(
        db: &DatabaseConnection,
        tree_id: Uuid,
        prefix: &str,
        threshold: i64,
    ) -> Result<(String, i64, Vec<(String, i64)>), OxidGeneError> {
        let mut current = prefix.to_uppercase();
        loop {
            let groups = Self::source_group_counts(db, tree_id, &current).await?;
            let total: i64 = groups.iter().map(|(_, c)| *c).sum();
            if total <= threshold {
                return Ok((current, total, Vec::new()));
            }
            if groups.len() != 1 {
                return Ok((current, total, groups));
            }
            let (only_label, _) = &groups[0];
            if only_label == &current {
                // No further characters to drill into (every remaining
                // source's title is exactly `current`) — stop even though
                // `total` is still above the threshold.
                return Ok((current, total, groups));
            }
            current = only_label.clone();
        }
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

    /// Distinct persons carrying a given surname in a tree.
    pub async fn family_name_usage_person_ids(
        db: &DatabaseConnection,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
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
            .filter(person_name::Column::Surname.eq(value))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(dedup(names.into_iter().map(|n| n.person_id).collect()))
    }

    /// Resolve a batch of person IDs (as returned by the `*_usage_person_ids`
    /// queries above) into display name parts + birth/death years, in bulk —
    /// avoids one HTTP round trip per person on the dictionary usage panel.
    /// Sorted by given name, matching how the panel lists people.
    pub async fn resolve_person_usage_entries(
        db: &DatabaseConnection,
        person_ids: &[Uuid],
    ) -> Result<Vec<PersonUsageEntry>, OxidGeneError> {
        if person_ids.is_empty() {
            return Ok(Vec::new());
        }

        let names = person_name::Entity::find()
            .filter(person_name::Column::PersonId.is_in(person_ids.to_vec()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut name_by_person: HashMap<Uuid, person_name::Model> = HashMap::new();
        for n in names {
            let is_better = match name_by_person.get(&n.person_id) {
                Some(existing) => !existing.is_primary && n.is_primary,
                None => true,
            };
            if is_better {
                name_by_person.insert(n.person_id, n);
            }
        }

        let events = event::Entity::find()
            .filter(event::Column::PersonId.is_in(person_ids.to_vec()))
            .filter(event::Column::DeletedAt.is_null())
            .filter(
                event::Column::EventType
                    .eq(sea_enums::EventType::from(EventType::Birth))
                    .or(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Death))),
            )
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut birth_by_person: HashMap<Uuid, i32> = HashMap::new();
        let mut death_by_person: HashMap<Uuid, i32> = HashMap::new();
        for e in events {
            let Some(pid) = e.person_id else { continue };
            let Some(year) = year_from_date(e.date_sort, e.date_value.as_deref()) else {
                continue;
            };
            let bucket = match EventType::from(e.event_type) {
                EventType::Birth => &mut birth_by_person,
                EventType::Death => &mut death_by_person,
                _ => continue,
            };
            bucket.entry(pid).or_insert(year);
        }

        let mut out: Vec<PersonUsageEntry> = person_ids
            .iter()
            .map(|&person_id| {
                let name = name_by_person.get(&person_id);
                PersonUsageEntry {
                    person_id,
                    given_names: name.and_then(|n| trimmed(n.given_names.as_deref())),
                    surname: name.and_then(|n| trimmed(n.surname.as_deref())),
                    birth_year: birth_by_person.get(&person_id).copied(),
                    death_year: death_by_person.get(&person_id).copied(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            let ka = a.given_names.as_deref().unwrap_or("").to_lowercase();
            let kb = b.given_names.as_deref().unwrap_or("").to_lowercase();
            ka.cmp(&kb)
        });
        Ok(out)
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
