//! Aggregated "dictionary" queries: distinct values entered across a tree
//! (family names, occupations) or existing entities (sources, places) paired
//! with how many persons/events reference them, plus drill-down lookups
//! resolving a value back to the persons that carry it.
//!
//! Plus the page's two bulk family-name edits:
//! [`DictionaryRepo::set_family_name_particle`], which re-cuts every
//! occurrence of one surname at once, and [`DictionaryRepo::rename_family_name`],
//! which gives every person carrying it as their main name another one. They
//! live here rather than in `PersonNameRepo` because "which rows are this
//! dictionary entry" is defined by [`DictionaryRepo::family_names`] right above
//! them, and all of them must agree on the answer: a row belongs to the entry
//! spelled exactly like its full surname, particle included.

use chrono::Utc;
use oxidgene_core::collections::sorted_unique;
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
use sea_orm::{ActiveValue::Set, Condition, JoinType, QuerySelect, Unchanged};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

use crate::entities::{
    citation, event, family_spouse, media, media_link, person, person_name, place, sea_enums,
    source, vignette,
};
use crate::repo::batch::in_chunks;
use crate::repo::db_err;

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
    /// For family names only: how many of those persons carry the value as
    /// their primary name — the ones a rename would reach, since it leaves
    /// alias, married and other names alone. `None` for every other
    /// dictionary.
    pub primary_count: Option<i64>,
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
    /// Those persons, for whoever records what the re-cut changed.
    pub person_ids: Vec<Uuid>,
}

/// Outcome of [`DictionaryRepo::rename_family_name`].
#[derive(Debug, Clone)]
pub struct FamilyNameRename {
    /// The surname renamed, as it was listed.
    pub value: String,
    /// The surname its carriers now bear, as it will be listed.
    pub new_value: String,
    /// The particle the renamed rows now store, `None` for none.
    pub surname_prefix: Option<String>,
    /// The root the renamed rows now store, i.e. what the name files under.
    pub surname: String,
    /// `person_name` rows rewritten.
    pub names_updated: usize,
    /// Distinct persons behind `names_updated`.
    pub persons_updated: usize,
    /// `new_value` was already in the dictionary: the renamed rows joined it.
    pub merged: bool,
    /// The persons rewritten, for whoever records and refreshes what changed.
    pub person_ids: Vec<Uuid>,
}

/// Above this many sources matching a prefix, the Sources tab's smart
/// drill-down (see `DictionaryRepo::resolve_source_drill_down` and
/// ui-dictionary.md §8) shows further branch choices instead of the final
/// flat list.
pub const SOURCE_DRILL_THRESHOLD: i64 = 250;

pub struct DictionaryRepo;

