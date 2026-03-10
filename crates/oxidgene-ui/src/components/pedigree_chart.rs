//! Vertical bidirectional pedigree chart with pan/zoom, icon sidebar, and event panel.
//!
//! Layout: `.pedigree-outer` (flex row)
//!   -> `.isb` (icon sidebar: depth/zoom controls)
//!   -> `.pedigree-viewport` (pannable/zoomable canvas)
//!   -> `.ev-panel` (selected-person event list)
//!
//! Cards are placed on a fixed-step grid using absolute positioning.
//! Connectors are drawn via SVG overlay with 90-degree L-shaped bends.

use std::collections::HashMap;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::tree_cache::{PedigreeViewState, use_view_state_cache};

use oxidgene_cache::types::CachedPedigree;
use oxidgene_core::types::{
    Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName, Place,
};
use oxidgene_core::{ChildType, EventType, Sex, SpouseRole};

use crate::i18n::use_i18n;

// ── Layout constants ─────────────────────────────────────────────────────

const CARD_W: f64 = 180.0;
const CARD_H: f64 = 80.0;
const H_GAP: f64 = 16.0;
const V_GAP: f64 = 40.0;
const STEP: f64 = CARD_W + H_GAP; // 196
const _CONNECTOR_MID: f64 = V_GAP / 2.0; // vertical midpoint between rows

// ── Helper functions ─────────────────────────────────────────────────────

/// Extract a 4-digit year from a GEDCOM date string (e.g. "ABT 1842", "1 JAN 1900").
fn fmt_year(date: &str) -> String {
    for word in date.split_whitespace() {
        if word.len() == 4
            && word
                .parse::<u32>()
                .is_ok_and(|y| (1000..=2099).contains(&y))
        {
            return word.to_string();
        }
    }
    if date.len() > 12 {
        format!("{}...", &date[..10])
    } else {
        date.to_string()
    }
}

/// Build two-letter initials from given name + surname.
fn make_initials(given: &str, surname: &str) -> String {
    let first = given.chars().next().map(|c| c.to_ascii_uppercase());
    let last = surname.chars().next().map(|c| c.to_ascii_uppercase());
    match (first, last) {
        (Some(f), Some(l)) => format!("{f}{l}"),
        (Some(f), None) => f.to_string(),
        (None, Some(l)) => l.to_string(),
        _ => "?".to_string(),
    }
}

/// Returns `(icon, css_class, i18n_key)` for an event type.
///
/// The third element is an i18n key that must be resolved via `i18n.t()`.
fn event_ui(et: EventType) -> (&'static str, &'static str, &'static str) {
    match et {
        EventType::Birth => ("\u{2726}", "ev-ic ev-ic-birth", "event.type.birth"),
        EventType::Baptism => ("\u{271F}", "ev-ic ev-ic-birth", "event.type.baptism"),
        EventType::Death => ("\u{271D}", "ev-ic ev-ic-death", "event.type.death"),
        EventType::Burial => ("\u{26B0}", "ev-ic ev-ic-death", "event.type.burial"),
        EventType::Cremation => ("\u{271D}", "ev-ic ev-ic-death", "event.type.cremation"),
        EventType::Marriage => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.marriage"),
        EventType::Engagement => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.engagement"),
        EventType::MarriageBann => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.banns"),
        EventType::MarriageContract => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.contract"),
        EventType::MarriageLicense => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.license"),
        EventType::MarriageSettlement => {
            ("\u{1F48D}", "ev-ic ev-ic-marry", "event.short.settlement")
        }
        EventType::Divorce => ("\u{2696}", "ev-ic ev-ic-other", "event.type.divorce"),
        EventType::Annulment => ("\u{2696}", "ev-ic ev-ic-other", "event.type.annulment"),
        EventType::Graduation => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.graduation"),
        EventType::Immigration => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.immigration"),
        EventType::Emigration => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.emigration"),
        EventType::Naturalization => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.naturalization"),
        EventType::Census => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.census"),
        EventType::Occupation => ("\u{2692}", "ev-ic ev-ic-other", "event.type.occupation"),
        EventType::Residence => ("\u{1F3E1}", "ev-ic ev-ic-other", "event.type.residence"),
        EventType::Retirement => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.retirement"),
        EventType::Will => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.will"),
        EventType::Probate => ("\u{1F4DC}", "ev-ic ev-ic-other", "event.type.probate"),
        EventType::Other => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.other"),
    }
}

// ── Data model ───────────────────────────────────────────────────────────

/// Data needed to render the pedigree chart, pre-computed from API data.
#[derive(Clone, Debug)]
pub struct PedigreeData {
    pub persons: HashMap<Uuid, Person>,
    pub names: HashMap<Uuid, Vec<PersonName>>,
    pub spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
    pub children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
    pub families_as_child: HashMap<Uuid, Vec<Uuid>>,
    pub families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
    pub events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
    pub events_by_family: HashMap<Uuid, Vec<DomainEvent>>,
    pub places: HashMap<Uuid, Place>,
}

impl PartialEq for PedigreeData {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl PedigreeData {
    pub fn build(
        persons: &[Person],
        names: HashMap<Uuid, Vec<PersonName>>,
        all_spouses: &[FamilySpouse],
        all_children: &[FamilyChild],
        events: Vec<DomainEvent>,
        places: HashMap<Uuid, Place>,
    ) -> Self {
        let persons_map: HashMap<Uuid, Person> =
            persons.iter().map(|p| (p.id, p.clone())).collect();

        let mut spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>> = HashMap::new();
        for s in all_spouses {
            spouses_by_family
                .entry(s.family_id)
                .or_default()
                .push(s.clone());
        }

        let mut children_by_family: HashMap<Uuid, Vec<FamilyChild>> = HashMap::new();
        for c in all_children {
            children_by_family
                .entry(c.family_id)
                .or_default()
                .push(c.clone());
        }

        let mut families_as_child: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for c in all_children {
            families_as_child
                .entry(c.person_id)
                .or_default()
                .push(c.family_id);
        }

        let mut families_as_spouse: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for s in all_spouses {
            families_as_spouse
                .entry(s.person_id)
                .or_default()
                .push(s.family_id);
        }

        let mut events_by_person: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        let mut events_by_family: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for e in events {
            if let Some(pid) = e.person_id {
                events_by_person.entry(pid).or_default().push(e.clone());
            }
            if let Some(fid) = e.family_id {
                events_by_family.entry(fid).or_default().push(e);
            }
        }

        Self {
            persons: persons_map,
            names,
            spouses_by_family,
            children_by_family,
            families_as_child,
            families_as_spouse,
            events_by_person,
            events_by_family,
            places,
        }
    }

