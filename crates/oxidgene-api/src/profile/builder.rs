//! Projection builder — assembles denormalized read models from raw entities.
//!
//! Takes the normalized rows a caller has already fetched (see
//! `ProfileService::fetch_persons_data`) and folds them into the shapes
//! stored in `person_denorm` and `person_search`.

use chrono::Utc;
use oxidgene_core::enums::*;
use oxidgene_core::projection::*;

use oxidgene_core::types::{
    Event, FamilyChild, FamilySpouse, Media, MediaLink, Person, PersonName, Place, TagRef, Vignette,
};
use oxidgene_db::repo::RELATIVE_SEP;
use std::collections::HashMap;
use uuid::Uuid;

/// The rows a set of projections is built from: the persons, their
/// relatives, and what is attached to them — see
/// `ProfileService::fetch_persons_data`.
///
/// Read once, then used to build every projection of the set without
/// another database call.
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
    pub portrait_vignettes: Vec<Vignette>,
    /// How many citations are directly linked to each person; a person
    /// without any is absent. A projection shows the count, never the
    /// citations, so they are counted by the database and not read.
    pub citation_counts: HashMap<Uuid, u32>,
    /// How many notes each person has, as `citation_counts`.
    pub note_counts: HashMap<Uuid, u32>,
    /// The tags of each person of `persons`, sorted by folded label.
    pub person_tags: HashMap<Uuid, Vec<TagRef>>,
    /// The tags of each event of `events`, sorted by folded label.
    pub event_tags: HashMap<Uuid, Vec<TagRef>>,
}

/// The rows of a [`TreeData`], indexed for building projections. It
/// borrows every row: indexing copies nothing.
struct IndexedData<'a> {
    /// PersonName entries grouped by person_id
    names_by_person: HashMap<Uuid, Vec<&'a PersonName>>,
    /// Events grouped by person_id (individual events)
    events_by_person: HashMap<Uuid, Vec<&'a Event>>,
    /// Events grouped by family_id (family events)
    events_by_family: HashMap<Uuid, Vec<&'a Event>>,
    /// Place indexed by place_id
    places_by_id: HashMap<Uuid, &'a Place>,
    /// FamilySpouse entries grouped by family_id
    spouses_by_family: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    /// FamilyChild entries grouped by family_id
    children_by_family: HashMap<Uuid, Vec<&'a FamilyChild>>,
    /// FamilySpouse entries grouped by person_id (families where person is a spouse)
    families_by_spouse: HashMap<Uuid, Vec<&'a FamilySpouse>>,
    /// FamilyChild entries grouped by person_id (family where person is a child)
    family_by_child: HashMap<Uuid, Vec<&'a FamilyChild>>,
    /// MediaLink entries grouped by person_id
    media_links_by_person: HashMap<Uuid, Vec<&'a MediaLink>>,
    /// The crops that are somebody's portrait, by id.
    portrait_vignette_by_id: HashMap<Uuid, &'a Vignette>,
    /// Media indexed by media_id
    media_by_id: HashMap<Uuid, &'a Media>,
    /// The first page of each document, by the document's id.
    first_page_by_document: HashMap<Uuid, Uuid>,
    /// Citation count by person_id
    citation_count_by_person: &'a HashMap<Uuid, u32>,
    /// Note count by person_id
    note_count_by_person: &'a HashMap<Uuid, u32>,
    /// Primary name by person_id, for the names a relative's projection
    /// carries: the display name, and the surname and given names already
    /// split.
    primary_names: HashMap<Uuid, &'a PersonName>,
    /// Person sex by person_id
    sex_by_person: HashMap<Uuid, Sex>,
    /// Tags of each person, by person_id
    tags_by_person: &'a HashMap<Uuid, Vec<TagRef>>,
    /// Tags of each event, by event_id
    tags_by_event: &'a HashMap<Uuid, Vec<TagRef>>,
}

