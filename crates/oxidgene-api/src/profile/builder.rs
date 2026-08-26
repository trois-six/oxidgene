//! Projection builder — assembles denormalized read models from raw entities.
//!
//! Takes the normalized rows a caller has already fetched (see
//! `ProfileService::fetch_tree_data` / `fetch_person_data`) and folds them into
//! the shapes stored in `person_denorm` and `person_search_fts`.

use chrono::Utc;
use oxidgene_core::enums::*;
use oxidgene_core::projection::*;

use oxidgene_core::types::{
    Citation, Event, FamilyChild, FamilySpouse, Media, MediaLink, Note, Person, PersonName, Place,
    Vignette,
};
use std::collections::HashMap;
use uuid::Uuid;

/// Holds all raw data for a tree, used to build projections efficiently.
///
/// This is populated once (in parallel) and then used to build every
/// projection without additional database calls.
pub struct TreeData {
    pub persons: Vec<Person>,
    pub names: Vec<PersonName>,
    pub events: Vec<Event>,
    pub places: Vec<Place>,
    pub spouses: Vec<FamilySpouse>,
    pub children: Vec<FamilyChild>,
    pub media: Vec<Media>,
    pub media_links: Vec<MediaLink>,
    /// Only the ones a projection needs: the crops that *are* somebody's
    /// portrait. Every vignette in the tree would be a large slice to carry
    /// for a field that is usually null.
    #[allow(clippy::struct_field_names)]
    pub portrait_vignettes: Vec<Vignette>,
    pub citations: Vec<Citation>,
    pub notes: Vec<Note>,
}

/// Pre-indexed tree data for efficient projection building.
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
    /// The crops that are somebody's portrait, by id.
    portrait_vignette_by_id: HashMap<Uuid, Vignette>,
    /// Media indexed by media_id
    media_by_id: HashMap<Uuid, Media>,
    /// Citation count by person_id
    citation_count_by_person: HashMap<Uuid, u32>,
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
        let portrait_vignette_by_id: HashMap<Uuid, Vignette> = data
            .portrait_vignettes
            .iter()
            .map(|v| (v.id, v.clone()))
            .collect();

        let media_by_id: HashMap<Uuid, Media> =
            data.media.iter().map(|m| (m.id, m.clone())).collect();

        // Count citations directly linked to each person.
        let mut citation_count_by_person: HashMap<Uuid, u32> = HashMap::new();
        for citation in &data.citations {
            if let Some(pid) = citation.person_id {
                *citation_count_by_person.entry(pid).or_default() += 1;
            }
        }

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
            portrait_vignette_by_id,
            media_by_id,
            citation_count_by_person,
            note_count_by_person,
            display_names,
            sex_by_person,
        }
    }
}

/// Build a `ProfileEvent` from a raw `Event` and the place index.
fn build_profile_event(event: &Event, places: &HashMap<Uuid, Place>) -> ProfileEvent {
    let place_name = event
        .place_id
        .and_then(|pid| places.get(&pid))
        .map(|p| p.name.clone());

    ProfileEvent {
        event_id: event.id,
        event_type: event.event_type,
        date_value: event.date_value.clone(),
        date_sort: event.date_sort,
        date_qualifier: event.date_qualifier,
        date_value2: event.date_value2.clone(),
        calendar: event.calendar,
        place_name,
        place_id: event.place_id,
        description: event.description.clone(),
    }
}

/// Extract a year string from a `ProfileEvent` for display.
///
/// Tries `date_sort` first (formatted as "YYYY"), then falls back to
/// extracting a 4-digit year from `date_value`.
pub fn extract_year(event: &ProfileEvent) -> Option<String> {
    oxidgene_core::types::year_from_date(event.date_sort, event.date_value.as_deref())
        .map(|y| format!("{y:04}"))
}

/// The precision to show beside a year pulled out by [`extract_year`].
///
/// A person with no birth event at all is `Exact` rather than anything hedged:
/// the card draws no year for them, so the qualifier is never read, and
/// `Exact` is the value that says "nothing was claimed here".
pub fn extract_qualifier(event: Option<&ProfileEvent>) -> DateQualifier {
    event.map(|e| e.date_qualifier).unwrap_or_default()
}

/// Build all `PersonProfile` entries for an entire tree.
pub fn build_all_persons(tree_id: Uuid, data: &TreeData) -> Vec<PersonProfile> {
    let idx = IndexedData::new(data);
    let now = Utc::now();

    data.persons
        .iter()
        .map(|person| build_one_person(person, tree_id, &idx, now))
        .collect()
}