    /// Build `PedigreeData` from a [`CachedPedigree`] returned by the cache API.
    ///
    /// Creates synthetic domain objects (Person, PersonName, Event) from the
    /// denormalized pedigree nodes so the existing layout + rendering code works
    /// unchanged.
    pub fn from_cached_pedigree(pedigree: &CachedPedigree) -> Self {
        use chrono::{NaiveDate, Utc};

        let now = Utc::now();
        let tree_id = pedigree.tree_id;

        // ── Persons & Names ──
        let mut persons: HashMap<Uuid, Person> = HashMap::new();
        let mut names: HashMap<Uuid, Vec<PersonName>> = HashMap::new();

        for node in pedigree.persons.values() {
            let person = Person {
                id: node.person_id,
                tree_id,
                sex: node.sex,
                created_at: now,
                updated_at: now,
                deleted_at: None,
            };
            persons.insert(node.person_id, person);

            // Use structured name fields if available, fall back to splitting display_name.
            let (given, surname) = if node.given_names.is_some() || node.surname.is_some() {
                (node.given_names.clone(), node.surname.clone())
            } else {
                match node.display_name.rsplit_once(' ') {
                    Some((g, s)) => (Some(g.to_string()), Some(s.to_string())),
                    None => (Some(node.display_name.clone()), None),
                }
            };
            let name = PersonName {
                id: Uuid::nil(),
                person_id: node.person_id,
                name_type: oxidgene_core::NameType::Birth,
                given_names: given,
                surname,
                prefix: None,
                suffix: None,
                nickname: None,
                is_primary: true,
                created_at: now,
                updated_at: now,
            };
            names.insert(node.person_id, vec![name]);
        }

        // ── Synthetic events from PedigreeNode birth/death years ──
        let mut events_by_person: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for node in pedigree.persons.values() {
            let mut person_events = Vec::new();
            if let Some(ref year_str) = node.birth_year {
                let date_sort = year_str
                    .parse::<i32>()
                    .ok()
                    .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1));
                let mut evt = DomainEvent {
                    id: Uuid::now_v7(),
                    tree_id,
                    event_type: EventType::Birth,
                    date_value: Some(year_str.clone()),
                    date_sort,
                    place_id: None,
                    person_id: Some(node.person_id),
                    family_id: None,
                    description: None,
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                };
                // If we have a birth place string, we can't create a real Place
                // (no UUID) — leave place_id as None; the chart reads place_name()
                // but falls back gracefully.
                if node.birth_place.is_some() {
                    evt.description = node.birth_place.clone();
                }
                person_events.push(evt);
            }
            if let Some(ref year_str) = node.death_year {
                let date_sort = year_str
                    .parse::<i32>()
                    .ok()
                    .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1));
                let mut evt = DomainEvent {
                    id: Uuid::now_v7(),
                    tree_id,
                    event_type: EventType::Death,
                    date_value: Some(year_str.clone()),
                    date_sort,
                    place_id: None,
                    person_id: Some(node.person_id),
                    family_id: None,
                    description: None,
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                };
                if node.death_place.is_some() {
                    evt.description = node.death_place.clone();
                }
                person_events.push(evt);
            }
            if !person_events.is_empty() {
                events_by_person.insert(node.person_id, person_events);
            }
        }

        // ── Family relationships from CachedFamily + PedigreeEdge ──
        //
        // CachedFamily carries full family membership (spouses + children),
        // covering childless couples that produce no PedigreeEdge.
        // We supplement with edge data for child_type info.

        // Build child_type lookup from edges.
        let mut child_type_map: HashMap<(Uuid, Uuid), ChildType> = HashMap::new();
        for edge in &pedigree.edges {
            child_type_map.insert((edge.family_id, edge.child_id), edge.edge_type);
        }

        let mut spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>> = HashMap::new();
        let mut children_by_family: HashMap<Uuid, Vec<FamilyChild>> = HashMap::new();
        let mut families_as_child: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        let mut families_as_spouse: HashMap<Uuid, Vec<Uuid>> = HashMap::new();

        for (family_id, cached_family) in &pedigree.families {
            // Build FamilySpouse entries — assign role by sex.
            for (i, &spouse_id) in cached_family.spouse_ids.iter().enumerate() {
                let role = match persons.get(&spouse_id).map(|p| &p.sex) {
                    Some(Sex::Male) => SpouseRole::Husband,
                    Some(Sex::Female) => SpouseRole::Wife,
                    _ => {
                        if i == 0 {
                            SpouseRole::Husband
                        } else {
                            SpouseRole::Wife
                        }
                    }
                };
                let fs = FamilySpouse {
                    id: Uuid::nil(),
                    family_id: *family_id,
                    person_id: spouse_id,
                    role,
                    sort_order: i as i32,
                };
                spouses_by_family.entry(*family_id).or_default().push(fs);
                families_as_spouse
                    .entry(spouse_id)
                    .or_default()
                    .push(*family_id);
            }

            // Build FamilyChild entries.
            for (i, &child_id) in cached_family.children_ids.iter().enumerate() {
                let child_type = child_type_map
                    .get(&(*family_id, child_id))
                    .copied()
                    .unwrap_or(ChildType::Biological);
                let fc = FamilyChild {
                    id: Uuid::nil(),
                    family_id: *family_id,
                    person_id: child_id,
                    child_type,
                    sort_order: i as i32,
                };
                children_by_family.entry(*family_id).or_default().push(fc);
                families_as_child
                    .entry(child_id)
                    .or_default()
                    .push(*family_id);
            }
        }

        // Deduplicate families_as_spouse entries.
        for fids in families_as_spouse.values_mut() {
            fids.sort();
            fids.dedup();
        }

        // Deduplicate families_as_child entries.
        for fids in families_as_child.values_mut() {
            fids.sort();
            fids.dedup();
        }

        // ── Reconstruct family events from cached pedigree ──
        let mut events_by_family: HashMap<Uuid, Vec<DomainEvent>> = HashMap::new();
        for (family_id, cached_events) in &pedigree.family_events {
            let domain_events: Vec<DomainEvent> = cached_events
                .iter()
                .map(|ce| DomainEvent {
                    id: ce.event_id,
                    tree_id,
                    event_type: ce.event_type,
                    date_value: ce.date_value.clone(),
                    date_sort: ce.date_sort,
                    place_id: ce.place_id,
                    person_id: None,
                    family_id: Some(*family_id),
                    description: ce.description.clone(),
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                })
                .collect();
            events_by_family.insert(*family_id, domain_events);
        }

        // ── Synthetic events + names for family members outside the pedigree window ──
        for cached_family in pedigree.families.values() {
            for member in &cached_family.members {
                // Skip members already in the pedigree persons map.
                if persons.contains_key(&member.person_id) {
                    continue;
                }
                // Build synthetic person + name (for display in event panel).
                let (given, surname) = match member.display_name.rsplit_once(' ') {
                    Some((g, s)) => (Some(g.to_string()), Some(s.to_string())),
                    None => (Some(member.display_name.clone()), None),
                };
                let person = Person {
                    id: member.person_id,
                    tree_id,
                    sex: member.sex,
                    created_at: now,
                    updated_at: now,
                    deleted_at: None,
                };
                persons.insert(member.person_id, person);
                let name = PersonName {
                    id: Uuid::nil(),
                    person_id: member.person_id,
                    name_type: oxidgene_core::NameType::Birth,
                    given_names: given,
                    surname,
                    prefix: None,
                    suffix: None,
                    nickname: None,
                    is_primary: true,
                    created_at: now,
                    updated_at: now,
                };
                names.insert(member.person_id, vec![name]);

                // Build synthetic birth/death events.
                let mut member_events = Vec::new();
                if let Some(ref year_str) = member.birth_year {
                    let date_sort = year_str
                        .parse::<i32>()
                        .ok()
                        .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1));
                    member_events.push(DomainEvent {
                        id: Uuid::now_v7(),
                        tree_id,
                        event_type: EventType::Birth,
                        date_value: Some(year_str.clone()),
                        date_sort,
                        place_id: None,
                        person_id: Some(member.person_id),
                        family_id: None,
                        description: None,
                        created_at: now,
                        updated_at: now,
                        deleted_at: None,
                    });
                }
                if let Some(ref year_str) = member.death_year {
                    let date_sort = year_str
                        .parse::<i32>()
                        .ok()
                        .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1));
                    member_events.push(DomainEvent {
                        id: Uuid::now_v7(),
                        tree_id,
                        event_type: EventType::Death,
                        date_value: Some(year_str.clone()),
                        date_sort,
                        place_id: None,
                        person_id: Some(member.person_id),
                        family_id: None,
                        description: None,
                        created_at: now,
                        updated_at: now,
                        deleted_at: None,
                    });
                }
                if !member_events.is_empty() {
                    events_by_person.insert(member.person_id, member_events);
                }
            }
        }

        Self {
            persons,
            names,
            spouses_by_family,
            children_by_family,
            families_as_child,
            families_as_spouse,
            events_by_person,
            events_by_family,
            places: HashMap::new(),
        }
    }

    /// Compute the set of all ancestors of a given person (excluding the person).
    pub fn ancestor_set(&self, person_id: Uuid) -> std::collections::HashSet<Uuid> {
        let mut result = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(person_id);
        while let Some(pid) = queue.pop_front() {
            let (father, mother) = self.parents_of(pid);
            if let Some(f) = father
                && result.insert(f)
            {
                queue.push_back(f);
            }
            if let Some(m) = mother
                && result.insert(m)
            {
                queue.push_back(m);
            }
        }
        result
    }

    fn parents_of(&self, person_id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
        let Some(family_ids) = self.families_as_child.get(&person_id) else {
            return (None, None);
        };
        let Some(fid) = family_ids.first() else {
            return (None, None);
        };
        let Some(spouses) = self.spouses_by_family.get(fid) else {
            return (None, None);
        };

        let mut father = None;
        let mut mother = None;

        for sp in spouses {
            if let Some(person) = self.persons.get(&sp.person_id) {
                match person.sex {
                    Sex::Male => {
                        if father.is_none() {
                            father = Some(sp.person_id);
                        }
                    }
                    Sex::Female => {
                        if mother.is_none() {
                            mother = Some(sp.person_id);
                        }
                    }
                    Sex::Unknown => {
                        if father.is_none() {
                            father = Some(sp.person_id);
                        } else if mother.is_none() {
                            mother = Some(sp.person_id);
                        }
                    }
                }
            }
        }

        (father, mother)
    }

    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.persons
            .get(&person_id)
            .map(|p| p.sex)
            .unwrap_or(Sex::Unknown)
    }

    pub fn name_parts(&self, person_id: Uuid) -> (Option<String>, Option<String>, Option<String>) {
        let Some(names) = self.names.get(&person_id) else {
            return (None, None, None);
        };
        let name = names
            .iter()
            .find(|n| n.is_primary)
            .or_else(|| names.first());
        match name {
            Some(n) => (n.given_names.clone(), n.surname.clone(), n.nickname.clone()),
            None => (None, None, None),
        }
    }

    /// Resolve a full display name for a person.
    pub fn display_name(&self, person_id: Uuid) -> String {
        crate::utils::resolve_name(person_id, &self.names)
    }

    fn birth_date(&self, person_id: Uuid) -> Option<String> {
        let events = self.events_by_person.get(&person_id)?;
        if let Some(e) = events.iter().find(|e| e.event_type == EventType::Birth) {
            return e.date_value.as_deref().map(fmt_year);
        }
        if let Some(e) = events.iter().find(|e| e.event_type == EventType::Baptism) {
            return e.date_value.as_deref().map(fmt_year);
        }
        None
    }

    /// Returns the correct symbol for the birth/start date.
    fn birth_symbol(&self, person_id: Uuid) -> &'static str {
        let Some(events) = self.events_by_person.get(&person_id) else {
            return "\u{2726}";
        };
        if events
            .iter()
            .any(|e| e.event_type == EventType::Birth && e.date_value.is_some())
        {
            return "\u{2726}"; // ✦
        }
        if events
            .iter()
            .any(|e| e.event_type == EventType::Baptism && e.date_value.is_some())
        {
            return "\u{271F}"; // ✟
        }
        "\u{2726}"
    }

    /// Returns the correct symbol for the death/end date.
    fn death_symbol(&self, person_id: Uuid) -> &'static str {
        let Some(events) = self.events_by_person.get(&person_id) else {
            return "\u{271D}";
        };
        if events
            .iter()
            .any(|e| e.event_type == EventType::Death && e.date_value.is_some())
        {
            return "\u{271D}"; // ✝
        }
        if events
            .iter()
            .any(|e| e.event_type == EventType::Burial && e.date_value.is_some())
        {
            return "\u{26B0}"; // ⚰
        }
        "\u{271D}"
    }

    fn death_date(&self, person_id: Uuid) -> Option<String> {
        let events = self.events_by_person.get(&person_id)?;
        if let Some(e) = events.iter().find(|e| e.event_type == EventType::Death) {
            return e.date_value.as_deref().map(fmt_year);
        }
        if let Some(e) = events.iter().find(|e| e.event_type == EventType::Burial) {
            return e.date_value.as_deref().map(fmt_year);
        }
        None
    }

    fn marriage_date_for_family(&self, family_id: Uuid) -> Option<String> {
        let events = self.events_by_family.get(&family_id)?;
        events
            .iter()
            .find(|e| e.event_type == EventType::Marriage)
            .and_then(|e| e.date_value.as_deref().map(fmt_year))
    }

    #[allow(dead_code)]
    fn parent_family_of(&self, person_id: Uuid) -> Option<Uuid> {
        self.families_as_child.get(&person_id)?.first().copied()
    }

    /// Resolve a place_id to its name.
    fn place_name(&self, place_id: Uuid) -> Option<&str> {
        self.places.get(&place_id).map(|p| p.name.as_str())
    }

    /// Get unions for a person: Vec<(family_id, partner_name, marriage_year)>.
    pub fn unions_for_person(&self, person_id: Uuid) -> Vec<(Uuid, String, String)> {
        let Some(family_ids) = self.families_as_spouse.get(&person_id) else {
            return vec![];
        };
        family_ids
            .iter()
            .map(|&fid| {
                let partner_name = self
                    .spouses_by_family
                    .get(&fid)
                    .and_then(|sps| {
                        sps.iter()
                            .find(|s| s.person_id != person_id)
                            .map(|s| s.person_id)
                    })
                    .map(|pid| {
                        let (g, s, _) = self.name_parts(pid);
                        let gs = g.unwrap_or_default();
                        let ss = s.unwrap_or_default();
                        if gs.is_empty() && ss.is_empty() {
                            "Unknown".to_string()
                        } else {
                            format!("{} {}", gs, ss).trim().to_string()
                        }
                    })
                    .unwrap_or_else(|| "Unknown".to_string());
                let marriage_year = self.marriage_date_for_family(fid).unwrap_or_default();
                (fid, partner_name, marriage_year)
            })
            .collect()
    }
}