impl<'a> IndexedData<'a> {
    fn new(data: &'a TreeData) -> Self {
        // Index names by person
        let mut names_by_person: HashMap<Uuid, Vec<&PersonName>> = HashMap::new();
        let mut primary_names: HashMap<Uuid, &PersonName> = HashMap::new();
        for name in &data.names {
            names_by_person
                .entry(name.person_id)
                .or_default()
                .push(name);
            if name.is_primary {
                primary_names.insert(name.person_id, name);
            }
        }

        // Index events by person and family
        let mut events_by_person: HashMap<Uuid, Vec<&Event>> = HashMap::new();
        let mut events_by_family: HashMap<Uuid, Vec<&Event>> = HashMap::new();
        for event in &data.events {
            if let Some(pid) = event.person_id {
                events_by_person.entry(pid).or_default().push(event);
            }
            if let Some(fid) = event.family_id {
                events_by_family.entry(fid).or_default().push(event);
            }
        }

        // Index spouses by family and by person
        let mut spouses_by_family: HashMap<Uuid, Vec<&FamilySpouse>> = HashMap::new();
        let mut families_by_spouse: HashMap<Uuid, Vec<&FamilySpouse>> = HashMap::new();
        for spouse in &data.spouses {
            spouses_by_family
                .entry(spouse.family_id)
                .or_default()
                .push(spouse);
            families_by_spouse
                .entry(spouse.person_id)
                .or_default()
                .push(spouse);
        }

        // Index children by family and by person
        let mut children_by_family: HashMap<Uuid, Vec<&FamilyChild>> = HashMap::new();
        let mut family_by_child: HashMap<Uuid, Vec<&FamilyChild>> = HashMap::new();
        for child in &data.children {
            children_by_family
                .entry(child.family_id)
                .or_default()
                .push(child);
            family_by_child
                .entry(child.person_id)
                .or_default()
                .push(child);
        }

        // Index media links by person
        let mut media_links_by_person: HashMap<Uuid, Vec<&MediaLink>> = HashMap::new();
        for link in &data.media_links {
            if let Some(pid) = link.person_id {
                media_links_by_person.entry(pid).or_default().push(link);
            }
        }

        Self {
            names_by_person,
            events_by_person,
            events_by_family,
            places_by_id: data.places.iter().map(|p| (p.id, p)).collect(),
            spouses_by_family,
            children_by_family,
            families_by_spouse,
            family_by_child,
            media_links_by_person,
            portrait_vignette_by_id: data.portrait_vignettes.iter().map(|v| (v.id, v)).collect(),
            media_by_id: data.media.iter().map(|m| (m.id, m)).collect(),
            first_page_by_document: first_pages(&data.media),
            citation_count_by_person: &data.citation_counts,
            note_count_by_person: &data.note_counts,
            primary_names,
            sex_by_person: data.persons.iter().map(|p| (p.id, p.sex)).collect(),
            tags_by_person: &data.person_tags,
            tags_by_event: &data.event_tags,
        }
    }

    /// The primary name of `person_id` as displayed, if one is recorded.
    fn display_name_of(&self, person_id: Uuid) -> Option<String> {
        self.primary_names
            .get(&person_id)
            .map(|name| name.display_name())
    }

    /// The primary surname of `person_id`, if one is recorded.
    fn surname_of(&self, person_id: Uuid) -> Option<String> {
        self.primary_names
            .get(&person_id)
            .and_then(|name| name.surname.clone())
    }

    /// The primary given names of `person_id`, if any are recorded.
    fn given_names_of(&self, person_id: Uuid) -> Option<String> {
        self.primary_names
            .get(&person_id)
            .and_then(|name| name.given_names.clone())
    }
}

/// The first page of each document among `media` that can represent
/// somebody ([`Media::can_portray`]), by the document's id.
fn first_pages(media: &[Media]) -> HashMap<Uuid, Uuid> {
    let mut first: HashMap<Uuid, &Media> = HashMap::new();
    for page in media.iter().filter(|page| page.can_portray()) {
        let Some(document_id) = page.parent_media_id else {
            continue;
        };
        first
            .entry(document_id)
            .and_modify(|held| {
                if (page.page_index, page.id) < (held.page_index, held.id) {
                    *held = page;
                }
            })
            .or_insert(page);
    }
    first
        .into_iter()
        .map(|(document_id, page)| (document_id, page.id))
        .collect()
}

/// Build person `pid`'s `ProfileEvent` from a raw `Event` and the place
/// index.
fn build_profile_event(
    event: &Event,
    places: &HashMap<Uuid, &Place>,
    tags: &HashMap<Uuid, Vec<TagRef>>,
    pid: Uuid,
) -> ProfileEvent {
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
        age: event.age_of(pid).map(str::to_owned),
        tags: tags.get(&event.id).cloned().unwrap_or_default(),
    }
}