/// Every name of every live person in a tree, read through the tree itself
/// rather than a list of its person ids — a list that long cannot be bound
/// into one query once a tree passes about 32 000 persons.
async fn tree_names(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<Vec<person_name::Model>, OxidGeneError> {
    person_name::Entity::find()
        .join(JoinType::InnerJoin, person_name::Relation::Person.def())
        .filter(person::Column::TreeId.eq(tree_id))
        .filter(person::Column::DeletedAt.is_null())
        .all(db)
        .await
        .map_err(db_err)
}

impl DictionaryRepo {
    /// Distinct surnames across all persons in a tree, with the number of
    /// persons carrying each (as entered — no accent-folding/normalization).
    pub async fn family_names(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
        // Only the columns the count needs: this reads one row per name in
        // the tree.
        let names: Vec<(Uuid, Option<String>, Option<String>, bool)> = person_name::Entity::find()
            .select_only()
            .column(person_name::Column::PersonId)
            .column(person_name::Column::Surname)
            .column(person_name::Column::SurnamePrefix)
            .column(person_name::Column::IsPrimary)
            .join(JoinType::InnerJoin, person_name::Relation::Person.def())
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(person_name::Column::Surname.is_not_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;

        // Group by person, not by row: a person with two `PersonName` entries
        // sharing the same surname (e.g. birth + nickname) must count once.
        //
        // Keyed on the full surname exactly as spelled, particle included:
        // "de la Cruz" and "Cruz" are two different families and stay two
        // entries, and so do "Martin" and "MARTIN" — unifying spellings is
        // the rename's job, not the listing's. The particle only affects
        // where each one *files*, which is what `sort_key` carries.
        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        let mut primary: HashMap<String, HashSet<Uuid>> = HashMap::new();
        let mut roots: HashMap<String, BTreeMap<String, usize>> = HashMap::new();
        for (person_id, surname, surname_prefix, is_primary) in names {
            let Some((full, root)) = full_surname(surname_prefix.as_deref(), surname.as_deref())
            else {
                continue;
            };
            *roots
                .entry(full.clone())
                .or_default()
                .entry(root)
                .or_default() += 1;
            if is_primary {
                primary.entry(full.clone()).or_default().insert(person_id);
            }
            per_value.entry(full).or_default().insert(person_id);
        }
        let mut entries = sorted_entries_with(per_value, |value| {
            // Rows of one entry may be cut differently; file it where most
            // are, the same cut a rename into this entry adopts.
            roots
                .get(value)
                .and_then(dominant)
                .map_or_else(|| value.to_lowercase(), |root| root.to_lowercase())
        });
        for entry in &mut entries {
            entry.primary_count = Some(primary.get(&entry.value).map_or(0, |ids| ids.len() as i64));
        }
        Ok(entries)
    }

    /// Distinct given names across a tree, one per word of the given-names
    /// field ("Jean Marie" holds "Jean" and "Marie"), with the number of
    /// persons carrying each.
    pub async fn given_names(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
        let names: Vec<(Uuid, Option<String>)> = person_name::Entity::find()
            .select_only()
            .column(person_name::Column::PersonId)
            .column(person_name::Column::GivenNames)
            .join(JoinType::InnerJoin, person_name::Relation::Person.def())
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(person_name::Column::GivenNames.is_not_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;

        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for (person_id, given_names) in names {
            for word in given_names
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
            {
                per_value
                    .entry(word.to_string())
                    .or_default()
                    .insert(person_id);
            }
        }
        Ok(sorted_entries(per_value))
    }

    /// Distinct occupation labels (`Event.description` for `Occupation`
    /// events) across a tree, with the number of persons holding each.
    pub async fn occupations(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryValueEntry>, OxidGeneError> {
        let labels: Vec<(Option<String>, Option<Uuid>)> = event::Entity::find()
            .select_only()
            .column(event::Column::Description)
            .column(event::Column::PersonId)
            .filter(event::Column::TreeId.eq(tree_id))
            .filter(event::Column::DeletedAt.is_null())
            .filter(event::Column::EventType.eq(sea_enums::EventType::from(EventType::Occupation)))
            .filter(event::Column::PersonId.is_not_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;

        // Group by person: the same label recorded on two occupation events
        // for one person (e.g. at different life stages) must count once.
        let mut per_value: HashMap<String, HashSet<Uuid>> = HashMap::new();
        for (description, person_id) in labels {
            if let (Some(label), Some(pid)) = (trimmed(description.as_deref()), person_id) {
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
            .map_err(db_err)?;

        let mut counts: HashMap<Uuid, i64> = HashMap::new();
        if !sources.is_empty() {
            let cited: Vec<Uuid> = citation::Entity::find()
                .select_only()
                .column(citation::Column::SourceId)
                .join(JoinType::InnerJoin, citation::Relation::Source.def())
                .filter(source::Column::TreeId.eq(tree_id))
                .filter(source::Column::DeletedAt.is_null())
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            for source_id in cited {
                *counts.entry(source_id).or_insert(0) += 1;
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
            .map_err(db_err)?;

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
            .map_err(db_err)?;

        // Places belong to one tree, so the tree's own events and media are
        // every use there can be. The database counts them: one row per
        // used place comes back, not one per event.
        let mut counts: HashMap<Uuid, i64> = HashMap::new();
        if !places.is_empty() {
            let event_counts: Vec<(Option<Uuid>, i64)> = event::Entity::find()
                .select_only()
                .column(event::Column::PlaceId)
                .column_as(event::Column::Id.count(), "uses")
                .filter(event::Column::TreeId.eq(tree_id))
                .filter(event::Column::PlaceId.is_not_null())
                .filter(event::Column::DeletedAt.is_null())
                .group_by(event::Column::PlaceId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            let media_counts: Vec<(Option<Uuid>, i64)> = media::Entity::find()
                .select_only()
                .column(media::Column::PlaceId)
                .column_as(media::Column::Id.count(), "uses")
                .filter(media::Column::TreeId.eq(tree_id))
                .filter(media::Column::PlaceId.is_not_null())
                .filter(media::Column::DeletedAt.is_null())
                .group_by(media::Column::PlaceId)
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            for (pid, uses) in event_counts.into_iter().chain(media_counts) {
                if let Some(pid) = pid {
                    *counts.entry(pid).or_insert(0) += uses;
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
            .map_err(db_err)?;

        let mut event_ids = Vec::new();
        let mut person_ids: Vec<Uuid> = Vec::new();
        for c in &citations {
            if let Some(pid) = c.person_id {
                person_ids.push(pid);
            } else if let Some(eid) = c.event_id {
                event_ids.push(eid);
            }
        }

        let events = in_chunks(&event_ids, |chunk| async move {
            event::Entity::find()
                .filter(event::Column::Id.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        person_ids.extend(events.into_iter().filter_map(|e| e.person_id));

        Ok(sorted_unique(person_ids))
    }

    /// Distinct live persons a place concerns: those whose own events take
    /// place there, the spouses of the couples whose events do, and the
    /// persons a media filed there — or a page of one — is attached to: by a
    /// media link to them, to one of their events or to one of their couples,
    /// or by a crop identifying them. The same uses the place's count counts
    /// (its events and media), so a place with uses never lists nobody.
    pub async fn place_usage_person_ids(
        db: &impl ConnectionTrait,
        place_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        let mut persons: Vec<Uuid> = Vec::new();
        let mut families: Vec<Uuid> = Vec::new();
        let mut event_ids: Vec<Uuid> = Vec::new();

        let events = event::Entity::find()
            .filter(event::Column::PlaceId.eq(place_id))
            .filter(event::Column::DeletedAt.is_null())
            .all(db)
            .await
            .map_err(db_err)?;
        for e in events {
            persons.extend(e.person_id);
            families.extend(e.family_id);
        }

        // Media filed at the place, and their pages, which carry the links
        // as often as the document does.
        let documents: Vec<Uuid> = media::Entity::find()
            .select_only()
            .column(media::Column::Id)
            .filter(media::Column::PlaceId.eq(place_id))
            .filter(media::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        let mut media_ids = documents.clone();
        media_ids.extend(
            in_chunks(&documents, |chunk| async move {
                media::Entity::find()
                    .select_only()
                    .column(media::Column::Id)
                    .filter(media::Column::ParentMediaId.is_in(chunk))
                    .filter(media::Column::DeletedAt.is_null())
                    .into_tuple::<Uuid>()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?,
        );
        let links = in_chunks(&media_ids, |chunk| async move {
            media_link::Entity::find()
                .filter(media_link::Column::MediaId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        for link in links {
            persons.extend(link.person_id);
            families.extend(link.family_id);
            event_ids.extend(link.event_id);
        }
        let crops = in_chunks(&media_ids, |chunk| async move {
            vignette::Entity::find()
                .filter(vignette::Column::MediaId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        for crop in crops {
            persons.extend(crop.person_id);
            event_ids.extend(crop.event_id);
        }

        // The events a media is linked to: their person, or their couple.
        let linked = in_chunks(&event_ids, |chunk| async move {
            event::Entity::find()
                .filter(event::Column::Id.is_in(chunk))
                .filter(event::Column::DeletedAt.is_null())
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        for e in linked {
            persons.extend(e.person_id);
            families.extend(e.family_id);
        }

        // A couple is its spouses.
        let spouses = in_chunks(&sorted_unique(families), |chunk| async move {
            family_spouse::Entity::find()
                .filter(family_spouse::Column::FamilyId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        persons.extend(spouses.into_iter().map(|s| s.person_id));

        let persons = sorted_unique(persons);
        let live: HashSet<Uuid> = in_chunks(&persons, |chunk| async move {
            person::Entity::find()
                .select_only()
                .column(person::Column::Id)
                .filter(person::Column::Id.is_in(chunk))
                .filter(person::Column::DeletedAt.is_null())
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?
        .into_iter()
        .collect();
        Ok(persons.into_iter().filter(|id| live.contains(id)).collect())
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
            .map_err(db_err)?;

        Ok(sorted_unique(
            events.into_iter().filter_map(|e| e.person_id),
        ))
    }

    /// Distinct persons carrying a given surname in a tree.
    pub async fn family_name_usage_person_ids(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        // `value` comes from `family_names`, which reports full surnames, so
        // it may carry a particle while the column holds only the root. Where
        // the name is cut is not re-detected — a cut corrected by hand differs
        // from detection, which is what the correction was for. Every cut the
        // value allows is a candidate root instead, and the full surname is
        // re-checked in memory, so that "Cruz" and "de la Cruz" resolve to
        // their own people rather than to each other's.
        let value = value.trim();
        let names = person_name::Entity::find()
            .join(JoinType::InnerJoin, person_name::Relation::Person.def())
            .filter(person::Column::TreeId.eq(tree_id))
            .filter(person::Column::DeletedAt.is_null())
            .filter(person_name::Column::Surname.is_in(root_candidates(value)))
            .all(db)
            .await
            .map_err(db_err)?;

        Ok(sorted_unique(
            names
                .into_iter()
                .filter(|n| is_spelled(n, value))
                .map(|n| n.person_id),
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

        let rows: Vec<person_name::Model> = tree_names(db, tree_id)
            .await?
            .into_iter()
            .filter(|n| is_spelled(n, value))
            .collect();
        let (names_updated, person_ids) =
            rewrite_surnames(db, &rows, new_prefix.as_deref(), &new_surname).await?;

        Ok(FamilyNameParticleUpdate {
            value: value.to_string(),
            surname_prefix: new_prefix,
            surname: new_surname,
            names_updated,
            persons_updated: person_ids.len(),
            person_ids,
        })
    }

    /// Give every person whose primary name carries surname `value` the
    /// surname `new_value` instead.
    ///
    /// `value` is a surname as listed by [`Self::family_names`], matched
    /// exactly — particle included and case included, so "Martin" and
    /// "MARTIN" are two names, and renaming one is how they are unified.
    /// Only primary names are rewritten: an alias, a married name or any other
    /// name carrying `value` keeps it, so `value` may stay listed for them.
    ///
    /// `new_value` is stored as given. Where it splits between particle and
    /// root comes from, in order:
    ///
    /// 1. `particle`, when given — empty meaning none. It must be at the head
    ///    of `new_value`, as for [`Self::set_family_name_particle`], and it
    ///    then also re-cuts every row already carrying `new_value`, so the
    ///    entry never holds two cuts at once;
    /// 2. the cut `new_value` already has in the tree, when it is listed —
    ///    the renamed rows then merge into that entry as it stands;
    /// 3. particle detection otherwise.
    ///
    /// Renaming a name to itself changes nothing and reports zero rows.
    ///
    /// # Errors
    ///
    /// Returns [`OxidGeneError::Validation`] if `value` or `new_value` is
    /// blank or `particle` is not at the head of `new_value`, and
    /// [`OxidGeneError::Database`] on query failure.
    pub async fn rename_family_name(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        value: &str,
        new_value: &str,
        particle: Option<&str>,
    ) -> Result<FamilyNameRename, OxidGeneError> {
        let value = value.trim();
        let new_value = new_value.trim();
        if value.is_empty() || new_value.is_empty() {
            return Err(OxidGeneError::Validation(
                "a family name is required".to_string(),
            ));
        }
        let explicit = match particle {
            Some(particle) => {
                Some(split_surname_at_head(new_value, particle).ok_or_else(|| {
                    OxidGeneError::Validation(format!(
                        "particle \"{particle}\" is not at the head of surname \"{new_value}\""
                    ))
                })?)
            }
            None => None,
        };

        let mut carriers: Vec<person_name::Model> = Vec::new();
        let mut target: Vec<person_name::Model> = Vec::new();
        for n in tree_names(db, tree_id).await? {
            if is_spelled(&n, new_value) {
                target.push(n.clone());
            }
            if n.is_primary && is_spelled(&n, value) {
                carriers.push(n);
            }
        }

        let explicit_cut = explicit.is_some();
        let (new_prefix, new_surname) = match explicit {
            Some(cut) => cut,
            None => existing_cut(&target).unwrap_or_else(|| split_surname_particle(new_value)),
        };
        let unchanged = value == new_value;
        let merged = !unchanged && !target.is_empty();

        let (names_updated, person_ids) = if unchanged {
            (0, Vec::new())
        } else {
            let mut rows = carriers;
            if explicit_cut {
                rows.extend(target);
            }
            rewrite_surnames(db, &rows, new_prefix.as_deref(), &new_surname).await?
        };

        Ok(FamilyNameRename {
            value: value.to_string(),
            new_value: new_value.to_string(),
            surname_prefix: new_prefix,
            surname: new_surname,
            names_updated,
            persons_updated: person_ids.len(),
            merged,
            person_ids,
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

        let names = in_chunks(person_ids, |chunk| async move {
            person_name::Entity::find()
                .filter(person_name::Column::PersonId.is_in(chunk))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
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

        let events = in_chunks(person_ids, |chunk| async move {
            event::Entity::find()
                .filter(event::Column::PersonId.is_in(chunk))
                .filter(event::Column::DeletedAt.is_null())
                .filter(
                    Condition::any()
                        .add(
                            event::Column::EventType
                                .eq(sea_enums::EventType::from(EventType::Birth)),
                        )
                        .add(
                            event::Column::EventType
                                .eq(sea_enums::EventType::from(EventType::Death)),
                        ),
                )
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
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

/// A name row's full surname, particle included — the dictionary entry it
/// belongs to — and its root. `None` for a row without a surname.
fn full_surname(prefix: Option<&str>, surname: Option<&str>) -> Option<(String, String)> {
    let root = trimmed(surname)?;
    Some((join_surname_particle(prefix, &root), root))
}

/// Whether a name row belongs to the dictionary entry spelled `value`.
fn is_spelled(n: &person_name::Model, value: &str) -> bool {
    full_surname(n.surname_prefix.as_deref(), n.surname.as_deref())
        .is_some_and(|(full, _)| full == value)
}

/// Every root a row listed as `value` can store: the whole value, or what
/// follows a space or an apostrophe — where a particle can end.
fn root_candidates(value: &str) -> Vec<String> {
    let mut roots = vec![value.to_string()];
    for (at, c) in value.char_indices() {
        if c.is_whitespace() || c == '\'' || c == '\u{2019}' {
            let rest = value[at + c.len_utf8()..].trim_start();
            if !rest.is_empty() {
                roots.push(rest.to_string());
            }
        }
    }
    roots.dedup();
    roots
}

/// The key counted most often; the greatest of those on a tie, so the answer
/// does not depend on the order rows were read in.
fn dominant(counts: &BTreeMap<String, usize>) -> Option<&String> {
    counts
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(key, _)| key)
}

/// How rows of one dictionary entry are cut, as `(particle, root)` — the cut
/// most of them have, see [`dominant`]. `None` without rows.
fn existing_cut(rows: &[person_name::Model]) -> Option<(Option<String>, String)> {
    let mut roots: BTreeMap<String, usize> = BTreeMap::new();
    for n in rows {
        if let Some(root) = trimmed(n.surname.as_deref()) {
            *roots.entry(root).or_default() += 1;
        }
    }
    let root = dominant(&roots)?;
    let n = rows
        .iter()
        .find(|n| trimmed(n.surname.as_deref()).as_ref() == Some(root))?;
    Some((trimmed(n.surname_prefix.as_deref()), root.clone()))
}

/// Write `prefix` + `root` into every row of `rows`, skipping rows already
/// written that way — a repeated edit is then a no-op, not an `updated_at`
/// bump. Returns the number of rows written and their persons, sorted.
async fn rewrite_surnames(
    db: &impl ConnectionTrait,
    rows: &[person_name::Model],
    prefix: Option<&str>,
    root: &str,
) -> Result<(usize, Vec<Uuid>), OxidGeneError> {
    let mut persons: HashSet<Uuid> = HashSet::new();
    let mut names_updated = 0usize;
    for n in rows {
        if trimmed(n.surname_prefix.as_deref()).as_deref() == prefix
            && trimmed(n.surname.as_deref()).as_deref() == Some(root)
        {
            continue;
        }
        person_name::ActiveModel {
            id: Unchanged(n.id),
            surname: Set(Some(root.to_string())),
            surname_prefix: Set(prefix.map(str::to_string)),
            updated_at: Set(Utc::now()),
            ..Default::default()
        }
        .update(db)
        .await
        .map_err(db_err)?;
        persons.insert(n.person_id);
        names_updated += 1;
    }
    let mut persons: Vec<Uuid> = persons.into_iter().collect();
    persons.sort();
    Ok((names_updated, persons))
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
            primary_count: None,
        })
        .collect();
    out.sort_by_cached_key(|a| a.value.to_lowercase());
    out
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