// ── Ancestor slot tree ───────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum PedigreeSlot {
    Person(Uuid),
    Empty,
}

fn build_ancestor_slots(
    root_id: Uuid,
    data: &PedigreeData,
    levels: usize,
) -> Vec<Vec<PedigreeSlot>> {
    let mut generations = vec![vec![PedigreeSlot::Person(root_id)]];
    for _ in 0..levels {
        let prev = generations.last().unwrap();
        let mut next = Vec::new();
        for slot in prev {
            match slot {
                PedigreeSlot::Person(pid) => {
                    let (father, mother) = data.parents_of(*pid);
                    next.push(match father {
                        Some(id) => PedigreeSlot::Person(id),
                        None => PedigreeSlot::Empty,
                    });
                    next.push(match mother {
                        Some(id) => PedigreeSlot::Person(id),
                        None => PedigreeSlot::Empty,
                    });
                }
                PedigreeSlot::Empty => {
                    next.push(PedigreeSlot::Empty);
                    next.push(PedigreeSlot::Empty);
                }
            }
        }
        generations.push(next);
    }
    generations
}

// ── Descendant structures ────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct DescendantFamily {
    #[allow(dead_code)]
    parent_id: Uuid,
    #[allow(dead_code)]
    spouse_id: Option<Uuid>,
    family_id: Uuid,
    children: Vec<Uuid>,
}

#[derive(Clone, Debug)]
struct DescendantGeneration {
    families: Vec<DescendantFamily>,
}

fn build_descendant_generations(
    root_id: Uuid,
    data: &PedigreeData,
    max_levels: usize,
) -> Vec<DescendantGeneration> {
    if max_levels == 0 {
        return vec![];
    }
    let mut result = Vec::new();
    let mut current_parents = vec![root_id];

    for _ in 0..max_levels {
        let mut generation = DescendantGeneration {
            families: Vec::new(),
        };
        let mut next_parents = Vec::new();
        let mut seen_families = std::collections::HashSet::new();

        for &parent_id in &current_parents {
            let family_ids = data
                .families_as_spouse
                .get(&parent_id)
                .cloned()
                .unwrap_or_default();

            for fid in family_ids {
                if !seen_families.insert(fid) {
                    continue;
                }

                let children: Vec<Uuid> = data
                    .children_by_family
                    .get(&fid)
                    .map(|cs| cs.iter().map(|c| c.person_id).collect())
                    .unwrap_or_default();

                if children.is_empty() {
                    continue;
                }

                let spouse_id = data
                    .spouses_by_family
                    .get(&fid)
                    .and_then(|sps| sps.iter().find(|s| s.person_id != parent_id))
                    .map(|s| s.person_id);

                for &child_id in &children {
                    if !next_parents.contains(&child_id) {
                        next_parents.push(child_id);
                    }
                }

                generation.families.push(DescendantFamily {
                    parent_id,
                    spouse_id,
                    family_id: fid,
                    children,
                });
            }
        }

        if generation.families.is_empty() {
            break;
        }
        result.push(generation);
        current_parents = next_parents;
    }

    result
}

