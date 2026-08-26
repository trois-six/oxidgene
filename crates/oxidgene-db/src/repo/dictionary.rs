//! Aggregated "dictionary" queries: distinct values entered across a tree
//! (family names, occupations) or existing entities (sources, places) paired
//! with how many persons/events reference them, plus drill-down lookups
//! resolving a value back to the persons that carry it.
//!
//! Plus the one bulk edit the page offers — [`DictionaryRepo::set_family_name_particle`],
//! which re-cuts every occurrence of one surname at once. It lives here rather
//! than in `PersonNameRepo` because "which rows are this dictionary entry" is
//! defined by [`DictionaryRepo::family_names`] right above it, and the two must
//! agree on the answer.

use chrono::Utc;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::{
    enums::{DateQualifier, EventType},
    types::{
        Place, Source, join_surname_particle, split_surname_at_head, split_surname_particle,
        year_from_date,
    },
};
use sea_orm::ConnectionTrait;
use sea_orm::QueryFilter;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, Condition, Unchanged};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::entities::{citation, event, media, person, person_name, place, sea_enums, source};

/// A distinct free-text value (surname, occupation label) plus the number of
/// persons carrying it.
#[derive(Debug, Clone)]
pub struct DictionaryValueEntry {
    /// The value as it should be displayed — for surnames, particle included
    /// ("de la Cruz").
    pub value: String,
    /// The key this value files under when particles are ignored — for
    /// surnames, the root only ("cruz"), lowercased.
    ///
    /// Returned alongside `value` so the client can honour the user's
    /// "sort particles" preference without a second round trip. Entries
    /// arrive sorted by `value`, i.e. particles included.
    pub sort_key: String,
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
    /// Precision of `birth_year`, so the list hedges the same way the pedigree
    /// cards do. Beside the year rather than folded into it — the year stays an
    /// integer the client can sort on.
    pub birth_qualifier: DateQualifier,
    pub death_year: Option<i32>,
    pub death_qualifier: DateQualifier,
}

