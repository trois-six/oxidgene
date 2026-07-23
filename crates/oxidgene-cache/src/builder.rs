//! Cache builder — constructs cache entries from database data.
//!
//! The builder fetches raw entities from the database and assembles
//! them into denormalized cache structures (CachedPerson, SearchEntry, etc.).

use crate::types::*;
use chrono::Utc;
use oxidgene_core::enums::*;

use oxidgene_core::types::{
    Event, FamilyChild, FamilySpouse, Media, MediaLink, Note, Person, PersonName, Place,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Holds all raw data for a tree, used to build cache entries efficiently.
///
/// This is populated once (in parallel) and then used to build all cache entries
/// without additional database calls.
pub struct TreeData {
    pub persons: Vec<Person>,
    pub names: Vec<PersonName>,
    pub events: Vec<Event>,
    pub places: Vec<Place>,
    pub spouses: Vec<FamilySpouse>,
    pub children: Vec<FamilyChild>,
    pub media: Vec<Media>,
    pub media_links: Vec<MediaLink>,
    pub notes: Vec<Note>,
}

/// Pre-indexed tree data for efficient cache building.
struct IndexedData {
    /// PersonName entries grouped by person_id
    names_by_person: HashMap<Uuid, Vec<PersonName>>,
    /// Events grouped by person_id (individual events)
    events_by_person: HashMap<Uuid, Vec<Event>>,
    /// Events grouped by family_id (family events)
    events_by_family: HashMap<Uuid, Vec<Event>>,
    /// Place indexed by place_id
    places_by_id: HashMap<Uuid, Place>,
    /// FamilySpouse entries grouped by family_id
    spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
    /// FamilyChild entries grouped by family_id
    children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
    /// FamilySpouse entries grouped by person_id (families where person is a spouse)
    families_by_spouse: HashMap<Uuid, Vec<FamilySpouse>>,
    /// FamilyChild entries grouped by person_id (family where person is a child)
    family_by_child: HashMap<Uuid, Vec<FamilyChild>>,
    /// MediaLink entries grouped by person_id
    media_links_by_person: HashMap<Uuid, Vec<MediaLink>>,
    /// Media indexed by media_id
    media_by_id: HashMap<Uuid, Media>,
    /// Note count by person_id
    note_count_by_person: HashMap<Uuid, u32>,
    /// Primary name display string by person_id (for cross-references)
    display_names: HashMap<Uuid, String>,
    /// Person sex by person_id
    sex_by_person: HashMap<Uuid, Sex>,
}

impl IndexedData {
    fn new(data: &TreeData) -> Self {
        // Index names by person
        let mut names_by_person: HashMap<Uuid, Vec<PersonName>> = HashMap::new();
        let mut display_names: HashMap<Uuid, String> = HashMap::new();
        for name in &data.names {
            names_by_person
                .entry(name.person_id)
                .or_default()
                .push(name.clone());
            if name.is_primary {
                display_names.insert(name.person_id, name.display_name());
            }
        }

        // Index events by person and family
        let mut events_by_person: HashMap<Uuid, Vec<Event>> = HashMap::new();
        let mut events_by_family: HashMap<Uuid, Vec<Event>> = HashMap::new();
        for event in &data.events {
            if let Some(pid) = event.person_id {
                events_by_person.entry(pid).or_default().push(event.clone());
            }
            if let Some(fid) = event.family_id {
                events_by_family.entry(fid).or_default().push(event.clone());
            }
        }

        // Index places
        let places_by_id: HashMap<Uuid, Place> =
            data.places.iter().map(|p| (p.id, p.clone())).collect();

        // Index spouses by family
        let mut spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>> = HashMap::new();
        let mut families_by_spouse: HashMap<Uuid, Vec<FamilySpouse>> = HashMap::new();
        for spouse in &data.spouses {
            spouses_by_family
                .entry(spouse.family_id)
                .or_default()
                .push(spouse.clone());
            families_by_spouse
                .entry(spouse.person_id)
                .or_default()
                .push(spouse.clone());
        }

        // Index children by family and by person
        let mut children_by_family: HashMap<Uuid, Vec<FamilyChild>> = HashMap::new();
        let mut family_by_child: HashMap<Uuid, Vec<FamilyChild>> = HashMap::new();
        for child in &data.children {
            children_by_family
                .entry(child.family_id)
                .or_default()
                .push(child.clone());
            family_by_child
                .entry(child.person_id)
                .or_default()
                .push(child.clone());
        }

        // Index media links by person
        let mut media_links_by_person: HashMap<Uuid, Vec<MediaLink>> = HashMap::new();
        for link in &data.media_links {
            if let Some(pid) = link.person_id {
                media_links_by_person
                    .entry(pid)
                    .or_default()
                    .push(link.clone());
            }
        }

        // Index media by ID
        let media_by_id: HashMap<Uuid, Media> =
            data.media.iter().map(|m| (m.id, m.clone())).collect();

        // Count notes by person
        let mut note_count_by_person: HashMap<Uuid, u32> = HashMap::new();
        for note in &data.notes {
            if let Some(pid) = note.person_id {
                *note_count_by_person.entry(pid).or_default() += 1;
            }
        }

        // Index sex by person
        let sex_by_person: HashMap<Uuid, Sex> =
            data.persons.iter().map(|p| (p.id, p.sex)).collect();

        Self {
            names_by_person,
            events_by_person,
            events_by_family,
            places_by_id,
            spouses_by_family,
            children_by_family,
            families_by_spouse,
            family_by_child,
            media_links_by_person,
            media_by_id,
            note_count_by_person,
            display_names,
            sex_by_person,
        }
    }
}

/// Build a `CachedEvent` from a raw `Event` and the place index.
fn build_cached_event(event: &Event, places: &HashMap<Uuid, Place>) -> CachedEvent {
    let place_name = event
        .place_id
        .and_then(|pid| places.get(&pid))
        .map(|p| p.name.clone());

    CachedEvent {
        event_id: event.id,
        event_type: event.event_type,
        date_value: event.date_value.clone(),
        date_sort: event.date_sort,
        place_name,
        place_id: event.place_id,
        description: event.description.clone(),
    }
}

/// Extract a year string from a `CachedEvent` for display.
///
/// Tries `date_sort` first (formatted as "YYYY"), then falls back to
/// extracting a 4-digit year from `date_value`.
pub fn extract_year(event: &CachedEvent) -> Option<String> {
    oxidgene_core::types::year_from_date(event.date_sort, event.date_value.as_deref())
        .map(|y| format!("{y:04}"))
}

/// Build all `CachedPerson` entries for an entire tree.
pub fn build_all_persons(tree_id: Uuid, data: &TreeData) -> Vec<CachedPerson> {
    let idx = IndexedData::new(data);
    let now = Utc::now();

    data.persons
        .iter()
        .map(|person| build_one_person(person, tree_id, &idx, now))
        .collect()
}

/// Build a single `CachedPerson` from (possibly targeted) tree data.
///
/// `data` only needs to contain the person, their relatives (spouses, parents,
/// children) and the entities attached to them — see
/// `CacheService::fetch_person_data`. Returns `None` if the person is not in
/// `data.persons`.
pub fn build_person(tree_id: Uuid, person_id: Uuid, data: &TreeData) -> Option<CachedPerson> {
    let person = data.persons.iter().find(|p| p.id == person_id)?;
    let idx = IndexedData::new(data);
    Some(build_one_person(person, tree_id, &idx, Utc::now()))
}

/// Build a single `CachedPerson` from indexed data.
fn build_one_person(
    person: &Person,
    tree_id: Uuid,
    idx: &IndexedData,
    now: chrono::DateTime<Utc>,
) -> CachedPerson {
    let pid = person.id;

    // ── Names ────────────────────────────────────────────────────────────
    let names = idx.names_by_person.get(&pid).cloned().unwrap_or_default();
    let mut primary_name: Option<CachedName> = None;
    let mut other_names: Vec<CachedName> = Vec::new();

    for name in &names {
        let cached = CachedName {
            name_id: name.id,
            name_type: name.name_type,
            display_name: name.display_name(),
            given_names: name.given_names.clone(),
            surname: name.surname.clone(),
        };
        if name.is_primary {
            primary_name = Some(cached);
        } else {
            other_names.push(cached);
        }
    }

    // ── Events ───────────────────────────────────────────────────────────
    let events = idx.events_by_person.get(&pid).cloned().unwrap_or_default();
    let mut birth: Option<CachedEvent> = None;
    let mut death: Option<CachedEvent> = None;
    let mut baptism: Option<CachedEvent> = None;
    let mut burial: Option<CachedEvent> = None;
    let mut occupation: Option<String> = None;
    let mut other_events: Vec<CachedEvent> = Vec::new();

    for event in &events {
        let cached = build_cached_event(event, &idx.places_by_id);
        match event.event_type {
            EventType::Birth => birth = Some(cached),
            EventType::Death => death = Some(cached),
            EventType::Baptism => baptism = Some(cached),
            EventType::Burial => burial = Some(cached),
            EventType::Occupation => {
                occupation = event.description.clone();
                other_events.push(cached);
            }
            _ => other_events.push(cached),
        }
    }

    // ── Family links (as spouse) ─────────────────────────────────────────
    let spouse_entries = idx
        .families_by_spouse
        .get(&pid)
        .cloned()
        .unwrap_or_default();
    let families_as_spouse: Vec<CachedFamilyLink> = spouse_entries
        .iter()
        .map(|fs| {
            let family_id = fs.family_id;

            // Find the other spouse in this family
            let other_spouse = idx
                .spouses_by_family
                .get(&family_id)
                .and_then(|spouses| spouses.iter().find(|s| s.person_id != pid));

            let spouse_id = other_spouse.map(|s| s.person_id);
            let spouse_display_name =
                spouse_id.and_then(|sid| idx.display_names.get(&sid).cloned());
            let spouse_sex = spouse_id.and_then(|sid| idx.sex_by_person.get(&sid).copied());

            // Collect all family events (marriage, divorce, annulment, etc.)
            let all_family_events: Vec<CachedEvent> = idx
                .events_by_family
                .get(&family_id)
                .map(|events| {
                    events
                        .iter()
                        .map(|e| build_cached_event(e, &idx.places_by_id))
                        .collect()
                })
                .unwrap_or_default();

            // Find marriage event for this family
            let marriage = all_family_events
                .iter()
                .find(|e| e.event_type == EventType::Marriage)
                .cloned();

            // Children in this family
            let family_children = idx
                .children_by_family
                .get(&family_id)
                .cloned()
                .unwrap_or_default();
            let children_ids: Vec<Uuid> = family_children.iter().map(|c| c.person_id).collect();
            let children_count = children_ids.len() as u32;

            CachedFamilyLink {
                family_id,
                role: fs.role,
                spouse_id,
                spouse_display_name,
                spouse_sex,
                marriage,
                events: all_family_events,
                children_ids,
                children_count,
            }
        })
        .collect();

    // ── Family link (as child) ───────────────────────────────────────────
    let family_as_child = idx
        .family_by_child
        .get(&pid)
        .and_then(|entries| entries.first())
        .map(|fc| {
            let family_id = fc.family_id;
            let parents = idx
                .spouses_by_family
                .get(&family_id)
                .cloned()
                .unwrap_or_default();

            let mut father_id: Option<Uuid> = None;
            let mut father_display_name: Option<String> = None;
            let mut mother_id: Option<Uuid> = None;
            let mut mother_display_name: Option<String> = None;

            for parent in &parents {
                let sex = idx.sex_by_person.get(&parent.person_id).copied();
                let name = idx.display_names.get(&parent.person_id).cloned();
                match (parent.role, sex) {
                    (SpouseRole::Husband, _) | (SpouseRole::Partner, Some(Sex::Male)) => {
                        father_id = Some(parent.person_id);
                        father_display_name = name;
                    }
                    (SpouseRole::Wife, _) | (SpouseRole::Partner, Some(Sex::Female)) => {
                        mother_id = Some(parent.person_id);
                        mother_display_name = name;
                    }
                    _ => {
                        // For unknown sex partner, assign to first empty slot
                        if father_id.is_none() {
                            father_id = Some(parent.person_id);
                            father_display_name = name;
                        } else if mother_id.is_none() {
                            mother_id = Some(parent.person_id);
                            mother_display_name = name;
                        }
                    }
                }
            }

            CachedChildLink {
                family_id,
                child_type: fc.child_type,
                father_id,
                father_display_name,
                mother_id,
                mother_display_name,
            }
        });

    // ── Media ────────────────────────────────────────────────────────────
    let person_media_links = idx
        .media_links_by_person
        .get(&pid)
        .cloned()
        .unwrap_or_default();
    let media_count = person_media_links.len() as u32;

    // Find the primary media (first by sort_order)
    let primary_media = person_media_links
        .iter()
        .min_by_key(|ml| ml.sort_order)
        .and_then(|ml| idx.media_by_id.get(&ml.media_id))
        .map(|m| CachedMediaRef {
            media_id: m.id,
            file_path: m.file_path.clone(),
            mime_type: m.mime_type.clone(),
            title: m.title.clone(),
        });

    // ── Citation count ───────────────────────────────────────────────────
    // Citations reference persons directly via person_id, but we don't have
    // a tree-wide citation list indexed by person. For now, store 0 and
    // fill in when we add citation batch loading to TreeData.
    let citation_count = 0;

    // ── Note count ───────────────────────────────────────────────────────
    let note_count = idx.note_count_by_person.get(&pid).copied().unwrap_or(0);

    CachedPerson {
        person_id: pid,
        tree_id,
        sex: person.sex,
        primary_name,
        other_names,
        birth,
        death,
        baptism,
        burial,
        occupation,
        other_events,
        families_as_spouse,
        family_as_child,
        primary_media,
        media_count,
        citation_count,
        note_count,
        updated_at: person.updated_at,
        cached_at: now,
    }
}

/// Build a `SearchEntry` from a `CachedPerson`.
pub fn build_search_entry(person: &CachedPerson) -> SearchEntry {
    let display_name = person
        .primary_name
        .as_ref()
        .map(|n| n.display_name.clone())
        .unwrap_or_default();

    let surname = person
        .primary_name
        .as_ref()
        .and_then(|n| n.surname.clone())
        .unwrap_or_default();

    let given_names = person
        .primary_name
        .as_ref()
        .and_then(|n| n.given_names.clone())
        .unwrap_or_default();

    // Look for a maiden name
    let maiden_name = person
        .other_names
        .iter()
        .find(|n| n.name_type == NameType::Maiden)
        .and_then(|n| n.surname.clone());

    SearchEntry {
        person_id: person.person_id,
        sex: person.sex,
        surname_normalized: normalize_for_search(&surname),
        given_names_normalized: normalize_for_search(&given_names),
        maiden_name_normalized: maiden_name.as_deref().map(normalize_for_search),
        display_name,
        birth_year: person.birth.as_ref().and_then(extract_year),
        birth_place: person.birth.as_ref().and_then(|e| e.place_name.clone()),
        death_year: person.death.as_ref().and_then(extract_year),
        date_sort: person.birth.as_ref().and_then(|e| e.date_sort),
    }
}

/// Build a `person_search_fts` row from a `CachedPerson` (Sprint E.6).
///
/// This is the write model for the DB-native search table which replaced the
/// in-memory `CachedSearchIndex`.
pub fn build_db_search_entry(person: &CachedPerson) -> oxidgene_db::repo::PersonSearchEntry {
    let entry = build_search_entry(person);
    oxidgene_db::repo::PersonSearchEntry {
        person_id: entry.person_id,
        tree_id: person.tree_id,
        surname: entry.surname_normalized,
        given_names: entry.given_names_normalized,
        maiden_name: entry.maiden_name_normalized,
        birth_year: entry.birth_year,
        death_year: entry.death_year,
        sex: entry.sex.to_string(),
        display_name: entry.display_name,
        birth_place: entry.birth_place,
        date_sort: entry.date_sort.map(|d| d.format("%Y-%m-%d").to_string()),
    }
}

/// Convert a `person_search_fts` row back into the `SearchEntry` API shape.
pub fn search_entry_from_db(row: oxidgene_db::repo::PersonSearchEntry) -> SearchEntry {
    let sex = match row.sex.as_str() {
        "male" => Sex::Male,
        "female" => Sex::Female,
        _ => Sex::Unknown,
    };
    SearchEntry {
        person_id: row.person_id,
        sex,
        surname_normalized: row.surname,
        given_names_normalized: row.given_names,
        maiden_name_normalized: row.maiden_name,
        display_name: row.display_name,
        birth_year: row.birth_year,
        birth_place: row.birth_place,
        death_year: row.death_year,
        date_sort: row
            .date_sort
            .and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
    }
}

/// Build a `PedigreeNode` from a `CachedPerson`.
pub fn build_pedigree_node(
    person: &CachedPerson,
    generation: i32,
    sosa_number: Option<u64>,
) -> PedigreeNode {
    PedigreeNode {
        person_id: person.person_id,
        sex: person.sex,
        display_name: person
            .primary_name
            .as_ref()
            .map(|n| n.display_name.clone())
            .unwrap_or_default(),
        given_names: person
            .primary_name
            .as_ref()
            .and_then(|n| n.given_names.clone()),
        surname: person.primary_name.as_ref().and_then(|n| n.surname.clone()),
        birth_year: person.birth.as_ref().and_then(extract_year),
        birth_place: person.birth.as_ref().and_then(|e| e.place_name.clone()),
        death_year: person.death.as_ref().and_then(extract_year),
        death_place: person.death.as_ref().and_then(|e| e.place_name.clone()),
        occupation: person.occupation.clone(),
        primary_media_path: person.primary_media.as_ref().map(|m| m.file_path.clone()),
        generation,
        sosa_number,
    }
}

/// Normalize a string for search: lowercase + accent folding.
///
/// Re-exported from `oxidgene_core::search` so cache-crate callers keep
/// their existing import path.
pub use oxidgene_core::search::normalize_for_search;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year() {
        let event = CachedEvent {
            event_id: Uuid::now_v7(),
            event_type: EventType::Birth,
            date_value: Some("ABT 1842".to_string()),
            date_sort: None,
            place_name: None,
            place_id: None,
            description: None,
        };
        assert_eq!(extract_year(&event), Some("1842".to_string()));

        let event_with_sort = CachedEvent {
            date_sort: Some(chrono::NaiveDate::from_ymd_opt(1842, 3, 15).unwrap()),
            ..event.clone()
        };
        assert_eq!(extract_year(&event_with_sort), Some("1842".to_string()));
    }

    #[test]
    fn test_search_entry_db_roundtrip() {
        let entry = SearchEntry {
            person_id: Uuid::now_v7(),
            sex: Sex::Female,
            surname_normalized: "smith".to_string(),
            given_names_normalized: "jeanne".to_string(),
            maiden_name_normalized: Some("dupont".to_string()),
            display_name: "Jane Smith".to_string(),
            birth_year: Some("1850".to_string()),
            birth_place: Some("Berlin".to_string()),
            death_year: None,
            date_sort: chrono::NaiveDate::from_ymd_opt(1850, 6, 1),
        };
        let tree_id = Uuid::now_v7();

        let db_row = oxidgene_db::repo::PersonSearchEntry {
            person_id: entry.person_id,
            tree_id,
            surname: entry.surname_normalized.clone(),
            given_names: entry.given_names_normalized.clone(),
            maiden_name: entry.maiden_name_normalized.clone(),
            birth_year: entry.birth_year.clone(),
            death_year: entry.death_year.clone(),
            sex: entry.sex.to_string(),
            display_name: entry.display_name.clone(),
            birth_place: entry.birth_place.clone(),
            date_sort: entry.date_sort.map(|d| d.format("%Y-%m-%d").to_string()),
        };

        let back = search_entry_from_db(db_row);
        assert_eq!(back.person_id, entry.person_id);
        assert_eq!(back.sex, Sex::Female);
        assert_eq!(back.surname_normalized, "smith");
        assert_eq!(back.maiden_name_normalized.as_deref(), Some("dupont"));
        assert_eq!(back.date_sort, entry.date_sort);
        assert_eq!(back.display_name, "Jane Smith");
    }
}