// ── Grid layout computation ──────────────────────────────────────────────

/// A positioned node on the canvas.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct LayoutNode {
    id: Option<Uuid>,
    x: f64,
    y: f64,
    generation: i32,         // negative = ancestor, 0 = root, positive = descendant
    slot_idx: usize,         // index within generation for ancestors, child index for descendants
    child_of: Option<Uuid>,  // child person id (for empty ancestor slots)
    is_father: bool,         // for empty ancestor slots
    family_id: Option<Uuid>, // for descendant nodes, which family they belong to
}

/// SVG line segment.
#[derive(Clone, Debug)]
struct Segment {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

/// Compute full grid layout.
fn compute_layout(
    root_id: Uuid,
    data: &PedigreeData,
    ancestor_levels: usize,
    descendant_levels: usize,
) -> (Vec<LayoutNode>, Vec<Segment>, f64, f64) {
    let anc_gens = build_ancestor_slots(root_id, data, ancestor_levels);

    // Find deepest ancestor gen with at least one real person.
    let max_anc_gen_idx = anc_gens
        .iter()
        .rposition(|g| g.iter().any(|s| matches!(s, PedigreeSlot::Person(_))))
        .unwrap_or(0);
    let display_anc = (max_anc_gen_idx + 1)
        .min(anc_gens.len())
        .max(if ancestor_levels > 0 { 2 } else { 1 });
    let deepest_anc = display_anc.saturating_sub(1);

    // Descendant data.
    let desc_gens = build_descendant_generations(root_id, data, descendant_levels);

    // Width determined by the widest ancestor generation.
    let max_anc_slots = if deepest_anc > 0 {
        1usize << deepest_anc
    } else {
        1
    };
    let anc_width = max_anc_slots as f64 * STEP - H_GAP;

    // Descendant width: sum of all families' children widths + gaps.
    let mut max_desc_width: f64 = 0.0;
    for dg in &desc_gens {
        let mut gen_width: f64 = 0.0;
        for (fi, fam) in dg.families.iter().enumerate() {
            let fam_w = fam.children.len().max(1) as f64 * STEP - H_GAP;
            gen_width += fam_w;
            if fi < dg.families.len() - 1 {
                gen_width += STEP; // gap between families
            }
        }
        if gen_width > max_desc_width {
            max_desc_width = gen_width;
        }
    }

    // Root spouses: find all distinct spouses across root's families.
    let mut root_spouses_with_families: Vec<(Uuid, Uuid)> = Vec::new(); // (spouse_id, family_id)
    {
        let mut seen = std::collections::HashSet::new();
        if let Some(fids) = data.families_as_spouse.get(&root_id) {
            for &fid in fids {
                if let Some(sp) = data
                    .spouses_by_family
                    .get(&fid)
                    .and_then(|sps| sps.iter().find(|s| s.person_id != root_id))
                    && seen.insert(sp.person_id)
                {
                    root_spouses_with_families.push((sp.person_id, fid));
                }
            }
        }
    }
    let n_root_spouses = root_spouses_with_families.len();
    // Minimum width so root stays centered with spouses to the right.
    let root_group_min_w = if n_root_spouses > 0 {
        CARD_W + 2.0 * n_root_spouses as f64 * STEP
    } else {
        CARD_W
    };

    let total_width = anc_width
        .max(max_desc_width)
        .max(root_group_min_w)
        .max(CARD_W);
    let total_gen_count = display_anc + 1 + desc_gens.len(); // anc gens + root + desc gens
    let total_height = total_gen_count as f64 * (CARD_H + V_GAP) - V_GAP;

    let mut nodes = Vec::new();
    let mut segments = Vec::new();

    // ── Ancestor nodes ──
    for gen_idx in 0..display_anc {
        let num_slots = 1usize << gen_idx;
        let gen_width = num_slots as f64 * STEP - H_GAP;
        let x_offset = (total_width - gen_width) / 2.0;
        // Row Y: deepest ancestor is at top (row 0), root is at row deepest_anc.
        let row = deepest_anc - gen_idx;
        let y = row as f64 * (CARD_H + V_GAP);

        for (si, slot) in anc_gens[gen_idx].iter().enumerate() {
            if si >= num_slots {
                break;
            }
            let slot_width = gen_width / num_slots as f64;
            let x = x_offset + si as f64 * slot_width + (slot_width - CARD_W) / 2.0;

            let child_of = if gen_idx > 0 {
                let child_idx = si / 2;
                match anc_gens[gen_idx - 1].get(child_idx) {
                    Some(PedigreeSlot::Person(pid)) => Some(*pid),
                    _ => None,
                }
            } else {
                None
            };

            nodes.push(LayoutNode {
                id: match slot {
                    PedigreeSlot::Person(pid) => Some(*pid),
                    PedigreeSlot::Empty => None,
                },
                x,
                y,
                generation: -(gen_idx as i32),
                slot_idx: si,
                child_of,
                is_father: si % 2 == 0,
                family_id: None,
            });
        }
    }

    // ── Ancestor connectors ──
    // Between each ancestor gen and its child gen.
    for gen_idx in 1..display_anc {
        let parent_gen = gen_idx;
        let child_gen = gen_idx - 1;
        let parent_slots = 1usize << parent_gen;
        let child_slots = 1usize << child_gen;
        let parent_gen_w = parent_slots as f64 * STEP - H_GAP;
        let child_gen_w = child_slots as f64 * STEP - H_GAP;
        let parent_x_off = (total_width - parent_gen_w) / 2.0;
        let child_x_off = (total_width - child_gen_w) / 2.0;
        let parent_row = deepest_anc - parent_gen;
        let child_row = deepest_anc - child_gen;
        let parent_y_bottom = parent_row as f64 * (CARD_H + V_GAP) + CARD_H;
        let child_y_top = child_row as f64 * (CARD_H + V_GAP);
        let mid_y = (parent_y_bottom + child_y_top) / 2.0;

        let parent_slot_w = parent_gen_w / parent_slots as f64;
        let child_slot_w = child_gen_w / child_slots as f64;

        // Each pair of parents (2k, 2k+1) connects to child k.
        for ci in 0..child_slots {
            let father_si = ci * 2;
            let mother_si = ci * 2 + 1;

            let father_cx = parent_x_off + father_si as f64 * parent_slot_w + parent_slot_w / 2.0;
            let mother_cx = parent_x_off + mother_si as f64 * parent_slot_w + parent_slot_w / 2.0;
            let child_cx = child_x_off + ci as f64 * child_slot_w + child_slot_w / 2.0;
            let couple_mid_x = (father_cx + mother_cx) / 2.0;

            // Has at least one real parent?
            let has_father = matches!(
                anc_gens[parent_gen].get(father_si),
                Some(PedigreeSlot::Person(_))
            );
            let has_mother = matches!(
                anc_gens[parent_gen].get(mother_si),
                Some(PedigreeSlot::Person(_))
            );
            let has_child = matches!(anc_gens[child_gen].get(ci), Some(PedigreeSlot::Person(_)));

            if !has_child || (!has_father && !has_mother) {
                continue;
            }

            // Horizontal bar between parents.
            segments.push(Segment {
                x1: father_cx,
                y1: parent_y_bottom + 2.0,
                x2: mother_cx,
                y2: parent_y_bottom + 2.0,
            });
            // Vertical from couple midpoint down to mid_y.
            segments.push(Segment {
                x1: couple_mid_x,
                y1: parent_y_bottom + 2.0,
                x2: couple_mid_x,
                y2: mid_y,
            });
            // Horizontal at mid_y from couple_mid to child_cx.
            if (couple_mid_x - child_cx).abs() > 0.5 {
                segments.push(Segment {
                    x1: couple_mid_x,
                    y1: mid_y,
                    x2: child_cx,
                    y2: mid_y,
                });
            }
            // Vertical from mid_y down to child top.
            segments.push(Segment {
                x1: child_cx,
                y1: mid_y,
                x2: child_cx,
                y2: child_y_top - 2.0,
            });
        }
    }

    // ── Root node + spouse nodes ──
    let root_y = deepest_anc as f64 * (CARD_H + V_GAP);
    let root_x = (total_width - CARD_W) / 2.0;
    nodes.push(LayoutNode {
        id: Some(root_id),
        x: root_x,
        y: root_y,
        generation: 0,
        slot_idx: 0,
        child_of: None,
        is_father: false,
        family_id: None,
    });

    let mut person_cx: HashMap<Uuid, f64> = HashMap::new();
    let mut family_cx: HashMap<Uuid, f64> = HashMap::new();
    person_cx.insert(root_id, root_x + CARD_W / 2.0);

    for (si, &(spouse_id, fid)) in root_spouses_with_families.iter().enumerate() {
        let spouse_x = root_x + (si + 1) as f64 * STEP;
        nodes.push(LayoutNode {
            id: Some(spouse_id),
            x: spouse_x,
            y: root_y,
            generation: 0,
            slot_idx: si + 1,
            child_of: None,
            is_father: false,
            family_id: Some(fid),
        });
        person_cx.insert(spouse_id, spouse_x + CARD_W / 2.0);

        // Couple connector: horizontal line between root and spouse at card mid-height.
        let conn_y = root_y + CARD_H / 2.0;
        segments.push(Segment {
            x1: root_x + CARD_W,
            y1: conn_y,
            x2: spouse_x,
            y2: conn_y,
        });

        // Family center (midpoint between root and spouse).
        let fc = (root_x + CARD_W / 2.0 + spouse_x + CARD_W / 2.0) / 2.0;
        family_cx.insert(fid, fc);
    }

    for (di, dg) in desc_gens.iter().enumerate() {
        let desc_row = deepest_anc + 1 + di;
        let desc_y = desc_row as f64 * (CARD_H + V_GAP);
        let parent_y_bottom = (desc_row as f64 - 1.0) * (CARD_H + V_GAP) + CARD_H;
        let mid_y = (parent_y_bottom + desc_y) / 2.0;

        // Compute total width of this generation.
        let mut gen_total_w: f64 = 0.0;
        for (fi, fam) in dg.families.iter().enumerate() {
            gen_total_w += fam.children.len().max(1) as f64 * STEP - H_GAP;
            if fi < dg.families.len() - 1 {
                gen_total_w += STEP;
            }
        }
        let gen_x_off = (total_width - gen_total_w) / 2.0;

        let mut x_cursor = gen_x_off;
        for (fi, fam) in dg.families.iter().enumerate() {
            let child_count = fam.children.len();
            let fam_w = child_count.max(1) as f64 * STEP - H_GAP;

            // Find parent/family center x — prefer family center (couple midpoint) if available.
            let parent_cx = family_cx.get(&fam.family_id).copied().unwrap_or_else(|| {
                person_cx
                    .get(&fam.parent_id)
                    .copied()
                    .or_else(|| fam.spouse_id.and_then(|sid| person_cx.get(&sid).copied()))
                    .unwrap_or(x_cursor + fam_w / 2.0)
            });

            // Vertical stem from parent to mid_y.
            segments.push(Segment {
                x1: parent_cx,
                y1: parent_y_bottom + 2.0,
                x2: parent_cx,
                y2: mid_y,
            });

            if child_count > 1 {
                // Horizontal bar at mid_y spanning all children.
                let first_child_cx = x_cursor + CARD_W / 2.0;
                let last_child_cx = x_cursor + (child_count - 1) as f64 * STEP + CARD_W / 2.0;
                segments.push(Segment {
                    x1: first_child_cx,
                    y1: mid_y,
                    x2: last_child_cx,
                    y2: mid_y,
                });
                // Connect parent stem to the bar if not aligned.
                if parent_cx < first_child_cx - 0.5 || parent_cx > last_child_cx + 0.5 {
                    let bar_connect = parent_cx.clamp(first_child_cx, last_child_cx);
                    segments.push(Segment {
                        x1: parent_cx,
                        y1: mid_y,
                        x2: bar_connect,
                        y2: mid_y,
                    });
                }
            }

            for (ci, &child_id) in fam.children.iter().enumerate() {
                let child_x = x_cursor + ci as f64 * STEP;
                let child_cx_val = child_x + CARD_W / 2.0;

                nodes.push(LayoutNode {
                    id: Some(child_id),
                    x: child_x,
                    y: desc_y,
                    generation: (di + 1) as i32,
                    slot_idx: ci,
                    child_of: None,
                    is_father: false,
                    family_id: Some(fam.family_id),
                });

                person_cx.insert(child_id, child_cx_val);

                // Vertical from mid_y to child top.
                segments.push(Segment {
                    x1: child_cx_val,
                    y1: mid_y,
                    x2: child_cx_val,
                    y2: desc_y - 2.0,
                });
            }

            x_cursor += fam_w;
            if fi < dg.families.len() - 1 {
                x_cursor += STEP;
            }
        }
    }

    (nodes, segments, total_width, total_height)
}

// ── Component ────────────────────────────────────────────────────────────

#[derive(Props, Clone, PartialEq)]
pub struct PedigreeChartProps {
    pub root_person_id: Uuid,
    pub data: PedigreeData,
    pub tree_id: String,
    /// SOSA root person ID from tree settings. When set, ancestors of this
    /// person get a small badge indicator on their card.
    #[props(default)]
    pub sosa_root_person_id: Option<Uuid>,
    /// Pre-computed set of ancestor IDs for the SOSA root (from the closure
    /// table). When provided, used instead of traversing the limited pedigree
    /// graph — ensures badges appear even when jumping to distant ancestors.
    #[props(default)]
    pub sosa_ancestor_ids: Option<std::collections::HashSet<Uuid>>,
    /// Incremented by the parent to force re-centering on the root person,
    /// even when `root_person_id` hasn't changed (e.g. navigating back from
    /// the person profile page).
    #[props(default)]
    pub center_gen: u32,
    pub on_person_click: EventHandler<(Uuid, f64, f64)>,
    pub on_person_navigate: EventHandler<Uuid>,
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
    #[props(default)]
    pub on_add_person: EventHandler<()>,
    #[props(default)]
    pub on_profile_view: EventHandler<Uuid>,
    #[props(default)]
    pub on_settings: EventHandler<()>,
}

#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    let i18n = use_i18n();
    let view_cache = use_view_state_cache();
    let tid_parsed = props.tree_id.parse::<Uuid>().ok();
    let saved = tid_parsed.and_then(|t| view_cache.get_untracked(t));