/// Extract a year string from a `ProfileEvent` for display.
///
/// Tries `date_sort` first (formatted as "YYYY"), then falls back to
/// extracting a 4-digit year from `date_value`.
pub fn extract_year(event: &ProfileEvent) -> Option<String> {
    event_year(event).map(format_year)
}

/// The year of an event, as a number: what [`extract_year`] shows.
fn event_year(event: &ProfileEvent) -> Option<i32> {
    oxidgene_core::types::year_from_date(event.date_sort, event.date_value.as_deref())
}

/// A year as a result shows it: four digits at least.
fn format_year(year: i32) -> String {
    format!("{year:04}")
}

/// The precision to show beside a year pulled out by [`extract_year`].
///
/// A person with no birth event at all is `Exact` rather than anything hedged:
/// the card draws no year for them, so the qualifier is never read, and
/// `Exact` is the value that says "nothing was claimed here".
pub fn extract_qualifier(event: Option<&ProfileEvent>) -> DateQualifier {
    event.map(|e| e.date_qualifier).unwrap_or_default()
}

/// Build the `PersonProfile` of each of `person_ids` from targeted tree data.
///
/// `data` only needs to contain the persons, their relatives (spouses,
/// parents, children) and the entities attached to them — see
/// `ProfileService::fetch_persons_data`. An id that is not a person of this
/// tree in `data.persons` — deleted since, or another tree's — is skipped.
pub fn build_persons(tree_id: Uuid, person_ids: &[Uuid], data: &TreeData) -> Vec<PersonProfile> {
    // A targeted fetch reaches the persons by ID alone, so the tree is checked
    // here: a person of another tree would otherwise be projected — and stored,
    // and indexed for search — under this one.
    let persons: HashMap<Uuid, &Person> = data
        .persons
        .iter()
        .filter(|p| p.tree_id == tree_id)
        .map(|p| (p.id, p))
        .collect();
    let idx = IndexedData::new(data);
    let now = Utc::now();
    person_ids
        .iter()
        .filter_map(|id| persons.get(id))
        .map(|person| build_one_person(person, tree_id, &idx, now))
        .collect()
}

/// The row that actually holds pixels for a media somebody picked.
///
/// A document has no pixels: what can be drawn is its first page. Reporting the
/// document instead gives a card an id whose bytes do not exist, which is a
/// broken image rather than a missing one. A row that is already a page is used
/// as it stands. The same rule as the portrait resolution in
/// `PersonRepo::list_portraits`, and it must stay the same, or a card and an
/// avatar disagree about the same person.
fn drawable_media<'a>(idx: &IndexedData<'a>, media: &'a Media) -> Option<&'a Media> {
    match media.parent_media_id {
        None => idx
            .first_page_by_document
            .get(&media.id)
            .and_then(|page| idx.media_by_id.get(page).copied()),
        Some(_) => media.can_portray().then_some(media),
    }
}

/// Build a single `PersonProfile` from indexed data.
fn build_one_person(
    person: &Person,
    tree_id: Uuid,
    idx: &IndexedData<'_>,
    now: chrono::DateTime<Utc>,
) -> PersonProfile {
    let pid = person.id;
    let (primary_name, other_names) = profile_names(idx, pid);
    let events = KeyEvents::of(idx, pid);
    let families_as_spouse = idx
        .families_by_spouse
        .get(&pid)
        .map(|entries| entries.iter().map(|fs| spouse_link(idx, pid, fs)).collect())
        .unwrap_or_default();
    let family_as_child = idx
        .family_by_child
        .get(&pid)
        .and_then(|entries| entries.first())
        .map(|fc| child_link(idx, fc));
    let media_links: &[&MediaLink] = idx
        .media_links_by_person
        .get(&pid)
        .map_or(&[], Vec::as_slice);

    PersonProfile {
        placeholder: None,
        person_id: pid,
        tree_id,
        sex: person.sex,
        primary_name,
        other_names,
        birth: events.birth,
        death: events.death,
        baptism: events.baptism,
        burial: events.burial,
        other_events: events.others,
        families_as_spouse,
        family_as_child,
        primary_media: primary_media(idx, person, media_links),
        tags: idx.tags_by_person.get(&pid).cloned().unwrap_or_default(),
        media_count: media_links.len() as u32,
        citation_count: idx.citation_count_by_person.get(&pid).copied().unwrap_or(0),
        note_count: idx.note_count_by_person.get(&pid).copied().unwrap_or(0),
        updated_at: person.updated_at,
        built_at: now,
    }
}