/// Build a single `PersonProfile` from (possibly targeted) tree data.
///
/// `data` only needs to contain the person, their relatives (spouses, parents,
/// children) and the entities attached to them — see
/// `ProfileService::fetch_person_data`. Returns `None` if the person is not in
/// `data.persons`.
pub fn build_person(tree_id: Uuid, person_id: Uuid, data: &TreeData) -> Option<PersonProfile> {
    let person = data.persons.iter().find(|p| p.id == person_id)?;
    let idx = IndexedData::new(data);
    Some(build_one_person(person, tree_id, &idx, Utc::now()))
}

/// Build a single `PersonProfile` from indexed data.
fn build_one_person(
    person: &Person,
    tree_id: Uuid,
    idx: &IndexedData,
    now: chrono::DateTime<Utc>,
) -> PersonProfile {
    let pid = person.id;

    // ── Names ────────────────────────────────────────────────────────────
    let names = idx.names_by_person.get(&pid).cloned().unwrap_or_default();
    let mut primary_name: Option<ProfileName> = None;
    let mut other_names: Vec<ProfileName> = Vec::new();

    for name in &names {
        let cached = ProfileName {
            name_id: name.id,
            name_type: name.name_type,
            display_name: name.display_name(),
            given_names: name.given_names.clone(),
            // The full surname, particle included: this is a read projection,
            // so it carries display-ready values. It also indexes better —
            // FTS tokenizes "de la Cruz", so both "de la" and "Cruz" match.
            surname: name.full_surname(),
        };
        if name.is_primary {
            primary_name = Some(cached);
        } else {
            other_names.push(cached);
        }
    }

    // ── Events ───────────────────────────────────────────────────────────
    let events = idx.events_by_person.get(&pid).cloned().unwrap_or_default();
    let mut birth: Option<ProfileEvent> = None;
    let mut death: Option<ProfileEvent> = None;
    let mut baptism: Option<ProfileEvent> = None;
    let mut burial: Option<ProfileEvent> = None;
    let mut occupation: Option<String> = None;
    let mut other_events: Vec<ProfileEvent> = Vec::new();

    for event in &events {
        let cached = build_profile_event(event, &idx.places_by_id);
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
    let families_as_spouse: Vec<ProfileFamilyLink> = spouse_entries
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
            let all_family_events: Vec<ProfileEvent> = idx
                .events_by_family
                .get(&family_id)
                .map(|events| {
                    events
                        .iter()
                        .map(|e| build_profile_event(e, &idx.places_by_id))
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

            ProfileFamilyLink {
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

            ProfileChildLink {
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

    // The portrait the person actually chose.
    //
    // This used to take whichever media had the lowest `sort_order`, ignoring
    // the stored choice entirely — so a person could star a photograph and
    // have their pedigree card go on drawing a different one. A portrait that
    // is a crop resolves through the scan it is on, and the vignette id
    // travels with it so a card can ask for the cropped image rather than the
    // whole wedding party.
    // A crop resolves through the scan it sits on, and carries its own id so a
    // card asks for the cropped image rather than the whole wedding party.
    let primary_media = person
        .portrait_vignette_id
        .and_then(|vignette_id| {
            let vignette = idx.portrait_vignette_by_id.get(&vignette_id)?;
            let media = idx.media_by_id.get(&vignette.media_id)?;
            Some(ProfileMediaRef {
                media_id: media.id,
                vignette_id: Some(vignette_id),
                file_path: media.file_path.clone(),
                mime_type: media.mime_type.clone(),
                title: media.title.clone(),
            })
        })
        .or_else(|| {
            let media = idx.media_by_id.get(&person.portrait_media_id?)?;
            Some(ProfileMediaRef {
                media_id: media.id,
                vignette_id: None,
                file_path: media.file_path.clone(),
                mime_type: media.mime_type.clone(),
                title: media.title.clone(),
            })
        })
        // Nothing chosen: their first linked photograph. No import sets a
        // portrait — neither GEDCOM nor a `.gw` says which picture represents
        // somebody — so without this a freshly imported tree draws silhouettes
        // for everyone who has photographs. A document's page is skipped: the
        // register it belongs to is the picture, not page 7 of it.
        .or_else(|| {
            let media = person_media_links
                .iter()
                .filter_map(|link| Some((link, idx.media_by_id.get(&link.media_id)?)))
                .filter(|(_, media)| media.parent_media_id.is_none())
                .min_by_key(|(link, _)| (link.sort_order, link.id))
                .map(|(_, media)| media)?;
            Some(ProfileMediaRef {
                media_id: media.id,
                vignette_id: None,
                file_path: media.file_path.clone(),
                mime_type: media.mime_type.clone(),
                title: media.title.clone(),
            })
        });

    // ── Citation count ───────────────────────────────────────────────────
    let citation_count = idx.citation_count_by_person.get(&pid).copied().unwrap_or(0);

    // ── Note count ───────────────────────────────────────────────────────
    let note_count = idx.note_count_by_person.get(&pid).copied().unwrap_or(0);

    PersonProfile {
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
        built_at: now,
    }
}

/// Build a `SearchEntry` from a `PersonProfile`.
pub fn build_search_entry(person: &PersonProfile) -> SearchEntry {
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
        surname,
        given_names,
        display_name,
        birth_year: person.birth.as_ref().and_then(extract_year),
        birth_place: person.birth.as_ref().and_then(|e| e.place_name.clone()),
        death_year: person.death.as_ref().and_then(extract_year),
        date_sort: person.birth.as_ref().and_then(|e| e.date_sort),
    }
}

/// Build a `person_search_fts` row from a `PersonProfile` (Sprint E.6).
///
/// This is the write model for the DB-native search table which replaced the
/// in-memory search index.
pub fn build_db_search_entry(person: &PersonProfile) -> oxidgene_db::repo::PersonSearchEntry {
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
        surname_display: entry.surname,
        given_names_display: entry.given_names,
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
        surname: row.surname_display,
        given_names: row.given_names_display,
        display_name: row.display_name,
        birth_year: row.birth_year,
        birth_place: row.birth_place,
        death_year: row.death_year,
        date_sort: row
            .date_sort
            .and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
    }
}

/// Build a `PedigreeNode` from a `PersonProfile`.
pub fn build_pedigree_node(
    person: &PersonProfile,
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
        // Whole events, and falling back to baptism / burial the way GeneWeb
        // does — a parish register routinely holds one and not the other, and
        // a card with "1620" beats a card with nothing.
        birth: person.birth_or_baptism().cloned(),
        death: person.death_or_burial().cloned(),
        occupation: person.occupation.clone(),
        primary_media_path: person.primary_media.as_ref().map(|m| m.file_path.clone()),
        generation,
        sosa_number,
    }
}

/// Normalize a string for search: lowercase + accent folding.
///
/// Re-exported from `oxidgene_core::search` so callers of this module keep
/// a single import path.
pub use oxidgene_core::search::normalize_for_search;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year() {
        let event = ProfileEvent {
            event_id: Uuid::now_v7(),
            event_type: EventType::Birth,
            date_value: Some("ABT 1842".to_string()),
            date_sort: None,
            date_qualifier: DateQualifier::About,
            date_value2: None,
            calendar: Calendar::default(),
            place_name: None,
            place_id: None,
            description: None,
        };
        assert_eq!(extract_year(&event), Some("1842".to_string()));

        let event_with_sort = ProfileEvent {
            date_sort: Some(chrono::NaiveDate::from_ymd_opt(1842, 3, 15).unwrap()),
            ..event.clone()
        };
        assert_eq!(extract_year(&event_with_sort), Some("1842".to_string()));

        // The year alone cannot say "about", so the projection has to carry
        // the qualifier beside it — this is what a pedigree card reads to
        // decide between `1842` and `ca 1842`.
        assert_eq!(extract_qualifier(Some(&event)), DateQualifier::About);
        assert_eq!(extract_qualifier(None), DateQualifier::Exact);
    }

    #[test]
    fn test_search_entry_db_roundtrip() {
        let entry = SearchEntry {
            person_id: Uuid::now_v7(),
            sex: Sex::Female,
            surname_normalized: "smith".to_string(),
            given_names_normalized: "jeanne".to_string(),
            maiden_name_normalized: Some("dupont".to_string()),
            surname: "Smith".to_string(),
            given_names: "Jane".to_string(),
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
            surname_display: entry.surname.clone(),
            given_names_display: entry.given_names.clone(),
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
        assert_eq!(back.surname, "Smith");
        assert_eq!(back.given_names, "Jane");
    }
}