    // Extract initial values from saved state (or defaults)
    let init_anc = saved.as_ref().map_or(4, |s| s.ancestor_levels);
    let init_desc = saved.as_ref().map_or(3, |s| s.descendant_levels);
    let init_ox = saved.as_ref().map_or(0.0, |s| s.offset_x);
    let init_oy = saved.as_ref().map_or(0.0, |s| s.offset_y);
    let init_sc = saved.as_ref().map_or(1.0, |s| s.scale);
    let has_saved_state = saved.is_some();

    // ── Depth controls (max 10) ──
    let mut ancestor_levels = use_signal(move || init_anc);
    let mut descendant_levels = use_signal(move || init_desc);
    let mut depth_hover = use_signal(|| false);
    let mut depth_hover_gen = use_signal(|| 0u32);

    // ── Pan state ──
    let mut offset_x = use_signal(move || init_ox);
    let mut offset_y = use_signal(move || init_oy);
    let mut dragging = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0f64);
    let mut drag_start_y = use_signal(|| 0.0f64);
    let mut drag_origin_x = use_signal(|| 0.0f64);
    let mut drag_origin_y = use_signal(|| 0.0f64);

    // ── Zoom state ──
    let mut scale = use_signal(move || init_sc);

    // ── Selected person (drives event panel) ──
    let mut selected_person_id = use_signal(|| props.root_person_id);

    // ── Event panel collapse (persisted via localStorage) ──
    let mut panel_collapsed = use_signal(|| false);
    let mut panel_init = use_signal(|| false);
    if !panel_init() {
        panel_init.set(true);
        spawn(async move {
            if let Ok(val) =
                document::eval("return localStorage.getItem('oxidgene-ev-panel') === 'collapsed'")
                    .await
                && let Some(b) = val.as_bool()
            {
                panel_collapsed.set(b);
            }
        });
    }

    // ── Disable transition when root changes (avoid flying animation) ──
    let mut animating = use_signal(|| false);

    // ── Center the root in the viewport on first load and root change ──
    // Also center when explicitly requested via center_gen > 0 (e.g. navigation
    // from search results), even when there is saved pan/zoom state.
    let mut needs_center = use_signal(move || !has_saved_state || props.center_gen > 0);

    // ── Reset pan/zoom/selection when the root person changes ──
    let mut prev_root = use_signal(|| props.root_person_id);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        animating.set(false);
        scale.set(1.0);
        selected_person_id.set(props.root_person_id);
        needs_center.set(true);
    }

    // ── Force re-centering when parent increments center_gen ──
    let mut prev_center_gen = use_signal(|| props.center_gen);
    if prev_center_gen() != props.center_gen {
        prev_center_gen.set(props.center_gen);
        animating.set(false);
        scale.set(1.0);
        needs_center.set(true);
    }

    // ── Force re-centering when depth levels change ──
    let anc_now = ancestor_levels();
    let desc_now = descendant_levels();
    let mut prev_anc = use_signal(|| anc_now);
    let mut prev_desc = use_signal(|| desc_now);
    if prev_anc() != anc_now || prev_desc() != desc_now {
        prev_anc.set(anc_now);
        prev_desc.set(desc_now);
        animating.set(false);
        needs_center.set(true);
    }

    // ── Compute layout ──
    let (layout_nodes, connector_segments, total_w, total_h) = compute_layout(
        props.root_person_id,
        &props.data,
        ancestor_levels(),
        descendant_levels(),
    );

    // ── Compute SOSA ancestor set (persons who are ancestors of the SOSA root) ──
    // Use server-provided SOSA ancestor set (from closure table) when available,
    // falling back to local graph traversal (which only works within the pedigree window).
    let sosa_ancestors: std::collections::HashSet<Uuid> = props
        .sosa_ancestor_ids
        .clone()
        .or_else(|| {
            props
                .sosa_root_person_id
                .map(|sosa_id| props.data.ancestor_set(sosa_id))
        })
        .unwrap_or_default();

    // ── Center root card in viewport when needed ──
    if needs_center() {
        let root_center = layout_nodes
            .iter()
            .find(|n| n.id == Some(props.root_person_id))
            .map(|n| (n.x + CARD_W / 2.0, n.y + CARD_H / 2.0));
        if let Some((rcx, rcy)) = root_center {
            needs_center.set(false);
            spawn(async move {
                // Small delay so the DOM has rendered the viewport element.
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                if let Ok(val) = document::eval(
                    "var el = document.querySelector('.pedigree-viewport'); return el ? [el.clientWidth, el.clientHeight] : [800, 600]"
                ).await {
                    let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(800.0);
                    let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(600.0);
                    offset_x.set(vw / 2.0 - rcx);
                    offset_y.set(vh / 2.0 - rcy);
                }
                // Re-enable animation after centering.
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                animating.set(true);
            });
        }
    }

    // ── Persist view state into global cache so it survives navigation ──
    {
        let ox = offset_x();
        let oy = offset_y();
        let sc = scale();
        let anc = ancestor_levels();
        let desc = descendant_levels();
        let root = props.root_person_id;
        if let Some(tid) = tid_parsed {
            view_cache.save(PedigreeViewState {
                tree_id: tid,
                offset_x: ox,
                offset_y: oy,
                scale: sc,
                ancestor_levels: anc,
                descendant_levels: desc,
                selected_root: Some(root),
            });
        }
    }

    let transform = format!(
        "translate({}px, {}px) scale({})",
        offset_x(),
        offset_y(),
        scale()
    );
    let zoom_pct = (scale() * 100.0) as u32;

    let inner_class = if animating() {
        "pedigree-inner pedigree-animated"
    } else {
        "pedigree-inner"
    };

    // ── Event panel data (selected person) ──
    let sel_pid = selected_person_id();
    let (sel_given, sel_surname, _) = props.data.name_parts(sel_pid);
    let sel_given_s = sel_given.unwrap_or_default();
    let sel_surname_s = sel_surname.unwrap_or_default();
    let sel_full_name = match (sel_given_s.is_empty(), sel_surname_s.is_empty()) {
        (true, true) => "Unknown".to_string(),
        (false, true) => sel_given_s.clone(),
        (true, false) => sel_surname_s.clone(),
        _ => format!("{} {}", sel_given_s, sel_surname_s),
    };
    let sel_initials = make_initials(&sel_given_s, &sel_surname_s);
    let sel_birth = props.data.birth_date(sel_pid).unwrap_or_default();
    let sel_death = props.data.death_date(sel_pid).unwrap_or_default();
    let sel_dates = match (sel_birth.is_empty(), sel_death.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("b. {sel_birth}"),
        (true, false) => format!("d. {sel_death}"),
        _ => format!("{sel_birth} \u{2013} {sel_death}"),
    };

    // Collect all events relevant to this person:
    // 1. Individual events (birth, death, occupation, etc.)
    let mut sel_events: Vec<DomainEvent> = props
        .data
        .events_by_person
        .get(&sel_pid)
        .cloned()
        .unwrap_or_default();
    // 2. Conjugal family events (marriage, divorce, etc.)
    if let Some(fam_ids) = props.data.families_as_spouse.get(&sel_pid) {
        for fid in fam_ids {
            if let Some(fam_events) = props.data.events_by_family.get(fid) {
                sel_events.extend(fam_events.iter().cloned());
            }
            // Also include major life events of children (birth, death, baptism, burial).
            if let Some(children) = props.data.children_by_family.get(fid) {
                for child in children {
                    if let Some(child_events) = props.data.events_by_person.get(&child.person_id) {
                        for ce in child_events {
                            if ce.event_type == EventType::Birth
                                || ce.event_type == EventType::Death
                                || ce.event_type == EventType::Baptism
                                || ce.event_type == EventType::Burial
                            {
                                sel_events.push(ce.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    // 3. Parental family events (sibling birth, parent death, etc.)
    if let Some(fam_ids) = props.data.families_as_child.get(&sel_pid) {
        for fid in fam_ids {
            if let Some(fam_events) = props.data.events_by_family.get(fid) {
                sel_events.extend(fam_events.iter().cloned());
            }
            // Also include individual events of family members (parents, siblings).
            if let Some(spouses) = props.data.spouses_by_family.get(fid) {
                for spouse in spouses {
                    if let Some(parent_events) = props.data.events_by_person.get(&spouse.person_id)
                    {
                        for pe in parent_events {
                            // Include major life events of parents (death, burial).
                            if pe.event_type == EventType::Death
                                || pe.event_type == EventType::Burial
                            {
                                sel_events.push(pe.clone());
                            }
                        }
                    }
                }
            }
            if let Some(children) = props.data.children_by_family.get(fid) {
                for child in children {
                    if child.person_id == sel_pid {
                        continue; // Skip self.
                    }
                    if let Some(sib_events) = props.data.events_by_person.get(&child.person_id) {
                        for se in sib_events {
                            // Include major life events of siblings (birth, death).
                            if se.event_type == EventType::Birth
                                || se.event_type == EventType::Death
                                || se.event_type == EventType::Baptism
                                || se.event_type == EventType::Burial
                            {
                                sel_events.push(se.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    // Deduplicate by event ID and sort by date.
    sel_events.sort_by(|a, b| a.id.cmp(&b.id));
    sel_events.dedup_by_key(|e| e.id);
    sel_events.sort_by(|a, b| a.date_sort.cmp(&b.date_sort));

    // Group events by year for display.
    let mut event_groups: Vec<(String, Vec<DomainEvent>)> = Vec::new();
    for evt in &sel_events {
        let year = evt
            .date_value
            .as_deref()
            .map(fmt_year)
            .unwrap_or_else(|| "Unknown".to_string());
        if let Some(last) = event_groups.last_mut()
            && last.0 == year
        {
            last.1.push(evt.clone());
            continue;
        }
        event_groups.push((year, vec![evt.clone()]));
    }

    // ── Fit-to-content zoom calculation ──
    let fit_total_w = total_w;
    let fit_total_h = total_h;

    rsx! {
        div { class: "pedigree-outer",

            // ══════════════════════════════════
            // ICON SIDEBAR
            // ══════════════════════════════════
            div { class: "isb",
                // Tree view (active)
                button {
                    class: "isb-btn isb-btn-active",
                    title: "{i18n.t(\"pedigree.tree_view\")}",
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        // Tree/sitemap icon
                        line { x1: "12", y1: "2", x2: "12", y2: "8" }
                        rect { x: "8", y: "8", width: "8", height: "4", rx: "1" }
                        line { x1: "12", y1: "12", x2: "12", y2: "15" }
                        line { x1: "6", y1: "15", x2: "18", y2: "15" }
                        line { x1: "6", y1: "15", x2: "6", y2: "18" }
                        line { x1: "18", y1: "15", x2: "18", y2: "18" }
                        rect { x: "2", y: "18", width: "8", height: "4", rx: "1" }
                        rect { x: "14", y: "18", width: "8", height: "4", rx: "1" }
                    }
                }

                // Profile view
                {
                    let sel = selected_person_id();
                    let on_profile = props.on_profile_view;
                    rsx! {
                        button {
                            class: "isb-btn",
                            title: "{i18n.t(\"pedigree.profile_view\")}",
                            onclick: move |_| on_profile.call(sel),
                            svg {
                                width: "16",
                                height: "16",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                // Person icon
                                circle { cx: "12", cy: "8", r: "4" }
                                path { d: "M4 21v-1a6 6 0 0 1 12 0v1" }
                            }
                        }
                    }
                }

                div { class: "isb-hr" }

                // Depth selector (hover popover)
                div {
                    class: "isb-depth-wrap",
                    onmouseenter: move |_| {
                        // Bump generation to cancel any pending close task.
                        depth_hover_gen += 1;
                        depth_hover.set(true);
                    },
                    onmouseleave: move |_| {
                        // Close after 200ms unless mouse re-enters (generation changes).
                        let leave_gen = depth_hover_gen();
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                            if depth_hover_gen() == leave_gen {
                                depth_hover.set(false);
                            }
                        });
                    },
                    button {
                        class: "isb-btn",
                        title: "{i18n.t(\"pedigree.depth\")}",
                        svg {
                            width: "16",
                            height: "16",
                            fill: "none",
                            "viewBox": "0 0 24 24",
                            stroke: "currentColor",
                            "strokeWidth": "2",
                            // Layers/depth icon
                            path { d: "M12 2 2 7l10 5 10-5-10-5z" }
                            path { d: "M2 17l10 5 10-5" }
                            path { d: "M2 12l10 5 10-5" }
                        }
                    }
                    if depth_hover() {
                        div { class: "pedigree-depth-popover",
                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-arrow", "\u{2191}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() > 0 { ancestor_levels -= 1; } },
                                    "\u{2212}" // −
                                }
                                span { class: "pedigree-depth-val", "{ancestor_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() < 10 { ancestor_levels += 1; } },
                                    "+"
                                }
                            }
                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-arrow", "\u{2193}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() > 0 { descendant_levels -= 1; } },
                                    "\u{2212}"
                                }
                                span { class: "pedigree-depth-val", "{descendant_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() < 10 { descendant_levels += 1; } },
                                    "+"
                                }
                            }
                        }
                    }
                }

                div { class: "isb-hr" }

                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.zoom_in\")}",
                    onclick: move |_| scale.set((scale() * 1.2).clamp(0.3, 2.0)),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "11", cy: "11", r: "8" }
                        line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                        line { x1: "11", y1: "8", x2: "11", y2: "14" }
                        line { x1: "8", y1: "11", x2: "14", y2: "11" }
                    }
                }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.zoom_out\")}",
                    onclick: move |_| scale.set((scale() / 1.2).clamp(0.3, 2.0)),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "11", cy: "11", r: "8" }
                        line { x1: "21", y1: "21", x2: "16.65", y2: "16.65" }
                        line { x1: "8", y1: "11", x2: "14", y2: "11" }
                    }
                }
                span { class: "isb-zoom-val", "{zoom_pct}%" }
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.fit_screen\")}",
                    onclick: move |_| {
                        spawn(async move {
                            if let Ok(val) = document::eval(
                                "var el = document.querySelector('.pedigree-viewport'); return el ? [el.clientWidth, el.clientHeight] : [800, 600]"
                            ).await {
                                let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(800.0);
                                let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(600.0);
                                let fit_scale = (vw / fit_total_w).min(vh / fit_total_h).clamp(0.3, 2.0) * 0.85;
                                scale.set(fit_scale);
                                // Center the content in the viewport.
                                offset_x.set((vw - fit_total_w * fit_scale) / 2.0);
                                offset_y.set((vh - fit_total_h * fit_scale) / 2.0);
                            }
                        });
                    },
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        // Maximize/fit-screen icon (four corners)
                        path { d: "M3 8V5a2 2 0 0 1 2-2h3" }
                        path { d: "M16 3h3a2 2 0 0 1 2 2v3" }
                        path { d: "M21 16v3a2 2 0 0 1-2 2h-3" }
                        path { d: "M8 21H5a2 2 0 0 1-2-2v-3" }
                    }
                }

                div { class: "isb-hr" }

                // Add person
                {
                    let on_add = props.on_add_person;
                    rsx! {
                        button {
                            class: "isb-btn",
                            title: "{i18n.t(\"pedigree.add_person\")}",
                            onclick: move |_| on_add.call(()),
                            svg {
                                width: "16",
                                height: "16",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                // Person + plus icon
                                circle { cx: "10", cy: "8", r: "4" }
                                path { d: "M2 21v-1a6 6 0 0 1 12 0v1" }
                                line { x1: "20", y1: "8", x2: "20", y2: "14" }
                                line { x1: "17", y1: "11", x2: "23", y2: "11" }
                            }
                        }
                    }
                }

                div { class: "isb-hr" }

                // Settings
                {
                    let on_settings = props.on_settings;
                    rsx! {
                        button {
                            class: "isb-btn",
                            title: "{i18n.t(\"settings.breadcrumb\")}",
                            onclick: move |_| on_settings.call(()),
                            svg {
                                width: "16",
                                height: "16",
                                fill: "none",
                                "viewBox": "0 0 24 24",
                                stroke: "currentColor",
                                "strokeWidth": "2",
                                // Gear icon
                                circle { cx: "12", cy: "12", r: "3" }
                                path { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" }
                            }
                        }
                    }
                }
            }

            // ══════════════════════════════════
            // CANVAS VIEWPORT
            // ══════════════════════════════════
            div {
                class: "pedigree-viewport",

                onpointerdown: move |evt| {
                    let coords = evt.client_coordinates();
                    drag_start_x.set(coords.x);
                    drag_start_y.set(coords.y);
                    drag_origin_x.set(offset_x());
                    drag_origin_y.set(offset_y());
                    dragging.set(true);
                },
                onpointermove: move |evt| {
                    if dragging() {
                        let coords = evt.client_coordinates();
                        offset_x.set(drag_origin_x() + coords.x - drag_start_x());
                        offset_y.set(drag_origin_y() + coords.y - drag_start_y());
                    }
                },
                onpointerup: move |_| { dragging.set(false); },
                onpointerleave: move |_| { dragging.set(false); },
                onwheel: move |evt| {
                    let delta_y = match evt.delta() {
                        WheelDelta::Lines(l) => l.y * 20.0,
                        WheelDelta::Pixels(p) => p.y,
                        WheelDelta::Pages(p) => p.y * 400.0,
                    };
                    let factor = if delta_y > 0.0 { 0.9 } else { 1.0 / 0.9 };
                    scale.set((scale() * factor).clamp(0.3, 2.0));
                },

                div {
                    class: inner_class,
                    style: "transform: {transform};",

                    div {
                        class: "pedigree-tree",
                        style: "position: relative; width: {total_w}px; height: {total_h}px;",

                        // SVG connector overlay
                        svg {
                            class: "pedigree-svg-connectors",
                            "viewBox": "0 0 {total_w} {total_h}",
                            width: "{total_w}",
                            height: "{total_h}",
                            for (si, seg) in connector_segments.iter().enumerate() {
                                line {
                                    key: "seg-{si}",
                                    x1: "{seg.x1}",
                                    y1: "{seg.y1}",
                                    x2: "{seg.x2}",
                                    y2: "{seg.y2}",
                                    class: "pedigree-connector-line",
                                }
                            }
                        }

                        // Person cards (absolute positioned)
                        for (ni, node) in layout_nodes.iter().enumerate() {
                            {
                                let node = node.clone();
                                let style = format!(
                                    "position: absolute; left: {}px; top: {}px; width: {}px;",
                                    node.x, node.y, CARD_W
                                );

                                match node.id {
                                    Some(pid) => {
                                        let sex = props.data.sex_of(pid);
                                        let (given, surname, _) = props.data.name_parts(pid);
                                        let has_name = given.is_some() || surname.is_some();
                                        let given_s = given.unwrap_or_default();
                                        let surname_s = surname.unwrap_or_default();
                                        let birth_s = props.data.birth_date(pid).unwrap_or_default();
                                        let death_s = props.data.death_date(pid).unwrap_or_default();
                                        let birth_sym = props.data.birth_symbol(pid);
                                        let death_sym = props.data.death_symbol(pid);
                                        let initials = make_initials(&given_s, &surname_s);
                                        let is_focus = pid == props.root_person_id;
                                        let is_sosa_ancestor = sosa_ancestors.contains(&pid);
                                        let is_sosa_root = props.sosa_root_person_id == Some(pid);
                                        let role_class = if node.generation == 0 {
                                            "current"
                                        } else if node.generation < 0 {
                                            "ancestor"
                                        } else {
                                            "descendant"
                                        };
                                        let sex_part = match sex {
                                            Sex::Male => "male",
                                            Sex::Female => "female",
                                            Sex::Unknown => "",
                                        };
                                        let is_sel = selected_person_id() == pid;
                                        let sel_part = if is_sel || is_focus { "selected" } else { "" };
                                        let node_class = format!(
                                            "pedigree-node {} {} {}",
                                            sex_part, role_class, sel_part
                                        );

                                        let on_navigate = props.on_person_navigate;
                                        let on_click = props.on_person_click;

                                        rsx! {
                                            div {
                                                key: "node-{ni}",
                                                class: "pedigree-card-wrap",
                                                style: style,
                                                div {
                                                    class: node_class,
                                                    onclick: move |_| {
                                                        selected_person_id.set(pid);
                                                        on_navigate.call(pid);
                                                    },
                                                    div { class: "pc-ph",
                                                        "{initials}"
                                                        if is_sosa_root || is_sosa_ancestor {
                                                            span {
                                                                class: if is_sosa_root { "sosa-badge sosa-badge-root" } else { "sosa-badge" },
                                                                title: if is_sosa_root { "SOSA 1" } else { "SOSA" },
                                                            }
                                                        }
                                                    }
                                                    div { class: "pc-body",
                                                        div { class: "pc-name",
                                                            if !surname_s.is_empty() {
                                                                span { class: "pc-last", "{surname_s}" }
                                                            }
                                                            if !given_s.is_empty() {
                                                                span { class: "pc-first", "{given_s}" }
                                                            }
                                                            if !has_name {
                                                                span { class: "pc-first", {i18n.t("common.unknown")} }
                                                            }
                                                        }
                                                        if !birth_s.is_empty() || !death_s.is_empty() {
                                                            div { class: "pc-dates",
                                                                if !birth_s.is_empty() {
                                                                    span { class: "pc-born", "{birth_sym} {birth_s}" }
                                                                }
                                                                if !death_s.is_empty() {
                                                                    span { class: "pc-died", "{death_sym} {death_s}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                // Pencil FAB on focus person only
                                                if is_focus {
                                                    button {
                                                        class: "pedigree-edit-fab",
                                                        title: "{i18n.t(\"pedigree.edit_actions\")}",
                                                        onclick: move |evt: Event<MouseData>| {
                                                            evt.stop_propagation();
                                                            let coords = evt.client_coordinates();
                                                            on_click.call((pid, coords.x, coords.y));
                                                        },
                                                        "\u{270E}" // ✎
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    None => {
                                        // Empty placeholder slot
                                        let child_id = node.child_of;
                                        let is_father = node.is_father;
                                        if let Some(cid) = child_id {
                                            let on_empty = props.on_empty_slot;
                                            rsx! {
                                                div {
                                                    key: "node-{ni}",
                                                    style: style,
                                                    button {
                                                        class: "pedigree-node empty-slot",
                                                        onclick: move |_| on_empty.call((cid, is_father)),
                                                        "+"
                                                    }
                                                }
                                            }
                                        } else {
                                            rsx! {
                                                div {
                                                    key: "node-{ni}",
                                                    style: style,
                                                    div { class: "pedigree-node empty-slot disabled" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ══════════════════════════════════
            // EVENT PANEL
            // ══════════════════════════════════
            div {
                class: if panel_collapsed() { "ev-panel ev-panel-collapsed" } else { "ev-panel" },
                button {
                    class: "evp-toggle",
                    title: if panel_collapsed() { i18n.t("pedigree.events") } else { i18n.t("pedigree.hide_events") },
                    onclick: move |_| {
                        let new_val = !panel_collapsed();
                        panel_collapsed.set(new_val);
                        let val = if new_val { "collapsed" } else { "open" };
                        document::eval(&format!(
                            "localStorage.setItem('oxidgene-ev-panel', '{}')",
                            val
                        ));
                    },
                    if panel_collapsed() { "\u{203A}" } else { "\u{2039}" }
                }
                if !panel_collapsed() {
                    div { class: "evp-hd", {i18n.t("pedigree.events")} }
                    div { class: "evp-person",
                        div { class: "evp-av", "{sel_initials}" }
                        div { class: "evp-name",
                            strong { "{sel_full_name}" }
                            if !sel_dates.is_empty() {
                                span { "{sel_dates}" }
                            }
                        }
                    }
                    div { class: "evp-list",
                        if sel_events.is_empty() {
                            div { class: "evp-empty", {i18n.t("person_form.no_other_events")} }
                        } else {
                            for (gi, (year, events)) in event_groups.iter().enumerate() {
                                {
                                    let year = year.clone();
                                    let events = events.clone();
                                    let tree_id = props.tree_id.clone();
                                    rsx! {
                                        div { key: "evg-{gi}", class: "ev-year-group",
                                            div { class: "ev-year-header", "{year}" }
                                            for (ei, evt) in events.iter().enumerate() {
                                                {
                                                    let (icon, ic_class, label_key) = event_ui(evt.event_type);
                                                    let label = i18n.t(label_key);
                                                    let date_s = evt.date_value.clone().unwrap_or_default();
                                                    let place_s = evt.place_id
                                                        .and_then(|pid| props.data.place_name(pid).map(String::from))
                                                        .or_else(|| evt.description.clone())
                                                        .unwrap_or_default();
                                                    // Build context label for events from related persons.
                                                    let context_name: Option<String> = if evt.person_id.is_some() && evt.person_id != Some(sel_pid) {
                                                        evt.person_id.map(|pid| props.data.display_name(pid))
                                                    } else if evt.family_id.is_some() && evt.person_id.is_none() {
                                                        // Family event (marriage, divorce…) — show partner name.
                                                        evt.family_id.and_then(|fid| {
                                                            props.data.spouses_by_family.get(&fid).and_then(|spouses| {
                                                                spouses.iter()
                                                                    .find(|s| s.person_id != sel_pid)
                                                                    .map(|s| props.data.display_name(s.person_id))
                                                            })
                                                        })
                                                    } else {
                                                        None
                                                    };
                                                    let full_label = if let Some(ref ctx) = context_name {
                                                        format!("{label} ({ctx})")
                                                    } else {
                                                        label
                                                    };
                                                    let sel = sel_pid;
                                                    let tid = tree_id.clone();
                                                    let nav = use_navigator();
                                                    rsx! {
                                                        div {
                                                            key: "ev-{gi}-{ei}",
                                                            class: "ev-item ev-item-clickable",
                                                            onclick: move |_| {
                                                                nav.push(crate::router::Route::PersonDetail {
                                                                    tree_id: tid.clone(),
                                                                    person_id: sel.to_string(),
                                                                });
                                                            },
                                                            div { class: ic_class, "{icon}" }
                                                            div { class: "ev-info",
                                                                div { class: "ev-type", "{full_label}" }
                                                                if !date_s.is_empty() {
                                                                    div { class: "ev-date", "{date_s}" }
                                                                }
                                                                if !place_s.is_empty() {
                                                                    div { class: "ev-place", "{place_s}" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