/// Outcome of [`DictionaryRepo::set_family_name_particle`].
#[derive(Debug, Clone)]
pub struct FamilyNameParticleUpdate {
    /// The surname as it will still be listed — re-cutting moves the boundary
    /// inside the name, never the text.
    pub value: String,
    /// The particle now stored, `None` when the name was declared to have one.
    pub surname_prefix: Option<String>,
    /// The root now stored, i.e. what the name files under.
    pub surname: String,
    /// `person_name` rows rewritten. Rows already cut that way are skipped, so
    /// a second identical call reports zero.
    pub names_updated: usize,
    /// Distinct persons behind `names_updated`.
    pub persons_updated: usize,
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
        db: &impl ConnectionTrait,
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
        //
        // Keyed on the full surname, particle included: "de la Cruz" and
        // "Cruz" are two different families and must stay two entries. The
        // particle only affects where each one *files*, which is what
        // `sort_key` carries.
        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        let mut roots: HashMap<String, String> = HashMap::new();
        for n in names {
            let Some(root) = trimmed(n.surname.as_deref()) else {
                continue;
            };
            let full = join_surname_particle(n.surname_prefix.as_deref(), &root);
            roots.insert(full.clone(), root.to_lowercase());
            per_value.entry(full).or_default().insert(n.person_id);
        }
        Ok(sorted_entries_with(per_value, |value| {
            roots
                .get(value)
                .cloned()
                .unwrap_or_else(|| value.to_lowercase())
        }))
    }

    /// Distinct occupation labels (`Event.description` for `Occupation`
    /// events) across a tree, with the number of persons holding each.
    pub async fn occupations(
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        out.sort_by_cached_key(|(a, _)| a.title.to_lowercase());
        Ok(out)
    }

    /// All sources in a tree whose title starts with `prefix` (case- and
    /// accent-insensitive on case only), paired with their citation count.
    /// Used by the Sources tab's smart drill-down once a prefix narrows the
    /// set to <= 250 sources (see `source_group_counts` below and
    /// ui-dictionary.md §8). An empty prefix returns every source, same as
    /// `sources_with_usage`.
    pub async fn sources_with_usage_by_prefix(
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        out.sort_by_cached_key(|(a, _)| a.name.to_lowercase());
        Ok(out)
    }

    /// Distinct persons cited by a given source (via a direct person
    /// citation, or via the person of a cited individual event).
    pub async fn source_usage_person_ids(
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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
        db: &impl ConnectionTrait,
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

        // `value` comes from `family_names`, which reports full surnames, so
        // it may carry a particle while the column holds only the root. Match
        // on the root and re-check the particle in memory, so that "Cruz" and
        // "de la Cruz" resolve to their own people rather than to each other's.
        let (particle, root) = split_surname_particle(value);

        let names = person_name::Entity::find()
            .filter(person_name::Column::PersonId.is_in(person_ids))
            .filter(person_name::Column::Surname.eq(root.as_str()))
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;

        Ok(dedup(
            names
                .into_iter()
                .filter(|n| {
                    trimmed(n.surname_prefix.as_deref()) == particle.as_ref().map(|p| p.to_string())
                })
                .map(|n| n.person_id)
                .collect(),
        ))
    }

    /// Re-cut every occurrence of one surname at the given particle.
    ///
    /// This is the dictionary's bulk repair for a particle that detection got
    /// wrong across a whole family — a tree full of "Le …" persons wrongly
    /// carrying a `Le` prefix is fixed in one call with an empty `particle`.
    ///
    /// `value` is a surname as listed by [`Self::family_names`], particle
    /// included. Rows are matched on that *joined* surname rather than by
    /// re-splitting it, because how they are currently cut is precisely what
    /// is being corrected — the full surname is a dictionary entry's only
    /// stable identity.
    ///
    /// The displayed surname never changes; only the boundary inside it does,
    /// and with it the letter the name files under. A `particle` that is not at
    /// the head of `value` is rejected rather than prepended, so this can never
    /// invent a word the tree does not already carry.
    ///
    /// Rows already cut that way are left untouched, making a repeated call a
    /// no-op instead of a pointless `updated_at` bump.
    ///
    /// # Errors
    ///
    /// Returns [`OxidGeneError::Validation`] if `value` is blank or `particle`
    /// is not at its head, and [`OxidGeneError::Database`] on query failure.
    pub async fn set_family_name_particle(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        value: &str,
        particle: &str,
    ) -> Result<FamilyNameParticleUpdate, OxidGeneError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(OxidGeneError::Validation(
                "a family name is required".to_string(),
            ));
        }
        let Some((new_prefix, new_surname)) = split_surname_at_head(value, particle) else {
            return Err(OxidGeneError::Validation(format!(
                "particle \"{particle}\" is not at the head of surname \"{value}\""
            )));
        };

        let person_ids: Vec<Uuid> = person::Entity::find()
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?
            .into_iter()
            .map(|p| p.id)
            .collect();

        let mut updated_persons: HashSet<Uuid> = HashSet::new();
        let mut names_updated = 0usize;

        if !person_ids.is_empty() {
            let names = person_name::Entity::find()
                .filter(person_name::Column::PersonId.is_in(person_ids))
                .all(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;

            for n in names {
                let Some(root) = trimmed(n.surname.as_deref()) else {
                    continue;
                };
                if join_surname_particle(n.surname_prefix.as_deref(), &root) != value {
                    continue;
                }
                if trimmed(n.surname_prefix.as_deref()) == new_prefix && root == new_surname {
                    continue;
                }

                let person_id = n.person_id;
                person_name::ActiveModel {
                    id: Unchanged(n.id),
                    surname: Set(Some(new_surname.clone())),
                    surname_prefix: Set(new_prefix.clone()),
                    updated_at: Set(Utc::now()),
                    ..Default::default()
                }
                .update(db)
                .await
                .map_err(|e| OxidGeneError::Database(e.to_string()))?;

                updated_persons.insert(person_id);
                names_updated += 1;
            }
        }

        Ok(FamilyNameParticleUpdate {
            value: value.to_string(),
            surname_prefix: new_prefix,
            surname: new_surname,
            names_updated,
            persons_updated: updated_persons.len(),
        })
    }

    /// Resolve a batch of person IDs (as returned by the `*_usage_person_ids`
    /// queries above) into display name parts + birth/death years, in bulk —
    /// avoids one HTTP round trip per person on the dictionary usage panel.
    /// Sorted by given name, matching how the panel lists people.
    pub async fn resolve_person_usage_entries(
        db: &impl ConnectionTrait,
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
                Condition::any()
                    .add(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Birth)))
                    .add(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Death))),
            )
            .all(db)
            .await
            .map_err(|e| OxidGeneError::Database(e.to_string()))?;
        let mut birth_by_person: HashMap<Uuid, (i32, DateQualifier)> = HashMap::new();
        let mut death_by_person: HashMap<Uuid, (i32, DateQualifier)> = HashMap::new();
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
            bucket
                .entry(pid)
                .or_insert((year, DateQualifier::from(e.date_qualifier)));
        }

        let mut out: Vec<PersonUsageEntry> = person_ids
            .iter()
            .map(|&person_id| {
                let name = name_by_person.get(&person_id);
                PersonUsageEntry {
                    person_id,
                    given_names: name.and_then(|n| trimmed(n.given_names.as_deref())),
                    // Full surname: this feeds a display list, not a filing key.
                    surname: name.and_then(|n| {
                        trimmed(n.surname.as_deref())
                            .map(|root| join_surname_particle(n.surname_prefix.as_deref(), &root))
                    }),
                    birth_year: birth_by_person.get(&person_id).map(|(y, _)| *y),
                    birth_qualifier: birth_by_person
                        .get(&person_id)
                        .map(|(_, q)| *q)
                        .unwrap_or_default(),
                    death_year: death_by_person.get(&person_id).map(|(y, _)| *y),
                    death_qualifier: death_by_person
                        .get(&person_id)
                        .map(|(_, q)| *q)
                        .unwrap_or_default(),
                }
            })
            .collect();
        out.sort_by_cached_key(|p| p.given_names.as_deref().unwrap_or("").to_lowercase());
        Ok(out)
    }
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Sorted entries whose filing key is just the value itself — correct for
/// every dictionary except family names, which file under the surname root.
fn sorted_entries(per_value: HashMap<String, HashSet<Uuid>>) -> Vec<DictionaryValueEntry> {
    sorted_entries_with(per_value, |value| value.to_lowercase())
}

fn sorted_entries_with(
    per_value: HashMap<String, HashSet<Uuid>>,
    sort_key: impl Fn(&str) -> String,
) -> Vec<DictionaryValueEntry> {
    let mut out: Vec<DictionaryValueEntry> = per_value
        .into_iter()
        .map(|(value, ids)| DictionaryValueEntry {
            sort_key: sort_key(&value),
            value,
            count: ids.len() as i64,
        })
        .collect();
    out.sort_by_cached_key(|a| a.value.to_lowercase());
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