/// The primary name of person `pid`, and their other names.
fn profile_names(idx: &IndexedData<'_>, pid: Uuid) -> (Option<ProfileName>, Vec<ProfileName>) {
    let mut primary_name: Option<ProfileName> = None;
    let mut other_names: Vec<ProfileName> = Vec::new();
    for name in idx.names_by_person.get(&pid).into_iter().flatten() {
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
    (primary_name, other_names)
}

/// A person's own events, the key ones apart.
#[derive(Default)]
struct KeyEvents {
    birth: Option<ProfileEvent>,
    death: Option<ProfileEvent>,
    baptism: Option<ProfileEvent>,
    burial: Option<ProfileEvent>,
    others: Vec<ProfileEvent>,
}

impl KeyEvents {
    fn of(idx: &IndexedData<'_>, pid: Uuid) -> Self {
        let mut events = Self::default();
        for event in idx.events_by_person.get(&pid).into_iter().flatten() {
            let cached = build_profile_event(event, &idx.places_by_id, idx.tags_by_event, pid);
            match event.event_type {
                EventType::Birth => events.birth = Some(cached),
                EventType::Death => events.death = Some(cached),
                EventType::Baptism => events.baptism = Some(cached),
                EventType::Burial => events.burial = Some(cached),
                _ => events.others.push(cached),
            }
        }
        events
    }
}

/// Person `pid`'s link to a family they are a spouse in.
fn spouse_link(idx: &IndexedData<'_>, pid: Uuid, fs: &FamilySpouse) -> ProfileFamilyLink {
    let family_id = fs.family_id;
    let spouse_id = idx
        .spouses_by_family
        .get(&family_id)
        .and_then(|spouses| spouses.iter().find(|s| s.person_id != pid))
        .map(|s| s.person_id);
    // Every family event: marriage, divorce, annulment and the rest.
    let events: Vec<ProfileEvent> = idx
        .events_by_family
        .get(&family_id)
        .into_iter()
        .flatten()
        .map(|e| build_profile_event(e, &idx.places_by_id, idx.tags_by_event, pid))
        .collect();
    let children_ids: Vec<Uuid> = idx
        .children_by_family
        .get(&family_id)
        .into_iter()
        .flatten()
        .map(|c| c.person_id)
        .collect();
    ProfileFamilyLink {
        spouse_placeholder: None,
        family_id,
        role: fs.role,
        spouse_id,
        spouse_display_name: spouse_id.and_then(|sid| idx.display_name_of(sid)),
        spouse_surname: spouse_id.and_then(|sid| idx.surname_of(sid)),
        spouse_given_names: spouse_id.and_then(|sid| idx.given_names_of(sid)),
        spouse_sex: spouse_id.and_then(|sid| idx.sex_by_person.get(&sid).copied()),
        events,
        children_ids,
    }
}

/// A person's link to the family they are a child of.
fn child_link(idx: &IndexedData<'_>, fc: &FamilyChild) -> ProfileChildLink {
    let (father_id, mother_id) = father_and_mother(idx, fc.family_id);
    ProfileChildLink {
        father_placeholder: None,
        mother_placeholder: None,
        family_id: fc.family_id,
        child_type: fc.child_type,
        father_id,
        father_display_name: father_id.and_then(|id| idx.display_name_of(id)),
        father_surname: father_id.and_then(|id| idx.surname_of(id)),
        father_given_names: father_id.and_then(|id| idx.given_names_of(id)),
        mother_id,
        mother_display_name: mother_id.and_then(|id| idx.display_name_of(id)),
        mother_surname: mother_id.and_then(|id| idx.surname_of(id)),
        mother_given_names: mother_id.and_then(|id| idx.given_names_of(id)),
    }
}

/// The father and the mother among a family's spouses, by role, then by sex
/// for a partner; a partner of unknown sex takes the first empty place.
fn father_and_mother(idx: &IndexedData<'_>, family_id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
    let mut father_id: Option<Uuid> = None;
    let mut mother_id: Option<Uuid> = None;
    for parent in idx.spouses_by_family.get(&family_id).into_iter().flatten() {
        let sex = idx.sex_by_person.get(&parent.person_id).copied();
        let slot = match (parent.role, sex) {
            (SpouseRole::Husband, _) | (SpouseRole::Partner, Some(Sex::Male)) => &mut father_id,
            (SpouseRole::Wife, _) | (SpouseRole::Partner, Some(Sex::Female)) => &mut mother_id,
            _ if father_id.is_none() => &mut father_id,
            _ if mother_id.is_none() => &mut mother_id,
            _ => continue,
        };
        *slot = Some(parent.person_id);
    }
    (father_id, mother_id)
}

/// What represents a person: their chosen portrait — a crop keeps its
/// vignette — or, nothing chosen, their first linked photograph.
fn primary_media(
    idx: &IndexedData<'_>,
    person: &Person,
    media_links: &[&MediaLink],
) -> Option<ProfileMediaRef> {
    let crop = || {
        let vignette_id = person.portrait_vignette_id?;
        let vignette = idx.portrait_vignette_by_id.get(&vignette_id)?;
        let media = idx.media_by_id.get(&vignette.media_id)?;
        Some(media_ref(media, Some(vignette_id)))
    };
    // What a reader chooses is a tile, and a tile is a document.
    let chosen = || {
        let media = drawable_media(idx, idx.media_by_id.get(&person.portrait_media_id?)?)?;
        Some(media_ref(media, None))
    };
    // No import sets a portrait — neither GEDCOM nor a `.gw` says which
    // picture represents somebody — so without this a freshly imported tree
    // draws silhouettes for everyone who has photographs.
    let first_linked = || {
        let media = media_links
            .iter()
            .filter_map(|link| Some((link, idx.media_by_id.get(&link.media_id)?)))
            .filter_map(|(link, media)| Some((link, drawable_media(idx, media)?)))
            .min_by_key(|(link, _)| (link.sort_order, link.id))
            .map(|(_, media)| media)?;
        Some(media_ref(media, None))
    };
    crop().or_else(chosen).or_else(first_linked)
}

fn media_ref(media: &Media, vignette_id: Option<Uuid>) -> ProfileMediaRef {
    ProfileMediaRef {
        media_id: media.id,
        vignette_id,
        file_path: media.file_path.clone(),
        mime_type: media.mime_type.clone(),
        title: media.title.clone(),
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

    // A birth recorded without a date is common — the register entry is the
    // baptism — so date the entry the way a pedigree card does, falling back to
    // the baptism and the burial rather than showing a blank year.
    let birth = person.birth_or_baptism();
    let death = person.death_or_burial();

    let spouse_names: Vec<String> = person
        .families_as_spouse
        .iter()
        .filter_map(|f| f.spouse_display_name.clone())
        .collect();
    let children_count = person
        .families_as_spouse
        .iter()
        .map(|f| f.children_ids.len() as u32)
        .sum();

    SearchEntry {
        person_id: person.person_id,
        sex: person.sex,
        surname_normalized: fold_words(&surname),
        given_names_normalized: fold_words(&given_names),
        maiden_name_normalized: maiden_name.as_deref().map(fold_words),
        surname,
        given_names,
        display_name,
        birth_year: birth.and_then(extract_year),
        birth_qualifier: extract_qualifier(birth),
        birth_place: birth.and_then(|e| e.place_name.clone()),
        death_year: death.and_then(extract_year),
        death_qualifier: extract_qualifier(death),
        spouse_names,
        father_name: person
            .family_as_child
            .as_ref()
            .and_then(|c| c.father_display_name.clone()),
        mother_name: person
            .family_as_child
            .as_ref()
            .and_then(|c| c.mother_display_name.clone()),
        children_count,
        date_sort: birth.and_then(|e| e.date_sort),
        portrait: None,
        tags: person.tags.clone(),
        placeholder: person.placeholder,
    }
}

/// Build the person search's row and words of a `PersonProfile`
/// (docs/data-model.md §4.3): the names folded once, here, the years as
/// numbers, and the close relatives' names, which are written as words.
pub fn build_db_search_entry(person: &PersonProfile) -> oxidgene_db::repo::PersonSearchEntry {
    let entry = build_search_entry(person);
    let child_link = person.family_as_child.as_ref();
    let spouses = |field: fn(&ProfileFamilyLink) -> Option<&String>| -> Vec<String> {
        person
            .families_as_spouse
            .iter()
            .filter_map(field)
            .map(|name| fold_words(name))
            .collect()
    };
    let parent = |field: fn(&ProfileChildLink) -> Option<&String>| -> Option<String> {
        child_link.and_then(field).map(|name| fold_words(name))
    };
    let year = |event: Option<&ProfileEvent>| event.and_then(event_year);

    oxidgene_db::repo::PersonSearchEntry {
        row: oxidgene_db::repo::PersonSearchRow {
            person_id: entry.person_id,
            tree_id: person.tree_id,
            surname_key: entry.surname_normalized,
            given_key: entry.given_names_normalized,
            maiden_key: entry.maiden_name_normalized,
            sex: entry.sex.into(),
            birth_year: year(person.birth_or_baptism()),
            birth_qualifier: entry.birth_qualifier.into(),
            death_year: year(person.death_or_burial()),
            death_qualifier: entry.death_qualifier.into(),
            date_sort: entry.date_sort,
            display_name: entry.display_name,
            surname_display: entry.surname,
            given_names_display: entry.given_names,
            birth_place: entry.birth_place,
            spouse_names: entry.spouse_names.join(&RELATIVE_SEP.to_string()),
            father_name: entry.father_name,
            mother_name: entry.mother_name,
            children_count: i32::try_from(entry.children_count).unwrap_or(i32::MAX),
        },
        relatives: oxidgene_db::repo::RelativeNames {
            spouse_surnames: spouses(|f| f.spouse_surname.as_ref()),
            spouse_given_names: spouses(|f| f.spouse_given_names.as_ref()),
            father_surname: parent(|c| c.father_surname.as_ref()),
            father_given_names: parent(|c| c.father_given_names.as_ref()),
            mother_surname: parent(|c| c.mother_surname.as_ref()),
            mother_given_names: parent(|c| c.mother_given_names.as_ref()),
        },
    }
}

/// Convert a `person_search` row back into the `SearchEntry` API shape.
pub fn search_entry_from_db(row: oxidgene_db::repo::PersonSearchRow) -> SearchEntry {
    SearchEntry {
        placeholder: None,
        person_id: row.person_id,
        sex: row.sex.into(),
        surname_normalized: row.surname_key,
        given_names_normalized: row.given_key,
        maiden_name_normalized: row.maiden_key,
        surname: row.surname_display,
        given_names: row.given_names_display,
        display_name: row.display_name,
        birth_year: row.birth_year.map(format_year),
        birth_qualifier: row.birth_qualifier.into(),
        birth_place: row.birth_place,
        death_year: row.death_year.map(format_year),
        death_qualifier: row.death_qualifier.into(),
        spouse_names: split_relatives(&row.spouse_names),
        father_name: row.father_name,
        mother_name: row.mother_name,
        children_count: u32::try_from(row.children_count).unwrap_or(0),
        date_sort: row.date_sort,
        portrait: None,
        // The search index holds no tags; the service fills them in from the
        // tag tables for the rows it returns.
        tags: Vec::new(),
    }
}

/// Split a joined relatives column back into one entry per relative.
///
/// An empty column means "no relatives", not one nameless relative.
fn split_relatives(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        return Vec::new();
    }
    joined.split(RELATIVE_SEP).map(str::to_owned).collect()
}

/// Build a `PedigreeNode` from a `PersonProfile`.
pub fn build_pedigree_node(
    person: &PersonProfile,
    generation: i32,
    sosa_number: Option<u64>,
) -> PedigreeNode {
    PedigreeNode {
        placeholder: None,
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
        occupation: person.occupation().map(str::to_owned),
        primary_media_path: person.primary_media.as_ref().map(|m| m.file_path.clone()),
        primary_media_id: person.primary_media.as_ref().map(|m| m.media_id),
        generation,
        sosa_number,
        // Filled by the assembly, which reads them for the whole window.
        portrait: None,
        sosa_ancestor: false,
        tags: person.tags.clone(),
    }
}

/// Normalize a string for search: lowercase + accent folding.
///
/// Re-exported from `oxidgene_core::search` so callers of this module keep
/// a single import path.
pub use oxidgene_core::search::fold_words;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_year() {
        let event = ProfileEvent {
            tags: Vec::new(),
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
            age: None,
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
            placeholder: None,
            person_id: Uuid::now_v7(),
            sex: Sex::Female,
            surname_normalized: "smith".to_string(),
            given_names_normalized: "jeanne".to_string(),
            maiden_name_normalized: Some("dupont".to_string()),
            surname: "Smith".to_string(),
            given_names: "Jane".to_string(),
            display_name: "Jane Smith".to_string(),
            birth_year: Some("0850".to_string()),
            birth_qualifier: DateQualifier::About,
            birth_place: Some("Berlin".to_string()),
            death_year: None,
            death_qualifier: DateQualifier::Exact,
            spouse_names: vec!["Spouse A".to_string(), "Spouse B".to_string()],
            father_name: Some("Father One".to_string()),
            mother_name: Some("Mother One".to_string()),
            children_count: 3,
            date_sort: chrono::NaiveDate::from_ymd_opt(850, 6, 1),
            portrait: None,
            tags: Vec::new(),
        };

        let db_row = oxidgene_db::repo::PersonSearchRow {
            person_id: entry.person_id,
            tree_id: Uuid::now_v7(),
            surname_key: entry.surname_normalized.clone(),
            given_key: entry.given_names_normalized.clone(),
            maiden_key: entry.maiden_name_normalized.clone(),
            sex: entry.sex.into(),
            birth_year: Some(850),
            birth_qualifier: entry.birth_qualifier.into(),
            death_year: None,
            death_qualifier: entry.death_qualifier.into(),
            date_sort: entry.date_sort,
            display_name: entry.display_name.clone(),
            surname_display: entry.surname.clone(),
            given_names_display: entry.given_names.clone(),
            birth_place: entry.birth_place.clone(),
            spouse_names: entry.spouse_names.join(&RELATIVE_SEP.to_string()),
            father_name: entry.father_name.clone(),
            mother_name: entry.mother_name.clone(),
            children_count: 3,
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
        // The qualifier has to survive the round trip, or every search result
        // silently claims a precision the source never had.
        assert_eq!(back.birth_qualifier, DateQualifier::About);
        assert_eq!(back.death_qualifier, DateQualifier::Exact);
        assert_eq!(back.spouse_names, entry.spouse_names);
        assert_eq!(back.father_name.as_deref(), Some("Father One"));
        assert_eq!(back.mother_name.as_deref(), Some("Mother One"));
        assert_eq!(back.children_count, 3);
    }

    #[test]
    fn a_stored_year_reads_back_with_four_digits_at_least() {
        assert_eq!(format_year(850), "0850");
        assert_eq!(format_year(1849), "1849");
    }

    #[test]
    fn empty_relatives_column_reads_as_no_relatives() {
        // The joined column is a plain string, so "" has to mean an empty list
        // rather than one nameless spouse — otherwise every single person in
        // the tree reads as married.
        assert!(split_relatives("").is_empty());
        assert_eq!(split_relatives("Only One"), vec!["Only One".to_string()]);
        assert_eq!(
            split_relatives("First\u{1f}Second"),
            vec!["First".to_string(), "Second".to_string()]
        );
    }

    #[test]
    fn search_entry_carries_the_person_tags() {
        let tag = TagRef {
            id: Uuid::now_v7(),
            label: "To verify".to_string(),
            color: 2,
        };
        let person = PersonProfile {
            placeholder: None,
            tags: vec![tag.clone()],
            person_id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            sex: Sex::Unknown,
            primary_name: None,
            other_names: Vec::new(),
            birth: None,
            death: None,
            baptism: None,
            burial: None,
            other_events: Vec::new(),
            families_as_spouse: Vec::new(),
            family_as_child: None,
            primary_media: None,
            media_count: 0,
            citation_count: 0,
            note_count: 0,
            updated_at: chrono::Utc::now(),
            built_at: chrono::Utc::now(),
        };

        // The projection's tags reach the search row as they are, so a result
        // shows the same tags as the person's profile.
        assert_eq!(build_search_entry(&person).tags, vec![tag]);
        // The search index holds none; a row read back from it has none until
        // the service attaches them.
        let row = build_db_search_entry(&person);
        assert!(search_entry_from_db(row.row).tags.is_empty());
    }
}
