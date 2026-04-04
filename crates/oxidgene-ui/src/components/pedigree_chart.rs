//! Vertical bidirectional pedigree chart with pan/zoom, icon sidebar, and event panel.
//!
//! Layout: `.pedigree-outer` (flex row)
//!   -> `.isb` (icon sidebar: depth/zoom controls)
//!   -> `.pedigree-viewport` (pannable/zoomable canvas)
//!   -> `.ev-panel` (selected-person event list)
//!
//! Cards are positioned using the Reingold-Tilford (Buchheim variant) algorithm,
//! the same algorithm used by Geneanet's tree view.
//! Connectors are drawn via SVG overlay with Bézier curves.

use std::collections::{HashMap, HashSet};

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

// ── Layout constants (matching the JS reference implementation) ──────────

/// Standard card width in pixels.
const CARD_W: f64 = 185.0;
/// Standard card height in pixels.
const CARD_H: f64 = 96.0;
/// Compact ancestor card width (for deepest level).
const COMPACT_W: f64 = 95.0;
/// Compact ancestor card height.
const COMPACT_H: f64 = 144.0;
/// First descendant level height.
const DESC_H: f64 = 140.0;

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
    /// person_id → photo URL (file_path from media table).
    pub photos: HashMap<Uuid, String>,
    /// Pre-computed SOSA ancestor set (persons who are ancestors of the SOSA root).
    pub sosa_ancestors: HashSet<Uuid>,
    /// The SOSA root person ID (from tree settings).
    pub sosa_root_id: Option<Uuid>,
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
            photos: HashMap::new(),
            sosa_ancestors: HashSet::new(),
            sosa_root_id: None,
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
            photos: HashMap::new(),
            sosa_ancestors: HashSet::new(),
            sosa_root_id: None,
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

// ── RT Layout Engine ─────────────────────────────────────────────────────
//
// Port of the Reingold-Tilford (Buchheim variant) algorithm from the
// JavaScript reference at test-tree-algo/layout.js + tree-builder.js.
// All traversals use explicit stacks to avoid stack overflows on deep trees.

/// SOSA badge type for a node.
#[derive(Clone, Debug, PartialEq)]
enum SosaBadge {
    None,
    Root,
    Direct,
}

/// A node in the layout tree arena.
#[derive(Clone, Debug)]
struct TreeNode {
    id: Option<Uuid>,
    depth: i32,
    sex: Sex,
    label_surname: String,
    label_given: String,
    birth_year: Option<i32>,
    death_year: Option<i32>,
    photo_url: Option<String>,
    sosa_badge: SosaBadge,
    /// Indices into the TreeNode arena of children (for RT traversal).
    children: Vec<usize>,
    /// Spouse node indices (siblings in RT terms).
    siblings: Vec<usize>,
    parent2: Option<usize>,
    /// 0 = male-first ordering, 1 = female-first.
    after: i32,
    is_sibling: bool,
    before_sibling: bool,
    after_sibling: bool,
    x: f64,
    y: f64,
    #[allow(dead_code)]
    show: bool,
    /// For empty ancestor slots: which child they belong to.
    child_of: Option<Uuid>,
    /// For empty ancestor slots: is this the father slot?
    is_father: bool,
}

impl TreeNode {
    #[allow(clippy::too_many_arguments)]
    fn new_real(
        id: Uuid,
        depth: i32,
        sex: Sex,
        given: String,
        surname: String,
        birth_year: Option<i32>,
        death_year: Option<i32>,
        photo_url: Option<String>,
        sosa_badge: SosaBadge,
        after: i32,
        before_sibling: bool,
        after_sibling: bool,
    ) -> Self {
        Self {
            id: Some(id),
            depth,
            sex,
            label_surname: surname,
            label_given: given,
            birth_year,
            death_year,
            photo_url,
            sosa_badge,
            children: vec![],
            siblings: vec![],
            parent2: None,
            after,
            is_sibling: false,
            before_sibling,
            after_sibling,
            x: 0.0,
            y: 0.0,
            show: true,
            child_of: None,
            is_father: false,
        }
    }

    fn new_empty(depth: i32, child_of: Option<Uuid>, is_father: bool) -> Self {
        Self {
            id: None,
            depth,
            sex: Sex::Unknown,
            label_surname: String::new(),
            label_given: String::new(),
            birth_year: None,
            death_year: None,
            photo_url: None,
            sosa_badge: SosaBadge::None,
            children: vec![],
            siblings: vec![],
            parent2: None,
            after: 0,
            is_sibling: false,
            before_sibling: false,
            after_sibling: false,
            x: 0.0,
            y: 0.0,
            show: true,
            child_of,
            is_father,
        }
    }
}

/// Working node for the Reingold-Tilford algorithm.
#[derive(Clone, Debug)]
struct WrapNode {
    orig: usize,
    parent: Option<usize>,
    children: Vec<usize>,
    siblings: Vec<usize>,
    parent2: Option<usize>,
    z: f64,
    m: f64,
    c: f64,
    s: f64,
    t: Option<usize>,
    /// ancestor pointer (self-index by default)
    a: usize,
    i: usize,
}

/// Connectivity for a single person extracted from pedigree data.
struct PersonNode {
    sex: Sex,
    given: String,
    surname: String,
    birth_year: Option<i32>,
    death_year: Option<i32>,
    photo_url: Option<String>,
    sosa_badge: SosaBadge,
}

impl PersonNode {
    fn from_data(
        id: Uuid,
        data: &PedigreeData,
        sosa_root_id: Option<Uuid>,
        sosa_ancestors: &HashSet<Uuid>,
    ) -> Self {
        let sex = data.sex_of(id);
        let (given, surname, _) = data.name_parts(id);
        let given = given.unwrap_or_default();
        let surname = surname.unwrap_or_default();

        let birth_year = data
            .events_by_person
            .get(&id)
            .and_then(|evts| evts.iter().find(|e| e.event_type == EventType::Birth))
            .and_then(|e| e.date_value.as_deref())
            .and_then(|d| {
                d.split_whitespace()
                    .find(|w| w.len() == 4 && w.parse::<u32>().is_ok())
                    .and_then(|w| w.parse::<i32>().ok())
            });

        let death_year = data
            .events_by_person
            .get(&id)
            .and_then(|evts| evts.iter().find(|e| e.event_type == EventType::Death))
            .and_then(|e| e.date_value.as_deref())
            .and_then(|d| {
                d.split_whitespace()
                    .find(|w| w.len() == 4 && w.parse::<u32>().is_ok())
                    .and_then(|w| w.parse::<i32>().ok())
            });

        let photo_url = data.photos.get(&id).cloned();

        let sosa_badge = if sosa_root_id == Some(id) {
            SosaBadge::Root
        } else if sosa_ancestors.contains(&id) {
            SosaBadge::Direct
        } else {
            SosaBadge::None
        };

        PersonNode {
            sex,
            given,
            surname,
            birth_year,
            death_year,
            photo_url,
            sosa_badge,
        }
    }
}

/// Build the ascending (ancestor) tree into the arena.
/// Returns index of root node in the arena.
fn build_ascending_tree(
    root_id: Uuid,
    data: &PedigreeData,
    max_ascendants: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> Vec<TreeNode> {
    let mut arena: Vec<TreeNode> = Vec::new();

    // Helper: get parents of a person (father, mother).
    let get_parents = |pid: Uuid| -> (Option<Uuid>, Option<Uuid>) { data.parents_of(pid) };

    // Check if root has siblings (children of same parent family).
    let (before_sibling, after_sibling) = {
        let siblings = get_siblings(root_id, data);
        let idx = siblings.iter().position(|&s| s == root_id).unwrap_or(0);
        (idx > 0, idx < siblings.len().saturating_sub(1))
    };

    let root_pn = PersonNode::from_data(root_id, data, sosa_root_id, sosa_ancestors);
    let root_after = if root_pn.sex == Sex::Female { 1 } else { 0 };
    arena.push(TreeNode::new_real(
        root_id,
        0,
        root_pn.sex,
        root_pn.given,
        root_pn.surname,
        root_pn.birth_year,
        root_pn.death_year,
        root_pn.photo_url,
        root_pn.sosa_badge,
        root_after,
        before_sibling,
        after_sibling,
    ));

    // Iterative BFS to build ancestor tree.
    // Stack items: (arena_index, current_depth).
    let mut work: Vec<(usize, i32)> = vec![(0, 0)];
    while let Some((node_idx, depth)) = work.pop() {
        if depth.unsigned_abs() as usize >= max_ascendants {
            continue;
        }
        let pid = match arena[node_idx].id {
            Some(p) => p,
            None => continue,
        };
        let (father_id, mother_id) = get_parents(pid);
        let child_depth = depth - 1;

        let mut child_indices = Vec::new();

        if let Some(fid) = father_id {
            let pn = PersonNode::from_data(fid, data, sosa_root_id, sosa_ancestors);
            let idx = arena.len();
            arena.push(TreeNode::new_real(
                fid,
                child_depth,
                pn.sex,
                pn.given,
                pn.surname,
                pn.birth_year,
                pn.death_year,
                pn.photo_url,
                pn.sosa_badge,
                0,
                false,
                false,
            ));
            child_indices.push(idx);
            work.push((idx, child_depth));
        } else if father_id.is_none() && mother_id.is_some() {
            // Empty father slot.
            let idx = arena.len();
            arena.push(TreeNode::new_empty(child_depth, Some(pid), true));
            child_indices.push(idx);
        }

        if let Some(mid) = mother_id {
            let pn = PersonNode::from_data(mid, data, sosa_root_id, sosa_ancestors);
            let idx = arena.len();
            arena.push(TreeNode::new_real(
                mid,
                child_depth,
                pn.sex,
                pn.given,
                pn.surname,
                pn.birth_year,
                pn.death_year,
                pn.photo_url,
                pn.sosa_badge,
                1,
                false,
                false,
            ));
            child_indices.push(idx);
            work.push((idx, child_depth));
        } else if mother_id.is_none() && father_id.is_some() {
            // Empty mother slot.
            let idx = arena.len();
            arena.push(TreeNode::new_empty(child_depth, Some(pid), false));
            child_indices.push(idx);
        }

        arena[node_idx].children = child_indices;
    }

    arena
}

/// Get ordered siblings of a person from their parent family.
fn get_siblings(pid: Uuid, data: &PedigreeData) -> Vec<Uuid> {
    let Some(fids) = data.families_as_child.get(&pid) else {
        return vec![pid];
    };
    let Some(&fid) = fids.first() else {
        return vec![pid];
    };
    let children: Vec<Uuid> = data
        .children_by_family
        .get(&fid)
        .map(|cs| cs.iter().map(|c| c.person_id).collect())
        .unwrap_or_default();
    if children.is_empty() {
        vec![pid]
    } else {
        children
    }
}

/// Build the descending (descendant) tree into the arena.
fn build_descending_tree(
    root_id: Uuid,
    data: &PedigreeData,
    max_descendants: usize,
    sosa_root_id: Option<Uuid>,
    sosa_ancestors: &HashSet<Uuid>,
) -> Vec<TreeNode> {
    let mut arena: Vec<TreeNode> = Vec::new();

    let (before_sibling, after_sibling) = {
        let siblings = get_siblings(root_id, data);
        let idx = siblings.iter().position(|&s| s == root_id).unwrap_or(0);
        (idx > 0, idx < siblings.len().saturating_sub(1))
    };

    let root_pn = PersonNode::from_data(root_id, data, sosa_root_id, sosa_ancestors);
    let root_after = if root_pn.sex == Sex::Female { 1 } else { 0 };
    arena.push(TreeNode::new_real(
        root_id,
        0,
        root_pn.sex,
        root_pn.given,
        root_pn.surname,
        root_pn.birth_year,
        root_pn.death_year,
        root_pn.photo_url,
        root_pn.sosa_badge,
        root_after,
        before_sibling,
        after_sibling,
    ));

    // Iterative DFS to build descendant tree.
    let mut visited: HashSet<Uuid> = HashSet::new();
    let mut work: Vec<(usize, i32)> = vec![(0, 0)];

    while let Some((node_idx, depth)) = work.pop() {
        let pid = match arena[node_idx].id {
            Some(p) => p,
            None => continue,
        };
        if visited.contains(&pid) {
            continue;
        }
        visited.insert(pid);

        let family_ids: Vec<Uuid> = data
            .families_as_spouse
            .get(&pid)
            .cloned()
            .unwrap_or_default();

        for fid in family_ids {
            let spouse_id = data
                .spouses_by_family
                .get(&fid)
                .and_then(|sps| sps.iter().find(|s| s.person_id != pid))
                .map(|s| s.person_id);

            let Some(sid) = spouse_id else { continue };

            if visited.contains(&sid) {
                continue;
            }

            let spn = PersonNode::from_data(sid, data, sosa_root_id, sosa_ancestors);
            let spouse_after = if spn.sex == Sex::Female { 1 } else { 0 };
            let spouse_arena_idx = arena.len();
            let mut spouse_node = TreeNode::new_real(
                sid,
                depth,
                spn.sex,
                spn.given,
                spn.surname,
                spn.birth_year,
                spn.death_year,
                spn.photo_url,
                spn.sosa_badge,
                spouse_after,
                false,
                false,
            );
            spouse_node.is_sibling = true;
            arena.push(spouse_node);
            arena[node_idx].siblings.push(spouse_arena_idx);

            if depth < max_descendants as i32 {
                let children: Vec<Uuid> = data
                    .children_by_family
                    .get(&fid)
                    .map(|cs| cs.iter().map(|c| c.person_id).collect())
                    .unwrap_or_default();

                for child_id in children {
                    if visited.contains(&child_id) {
                        continue;
                    }
                    let cpn = PersonNode::from_data(child_id, data, sosa_root_id, sosa_ancestors);
                    let child_after = if cpn.sex == Sex::Female { 1 } else { 0 };
                    let child_arena_idx = arena.len();
                    let mut child_node = TreeNode::new_real(
                        child_id,
                        depth + 1,
                        cpn.sex,
                        cpn.given,
                        cpn.surname,
                        cpn.birth_year,
                        cpn.death_year,
                        cpn.photo_url,
                        cpn.sosa_badge,
                        child_after,
                        false,
                        false,
                    );
                    child_node.parent2 = Some(spouse_arena_idx);
                    arena.push(child_node);
                    arena[node_idx].children.push(child_arena_idx);
                    work.push((child_arena_idx, depth + 1));
                }
            }
        }
    }

    arena
}

// ── Reingold-Tilford core ────────────────────────────────────────────────

fn wrap_tree(arena: &[TreeNode]) -> Vec<WrapNode> {
    let n = arena.len();
    // Pre-allocate all wrap nodes (one per arena node plus a virtual root).
    // We use indices into this vec. The virtual root is at index n.
    let mut wrap: Vec<WrapNode> = Vec::with_capacity(n + 1);

    // Initialize one WrapNode per TreeNode.
    for (i, tn) in arena.iter().enumerate() {
        wrap.push(WrapNode {
            orig: i,
            parent: None,
            children: tn.children.clone(),
            siblings: tn.siblings.clone(),
            parent2: tn.parent2,
            z: 0.0,
            m: 0.0,
            c: 0.0,
            s: 0.0,
            t: None,
            a: i, // self by default
            i: 0,
        });
    }

    // Wire parent pointers and child indices.
    // Process children (wire parent = this node, i = position among children).
    for wi in 0..n {
        let children = wrap[wi].children.clone();
        for (ci, &child_idx) in children.iter().enumerate() {
            wrap[child_idx].parent = Some(wi);
            wrap[child_idx].i = ci;
        }
    }

    wrap
}

fn tree_left(wrap: &[WrapNode], v: usize) -> Option<usize> {
    let children = &wrap[v].children;
    if !children.is_empty() {
        Some(children[0])
    } else {
        wrap[v].t
    }
}

fn tree_right(wrap: &[WrapNode], v: usize) -> Option<usize> {
    let children = &wrap[v].children;
    if !children.is_empty() {
        Some(*children.last().unwrap())
    } else {
        wrap[v].t
    }
}

fn tree_move(wrap: &mut [WrapNode], wm: usize, wp: usize, shift: f64) {
    let range = (wrap[wp].i as f64) - (wrap[wm].i as f64);
    if range > 0.0 {
        let change = shift / range;
        wrap[wp].c -= change;
        wrap[wm].c += change;
    }
    wrap[wp].s += shift;
    wrap[wp].z += shift;
    wrap[wp].m += shift;
}

fn tree_ancestor(wrap: &[WrapNode], vim: usize, v: usize, ancestor: usize) -> usize {
    let vim_a = wrap[vim].a;
    if wrap[vim_a].parent == wrap[v].parent {
        vim_a
    } else {
        ancestor
    }
}

fn tree_shift(wrap: &mut [WrapNode], node: usize) {
    let children = wrap[node].children.clone();
    let mut shift = 0.0f64;
    let mut change = 0.0f64;
    for i in (0..children.len()).rev() {
        let w = children[i];
        wrap[w].z += shift;
        wrap[w].m += shift;
        change += wrap[w].c;
        shift += wrap[w].s + change;
    }
}

fn tree_separation(arena: &[TreeNode], a: usize, b: usize, last_level: i32) -> f64 {
    let a_depth = arena[a].depth;
    let a_parent_depth = arena[b].depth; // same depth for siblings
    let b_parent = arena[b].depth;
    let _ = b_parent; // unused
    if a_depth == last_level && a_parent_depth == last_level {
        0.5
    } else if arena[a].depth == arena[b].depth {
        // same parent heuristic via depth equality
        1.0
    } else {
        2.0
    }
}

#[allow(clippy::too_many_arguments)]
fn apportion(
    wrap: &mut [WrapNode],
    arena: &[TreeNode],
    v: usize,
    w: Option<usize>,
    ancestor_in: usize,
    last_level: i32,
) -> usize {
    let mut ancestor = ancestor_in;
    let Some(w) = w else { return ancestor };

    let mut vip = v;
    let mut vop = v;
    let mut vim = w;
    let vom_start = wrap[v].parent.map(|p| wrap[p].children[0]).unwrap_or(v);
    let mut vom = vom_start;
    let mut sip = wrap[vip].m;
    let mut sop = wrap[vop].m;
    let mut sim = wrap[vim].m;
    let mut som = wrap[vom].m;

    loop {
        let vim_right = tree_right(wrap, vim);
        let vip_left = tree_left(wrap, vip);
        if vim_right.is_none() || vip_left.is_none() {
            break;
        }
        let vim_next = vim_right.unwrap();
        let vip_next = vip_left.unwrap();

        // Update vom and vop.
        let vom_left = tree_left(wrap, vom);
        let vop_right = tree_right(wrap, vop);
        if vom_left.is_none() || vop_right.is_none() {
            break;
        }
        vom = vom_left.unwrap();
        let vop_next = vop_right.unwrap();
        wrap[vop_next].a = v;

        // Compute siblings contribution for shift.
        let mut sibling_z: f64 = 0.0;
        for &si in &wrap[vim_next].siblings.clone() {
            sibling_z += wrap[si].z;
        }

        let sep = tree_separation(arena, vim_next, vip_next, last_level);
        let shift = wrap[vim_next].z + sim + sibling_z - wrap[vip_next].z - sip + sep;
        if shift > 0.0 {
            let anc = tree_ancestor(wrap, vim_next, v, ancestor);
            tree_move(wrap, anc, v, shift);
            sip += shift;
            sop += shift;
        }

        sim += wrap[vim_next].m;
        sip += wrap[vip_next].m;
        som += wrap[vom].m;
        sop += wrap[vop_next].m;

        vim = vim_next;
        vip = vip_next;
        vop = vop_next;
    }

    if tree_right(wrap, vim).is_some() && tree_right(wrap, vop).is_none() {
        let vim_r = tree_right(wrap, vim).unwrap();
        wrap[vop].t = Some(vim_r);
        wrap[vop].m += sim - sop;
    }
    if tree_left(wrap, vip).is_some() && tree_left(wrap, vom).is_none() {
        let vip_l = tree_left(wrap, vip).unwrap();
        wrap[vom].t = Some(vip_l);
        wrap[vom].m += sip - som;
        ancestor = v;
    }

    ancestor
}

fn first_walk(wrap: &mut [WrapNode], arena: &[TreeNode], root: usize, last_level: i32) {
    // Iterative post-order via explicit stack.
    let mut post_order: Vec<usize> = Vec::new();
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        post_order.push(v);
        for &c in &wrap[v].children.clone() {
            stack.push(c);
        }
    }
    post_order.reverse();

    for v in post_order {
        let parent = wrap[v].parent;
        let siblings_in_parent = parent.map(|p| wrap[p].children.clone()).unwrap_or_default();
        let prev_sibling = if wrap[v].i > 0 {
            siblings_in_parent.get(wrap[v].i - 1).copied()
        } else {
            None
        };

        // Determine effective children: filter by first sibling's parent2 if node has siblings.
        let node_siblings = wrap[v].siblings.clone();
        let orig_children = wrap[v].children.clone();
        let effective_children: Vec<usize> = if node_siblings.is_empty() {
            orig_children.clone()
        } else {
            // Filter children belonging to first sibling (spouse).
            let first_sib_orig = node_siblings.first().map(|&si| wrap[si].orig);
            orig_children
                .iter()
                .copied()
                .filter(|&ci| wrap[ci].parent2.map(|p2| wrap[p2].orig) == first_sib_orig)
                .collect()
        };

        if !effective_children.is_empty() {
            tree_shift(wrap, v);

            let mut midpoint = 0.0f64;
            if !node_siblings.is_empty() && (arena[v].after != 1 || effective_children.len() == 1) {
                midpoint -= 0.5;
            }

            // Adjustment for female-first nodes (after=1) with siblings.
            let first_child = effective_children[0];
            let last_child = *effective_children.last().unwrap();
            let mut m_adj = 0.0f64;
            if arena[wrap[first_child].orig].after == 1 {
                let fc_sibs = wrap[first_child].siblings.clone();
                if let Some(&last_fc_sib) = fc_sibs.last() {
                    m_adj += wrap[last_fc_sib].z;
                }
            }
            let last_child_orig = wrap[last_child].orig;
            if arena[last_child_orig].after == 1 {
                let lc_sibs = wrap[last_child].siblings.clone();
                if let Some(&last_lc_sib) = lc_sibs.last() {
                    m_adj += wrap[last_lc_sib].z;
                }
            }

            let last_sib_z = {
                let lc_sibs = wrap[last_child].siblings.clone();
                lc_sibs.last().map(|&s| wrap[s].z).unwrap_or(0.0)
            };
            midpoint += (wrap[first_child].z + wrap[last_child].z + last_sib_z + m_adj) / 2.0;

            // Special case for 2 children at deepest level.
            if effective_children.len() == 2 && arena[wrap[first_child].orig].depth == last_level {
                midpoint -= 0.25;
            }

            match prev_sibling {
                Some(w) => {
                    let w_sib_z = wrap[w].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
                    let sep = if parent.is_some() {
                        let w_orig = wrap[w].orig;
                        let v_orig = wrap[v].orig;
                        // Same parent = 1, else 2
                        if arena[v_orig].depth == arena[w_orig].depth {
                            1.0
                        } else {
                            2.0
                        }
                    } else {
                        1.0
                    };
                    wrap[v].z = wrap[w].z + w_sib_z + sep;
                    wrap[v].m = wrap[v].z - midpoint;
                }
                None => {
                    wrap[v].z = midpoint;
                }
            }
        } else if let Some(w) = prev_sibling {
            let w_sib_z = wrap[w].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
            wrap[v].z = wrap[w].z + w_sib_z + 1.0;
        }

        // Multi-spouse positioning (simplified port).
        let mut last_z = 0.0f64;
        let node_siblings_clone = wrap[v].siblings.clone();
        let orig_children_clone = wrap[v].children.clone();

        if !node_siblings_clone.is_empty() && arena[v].after == 1 && effective_children.len() != 1 {
            wrap[v].m -= 0.5;
        }

        for (si, &sib_wi) in node_siblings_clone.iter().enumerate() {
            let sib_children: Vec<usize> = orig_children_clone
                .iter()
                .copied()
                .filter(|&ci| wrap[ci].parent2 == Some(sib_wi))
                .collect();

            if si == 0 {
                wrap[sib_wi].z = 1.0;
                last_z = wrap[sib_wi].z;
            } else if !sib_children.is_empty() {
                let first_sc = sib_children[0];
                let last_sc = *sib_children.last().unwrap();
                let mut mp = (wrap[first_sc].z + wrap[last_sc].z) / 2.0;
                mp += if sib_children.len() > 1 { 0.0 } else { 0.5 };
                if arena[v].after == 1 {
                    mp = (wrap[first_sc].z + wrap[last_sc].z) / 2.0 + 0.5;
                }
                // Adjust relative to parent position.
                let parent_z = if !orig_children_clone.is_empty() {
                    wrap[orig_children_clone[0]]
                        .parent
                        .map(|p| wrap[p].m)
                        .unwrap_or(0.0)
                } else {
                    0.0
                };
                wrap[sib_wi].z = (mp - parent_z).max(last_z + 1.0);
                last_z = wrap[sib_wi].z;
            } else {
                last_z += 1.0;
                wrap[sib_wi].z = last_z;
            }

            wrap[sib_wi].m = wrap[v].m;
        }

        // Apportion.
        let _new_ancestor = apportion(
            wrap,
            arena,
            v,
            prev_sibling,
            siblings_in_parent.first().copied().unwrap_or(v),
            last_level,
        );
    }
}

fn second_walk(wrap: &mut [WrapNode], arena: &mut [TreeNode], root: usize) {
    // Iterative pre-order.
    let mut stack = vec![root];
    while let Some(v) = stack.pop() {
        // Get parent m.
        let parent_m = wrap[v].parent.map(|p| wrap[p].m).unwrap_or(0.0);

        if arena[v].after == 1
            && let Some(&last_sib) = wrap[v].siblings.last()
        {
            wrap[v].z += wrap[last_sib].z;
        }

        let node_x = wrap[v].z + parent_m;
        arena[v].x = node_x;
        wrap[v].m += parent_m;

        // Position siblings (spouses).
        let sibs = wrap[v].siblings.clone();
        for &sib_wi in &sibs {
            let sib_x = if arena[v].after == 1 {
                let last_sib_z = wrap[v].siblings.last().map(|&s| wrap[s].z).unwrap_or(0.0);
                node_x - last_sib_z + wrap[sib_wi].z - 1.0
            } else {
                node_x + wrap[sib_wi].z
            };
            arena[sib_wi].x = sib_x;
        }

        // Process children.
        for &c in &wrap[v].children.clone() {
            stack.push(c);
        }
    }
}

fn fix_spouse_group_overlaps(arena: &mut Vec<TreeNode>, node: usize) {
    let siblings = arena[node].siblings.clone();
    let children = arena[node].children.clone();

    if !siblings.is_empty() && !children.is_empty() {
        // Group children by parent2.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        let mut current_p2: Option<usize> = Some(usize::MAX);
        let mut current_group: Vec<usize> = Vec::new();

        for &ci in &children {
            if arena[ci].parent2 != current_p2 {
                if !current_group.is_empty() {
                    groups.push(std::mem::take(&mut current_group));
                }
                current_p2 = arena[ci].parent2;
            }
            current_group.push(ci);
        }
        if !current_group.is_empty() {
            groups.push(current_group);
        }

        for g in 1..groups.len() {
            let prev_group = groups[g - 1].clone();
            let next_group = groups[g].clone();

            let mut prev_max_x = f64::NEG_INFINITY;
            for &ci in &prev_group {
                collect_max_x(arena, ci, &mut prev_max_x);
            }
            let mut next_min_x = f64::INFINITY;
            for &ci in &next_group {
                collect_min_x(arena, ci, &mut next_min_x);
            }

            let gap = next_min_x - prev_max_x;
            if gap < 1.0 {
                let shift = 1.0 - gap;
                for group in groups.iter().skip(g) {
                    for &ci in group {
                        shift_subtree(arena, ci, shift);
                    }
                }
                // Shift spouse siblings too.
                for si in g..siblings.len() {
                    arena[siblings[si]].x += shift;
                }
            }
        }
    }

    // Recurse iteratively.
    let mut stack: Vec<usize> = children;
    while let Some(ci) = stack.pop() {
        let sub_children = arena[ci].children.clone();
        fix_spouse_group_overlaps(arena, ci);
        stack.extend(sub_children);
    }
}

fn collect_max_x(arena: &[TreeNode], node: usize, max_x: &mut f64) {
    if arena[node].x > *max_x {
        *max_x = arena[node].x;
    }
    for &si in &arena[node].siblings.clone() {
        if arena[si].x > *max_x {
            *max_x = arena[si].x;
        }
    }
    for &ci in &arena[node].children.clone() {
        collect_max_x(arena, ci, max_x);
    }
}

fn collect_min_x(arena: &[TreeNode], node: usize, min_x: &mut f64) {
    if arena[node].x < *min_x {
        *min_x = arena[node].x;
    }
    for &si in &arena[node].siblings.clone() {
        if arena[si].x < *min_x {
            *min_x = arena[si].x;
        }
    }
    for &ci in &arena[node].children.clone() {
        collect_min_x(arena, ci, min_x);
    }
}

fn shift_subtree(arena: &mut Vec<TreeNode>, node: usize, shift: f64) {
    arena[node].x += shift;
    let sibs = arena[node].siblings.clone();
    for si in sibs {
        arena[si].x += shift;
    }
    let children = arena[node].children.clone();
    for ci in children {
        shift_subtree(arena, ci, shift);
    }
}

fn size_node(
    arena: &mut Vec<TreeNode>,
    node: usize,
    translate_x: f64,
    translate_depth: i32,
    last_level: i32,
) {
    let tn = &arena[node];
    let depth = tn.depth - translate_depth;
    // Determine card height for this depth.
    let sh = if tn.depth > 0 {
        DESC_H
    } else if translate_depth < 0 && translate_depth == last_level {
        COMPACT_H
    } else {
        CARD_H
    };

    let pixel_x = (arena[node].x - translate_x) * CARD_W;
    let pixel_y = if depth > 0 {
        (depth as f64 - 1.0) * CARD_H + sh
    } else {
        0.0
    };
    arena[node].x = pixel_x;
    arena[node].y = pixel_y;

    // Size siblings (spouses).
    let sibs = arena[node].siblings.clone();
    for si in sibs {
        size_node(arena, si, translate_x, translate_depth, last_level);
    }
}

/// Collect all nodes flat from the tree arena (including siblings).
fn collect_all_nodes(arena: &[TreeNode]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(n) = stack.pop() {
        if !visited.insert(n) {
            continue;
        }
        result.push(n);
        for &si in &arena[n].siblings {
            result.push(si);
        }
        for &ci in &arena[n].children {
            stack.push(ci);
        }
    }
    result
}

/// Entry point: run the full RT layout on an arena.
fn layout_tree(arena: &mut Vec<TreeNode>, last_level: i32) -> (f64, f64) {
    if arena.is_empty() {
        return (0.0, 0.0);
    }

    let mut wrap = wrap_tree(arena);
    first_walk(&mut wrap, arena, 0, last_level);

    // Adjust root's parent m so root starts at 0.
    if let Some(p) = wrap[0].parent {
        wrap[p].m = -wrap[0].z;
    }

    second_walk(&mut wrap, arena, 0);
    fix_spouse_group_overlaps(arena, 0);

    // Compute bounding box.
    let all = collect_all_nodes(arena);
    let min_x = all
        .iter()
        .map(|&i| arena[i].x)
        .fold(f64::INFINITY, f64::min);
    let max_x = all
        .iter()
        .map(|&i| arena[i].x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_depth = all.iter().map(|&i| arena[i].depth).min().unwrap_or(0);
    let max_depth = all.iter().map(|&i| arena[i].depth).max().unwrap_or(0);

    let translate_x = min_x;
    let translate_depth = min_depth;

    // Size all nodes (convert tree units → pixels).
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(n) = stack.pop() {
        if !visited.insert(n) {
            continue;
        }
        size_node(arena, n, translate_x, translate_depth, last_level);
        let sibs = arena[n].siblings.clone();
        for si in sibs {
            size_node(arena, si, translate_x, translate_depth, last_level);
        }
        let children = arena[n].children.clone();
        for ci in children {
            stack.push(ci);
        }
    }

    let tree_h = (max_depth - min_depth) as f64 * CARD_H;
    let tree_w = (max_x - min_x) * CARD_W;
    (tree_w.max(CARD_W), tree_h.max(CARD_H))
}

// ── Bézier path generators ────────────────────────────────────────────────

/// Horizontal line between spouses (from right edge of node to left edge of spouse).
fn diagonal_spouse_link(n1_x: f64, n1_y: f64, n2_x: f64, _n2_y: f64, y_offset: f64) -> String {
    let x1 = n1_x + CARD_W - 15.0;
    let x2 = n2_x + 5.0;
    let y = n1_y + y_offset;
    format!("M{x1},{y} L{x2},{y}")
}

/// S-curve from parent to single child (no spouse).
fn diagonal_simple_child(
    n1_x: f64,
    n1_y: f64,
    n2_x: f64,
    n2_y: f64,
    is_first_or_last: bool,
) -> String {
    let sx = n1_x + CARD_W / 2.0;
    let sy = n1_y + CARD_H - 23.0;
    let ex = n2_x + CARD_W / 2.0;
    let ey = n2_y + 5.0;
    let m = (sy + ey) / 2.0;

    if is_first_or_last && (sx - ex).abs() > 0.5 {
        let ctrl_offset = if sx > ex { ex + 8.0 } else { ex - 8.0 };
        format!(
            "M{sx},{sy} L{sx},{m} L{ctrl_offset},{m} S{ex},{m} {ex},{} L{ex},{ey}",
            m + 8.0
        )
    } else {
        format!("M{sx},{sy} L{sx},{m} {ex},{m} {ex},{ey}")
    }
}

/// Double S-curve from child up to ancestor (ascending tree).
#[allow(clippy::too_many_arguments)]
fn diagonal_parent(
    n1_x: f64,
    n1_y: f64,
    n1_before_sib: bool,
    n1_after_sib: bool,
    n2_x: f64,
    n2_y: f64,
    n1_depth: i32,
    n2_depth: i32,
    last_level: i32,
) -> String {
    let sw = if n2_depth == last_level {
        COMPACT_W
    } else {
        CARD_W
    };
    let sh = if n2_depth == last_level {
        COMPACT_H
    } else if n2_depth > 0 {
        DESC_H
    } else {
        CARD_H
    };

    let sx = n1_x + CARD_W / 2.0;
    let sy = n1_y + 4.0;
    let ex = n2_x + sw / 2.0;
    let ey = n2_y + sh - 23.0;
    let m = (sy + ey) / 2.0;

    // Simple path when root has siblings that would cause crossings.
    if n1_depth == 0 && ((n1_before_sib && sx > ex) || (n1_after_sib && sx < ex)) {
        let c1x = if sx > ex { sx - 8.0 } else { sx + 8.0 };
        return format!(
            "M{sx},{sy} L{sx},{} S{sx},{m} {c1x},{m} L{ex},{ey}",
            sy - 5.0
        );
    }

    let c1x = if sx > ex { sx - 8.0 } else { sx + 8.0 };
    let c2x = if sx > ex { ex + 8.0 } else { ex - 8.0 };
    format!(
        "M{sx},{sy} L{sx},{} S{sx},{m} {c1x},{m} L{c2x},{m} S{ex},{m} {ex},{} L{ex},{ey}",
        sy - 5.0,
        ey + 5.0
    )
}

/// Curved path from spouse to child.
fn diagonal_child(
    spouse_x: f64,
    spouse_y: f64,
    child_x: f64,
    child_y: f64,
    parent_after: i32,
    y_offset: f64,
    is_first_or_last: bool,
) -> String {
    let sx = if parent_after == 1 {
        spouse_x + CARD_W
    } else {
        spouse_x
    };
    let sy = spouse_y + y_offset;
    let ex = child_x + CARD_W / 2.0;
    let ey = child_y + 4.0;
    let m = child_y + (CARD_H - 23.0) / 2.0 - 50.0;

    if is_first_or_last && (sx - ex).abs() > 0.5 {
        let ctrl_x = if sx > ex { ex + 8.0 } else { ex - 8.0 };
        format!(
            "M{sx},{sy} L{sx},{m} {ctrl_x},{m} S{ex},{m} {ex},{} L{ex},{ey}",
            m + 8.0
        )
    } else {
        format!("M{sx},{sy} L{sx},{m} {ex},{m} {ex},{ey}")
    }
}

// ── Link/path collection ──────────────────────────────────────────────────

/// An SVG path for a connector between nodes.
#[derive(Clone, Debug)]
struct ConnectorPath {
    d: String,
}

fn collect_links(arena: &[TreeNode], last_level: i32) -> Vec<ConnectorPath> {
    let mut links = Vec::new();
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();

    while let Some(ni) = stack.pop() {
        if !visited.insert(ni) {
            continue;
        }
        let node = &arena[ni];

        if !node.siblings.is_empty() {
            // Spouse links + child links.
            let center = {
                let base =
                    (CARD_H - 23.0).min((4.0 * node.siblings.len() as f64 + CARD_H - 23.0) / 2.0);
                base.max(6.0)
            };

            for (si, &sib_ni) in node.siblings.iter().enumerate() {
                let y = if node.after != 1 {
                    (center - 4.0 * si as f64).max(6.0)
                } else {
                    (center - 4.0 * (node.siblings.len() - si) as f64).max(6.0)
                };

                // Spouse connector.
                links.push(ConnectorPath {
                    d: diagonal_spouse_link(node.x, node.y, arena[sib_ni].x, arena[sib_ni].y, y),
                });

                // Children of this spouse.
                let sib_node = &arena[sib_ni];
                let children_of_sib: Vec<usize> = node
                    .children
                    .iter()
                    .copied()
                    .filter(|&ci| arena[ci].parent2 == Some(sib_ni))
                    .collect();

                for (ci, &child_ni) in children_of_sib.iter().enumerate() {
                    let is_edge = ci == 0 || ci == children_of_sib.len() - 1;
                    links.push(ConnectorPath {
                        d: diagonal_child(
                            sib_node.x,
                            sib_node.y,
                            arena[child_ni].x,
                            arena[child_ni].y,
                            node.after,
                            y,
                            is_edge,
                        ),
                    });
                    // Push child onto stack.
                    stack.push(child_ni);
                }
            }
        } else {
            // Simple parent→child links.
            for (ci, &child_ni) in node.children.iter().enumerate() {
                if arena[child_ni].depth < 0 {
                    // Ascending: child → ancestor.
                    links.push(ConnectorPath {
                        d: diagonal_parent(
                            node.x,
                            node.y,
                            node.before_sibling,
                            node.after_sibling,
                            arena[child_ni].x,
                            arena[child_ni].y,
                            node.depth,
                            arena[child_ni].depth,
                            last_level,
                        ),
                    });
                } else {
                    let is_edge = ci == 0 || ci == node.children.len() - 1;
                    links.push(ConnectorPath {
                        d: diagonal_simple_child(
                            node.x,
                            node.y,
                            arena[child_ni].x,
                            arena[child_ni].y,
                            is_edge,
                        ),
                    });
                }
                stack.push(child_ni);
            }
        }
    }

    links
}

// ── Flat layout result ────────────────────────────────────────────────────

/// A positioned node on the canvas (derived from TreeNode after layout).
#[derive(Clone, Debug)]
struct LayoutNode {
    id: Option<Uuid>,
    x: f64,
    y: f64,
    depth: i32,
    sex: Sex,
    label_surname: String,
    label_given: String,
    birth_year: Option<i32>,
    death_year: Option<i32>,
    photo_url: Option<String>,
    sosa_badge: SosaBadge,
    is_compact: bool,
    /// For empty ancestor slots: which child they belong to.
    child_of: Option<Uuid>,
    is_father: bool,
    #[allow(dead_code)]
    is_sibling: bool,
}

/// Compute the RT layout and return flat nodes + connector paths.
fn compute_layout(
    root_id: Uuid,
    data: &PedigreeData,
    ancestor_levels: usize,
    descendant_levels: usize,
) -> (Vec<LayoutNode>, Vec<ConnectorPath>, f64, f64) {
    let sosa_root_id = data.sosa_root_id;
    let sosa_ancestors = &data.sosa_ancestors;

    // ── Build ascending tree ──
    let last_asc_level = -(ancestor_levels as i32);
    let mut asc_arena =
        build_ascending_tree(root_id, data, ancestor_levels, sosa_root_id, sosa_ancestors);
    let (_asc_w, _asc_h) = layout_tree(&mut asc_arena, last_asc_level);
    let asc_links = collect_links(&asc_arena, last_asc_level);

    // ── Build descending tree ──
    let mut desc_arena = build_descending_tree(
        root_id,
        data,
        descendant_levels,
        sosa_root_id,
        sosa_ancestors,
    );
    let (_desc_w, _desc_h) = layout_tree(&mut desc_arena, 0);
    let desc_links = collect_links(&desc_arena, 0);

    // ── Merge the two arenas into a flat node list ──
    // Ascending arena: root is at y=0. We need to merge ascending nodes above the root.
    // Descending arena: root is at y=0. Nodes go downward.
    //
    // Strategy:
    //  - Use descending arena as the "base" (root at origin).
    //  - Ascending nodes (non-root) get negated y so they appear above.
    //  - Root from ascending arena is at the same logical position as desc root.

    // Find root node positions in each arena.
    let asc_root_x = asc_arena[0].x;
    let asc_root_y = asc_arena[0].y;
    let desc_root_x = desc_arena[0].x;

    let asc_all = collect_all_nodes(&asc_arena);
    let desc_all = collect_all_nodes(&desc_arena);

    // Y offset so that the ascending root aligns with the desc root.
    let asc_height = asc_root_y.abs();
    let y_offset = asc_height;

    let x_shift_desc = asc_root_x - desc_root_x; // shift desc nodes to align roots in X

    // Collect flat nodes.
    let mut layout_nodes: Vec<LayoutNode> = Vec::new();

    // Ascending nodes (skip root — it comes from desc arena).
    let deepest_anc_depth = asc_all
        .iter()
        .map(|&i| asc_arena[i].depth)
        .min()
        .unwrap_or(0);

    for &ni in &asc_all {
        let tn = &asc_arena[ni];
        let is_compact = tn.depth == deepest_anc_depth && deepest_anc_depth < 0;
        let px = tn.x;
        // Ancestors have positive y in the asc arena (root=0 at bottom, ancestors above).
        // We need to flip: ancestors should appear at smaller y than root.
        let py_final = y_offset - (asc_root_y - tn.y) - CARD_H;
        let py_display = if tn.depth == 0 {
            y_offset
        } else {
            py_final.max(0.0)
        };

        // Skip duplicate root (depth==0 comes from desc arena).
        if tn.depth == 0 && ni != 0 {
            continue;
        }
        if tn.depth == 0 {
            continue;
        } // root handled by desc

        layout_nodes.push(LayoutNode {
            id: tn.id,
            x: px,
            y: py_display,
            depth: tn.depth,
            sex: tn.sex,
            label_surname: tn.label_surname.clone(),
            label_given: tn.label_given.clone(),
            birth_year: tn.birth_year,
            death_year: tn.death_year,
            photo_url: tn.photo_url.clone(),
            sosa_badge: tn.sosa_badge.clone(),
            is_compact,
            child_of: tn.child_of,
            is_father: tn.is_father,
            is_sibling: tn.is_sibling,
        });
    }

    // Descending nodes (including root at depth 0).
    for &ni in &desc_all {
        let tn = &desc_arena[ni];
        let px = tn.x + x_shift_desc;
        let py = tn.y + y_offset;
        let is_compact = false;

        layout_nodes.push(LayoutNode {
            id: tn.id,
            x: px,
            y: py,
            depth: tn.depth,
            sex: tn.sex,
            label_surname: tn.label_surname.clone(),
            label_given: tn.label_given.clone(),
            birth_year: tn.birth_year,
            death_year: tn.death_year,
            photo_url: tn.photo_url.clone(),
            sosa_badge: tn.sosa_badge.clone(),
            is_compact,
            child_of: tn.child_of,
            is_father: tn.is_father,
            is_sibling: tn.is_sibling,
        });
    }

    // Collect all links (ascending + descending).
    let mut all_links: Vec<ConnectorPath> = Vec::with_capacity(asc_links.len() + desc_links.len());
    all_links.extend(asc_links);
    all_links.extend(desc_links);

    // Compute bounding box from layout_nodes.
    let min_x = layout_nodes
        .iter()
        .map(|n| n.x)
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let max_x = layout_nodes
        .iter()
        .map(|n| n.x + CARD_W)
        .fold(0.0f64, f64::max);
    let max_y = layout_nodes
        .iter()
        .map(|n| n.y + CARD_H)
        .fold(0.0f64, f64::max);
    let total_w = (max_x - min_x).max(CARD_W) + CARD_W; // some padding
    let total_h = max_y.max(CARD_H) + CARD_H;

    (layout_nodes, all_links, total_w, total_h)
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

    // ── Compute SOSA ancestor set (persons who are ancestors of the SOSA root) ──
    // Use server-provided SOSA ancestor set (from closure table) when available,
    // falling back to local graph traversal (which only works within the pedigree window).
    let sosa_ancestors: HashSet<Uuid> = props
        .sosa_ancestor_ids
        .clone()
        .or_else(|| {
            props
                .sosa_root_person_id
                .map(|sosa_id| props.data.ancestor_set(sosa_id))
        })
        .unwrap_or_default();

    // ── Augment PedigreeData with SOSA info for the layout engine ──
    let mut data_with_sosa = props.data.clone();
    data_with_sosa.sosa_ancestors = sosa_ancestors.clone();
    data_with_sosa.sosa_root_id = props.sosa_root_person_id;

    // ── Compute layout ──
    let (layout_nodes, connector_paths, total_w, total_h) = compute_layout(
        props.root_person_id,
        &data_with_sosa,
        ancestor_levels(),
        descendant_levels(),
    );

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
                            for (si, path) in connector_paths.iter().enumerate() {
                                path {
                                    key: "seg-{si}",
                                    d: "{path.d}",
                                    class: "pedigree-connector-path",
                                    fill: "none",
                                }
                            }
                        }

                        // Person cards (absolute positioned)
                        for (ni, node) in layout_nodes.iter().enumerate() {
                            {
                                let node = node.clone();
                                let card_w = if node.is_compact { COMPACT_W } else { CARD_W };
                                let card_h = if node.is_compact { COMPACT_H } else if node.depth > 0 { DESC_H } else { CARD_H };
                                let style = format!(
                                    "position: absolute; left: {:.1}px; top: {:.1}px; width: {:.1}px; height: {:.1}px;",
                                    node.x, node.y, card_w, card_h
                                );

                                match node.id {
                                    Some(pid) => {
                                        let given_s = node.label_given.clone();
                                        let surname_s = node.label_surname.clone();
                                        let has_name = !given_s.is_empty() || !surname_s.is_empty();
                                        let birth_s = node.birth_year.map(|y| y.to_string()).unwrap_or_default();
                                        let death_s = node.death_year.map(|y| y.to_string()).unwrap_or_default();
                                        let initials = make_initials(&given_s, &surname_s);
                                        let is_focus = pid == props.root_person_id;
                                        let is_sosa_ancestor = matches!(node.sosa_badge, SosaBadge::Direct);
                                        let is_sosa_root = matches!(node.sosa_badge, SosaBadge::Root);
                                        let role_class = if node.depth == 0 {
                                            "current"
                                        } else if node.depth < 0 {
                                            "ancestor"
                                        } else {
                                            "descendant"
                                        };
                                        let sex_part = match node.sex {
                                            Sex::Male => "male",
                                            Sex::Female => "female",
                                            Sex::Unknown => "",
                                        };
                                        let is_sel = selected_person_id() == pid;
                                        let sel_part = if is_sel || is_focus { "selected" } else { "" };
                                        let compact_part = if node.is_compact { "pedigree-node-compact" } else { "pedigree-node" };
                                        let node_class = format!(
                                            "{compact_part} {sex_part} {role_class} {sel_part}"
                                        );
                                        let photo_url = node.photo_url.clone();
                                        let gender_line_class = match node.sex {
                                            Sex::Male => "pc-gender-line pc-gender-line-male",
                                            Sex::Female => "pc-gender-line pc-gender-line-female",
                                            Sex::Unknown => "pc-gender-line",
                                        };

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
                                                    // Gender line (2px vertical, left of photo)
                                                    div { class: gender_line_class }
                                                    // Photo (50×50 square)
                                                    div { class: "pc-ph",
                                                        if let Some(ref url) = photo_url {
                                                            img {
                                                                class: "pc-avatar",
                                                                src: "{url}",
                                                                alt: "{initials}",
                                                            }
                                                        } else {
                                                            span { class: "pc-initials", "{initials}" }
                                                        }
                                                    }
                                                    // SOSA badge (positioned absolute, sibling of photo)
                                                    if is_sosa_root {
                                                        span { class: "sosa-badge sosa-badge-root", title: "SOSA 1", "1" }
                                                    } else if is_sosa_ancestor {
                                                        span { class: "sosa-badge sosa-badge-direct", title: "SOSA" }
                                                    }
                                                    // Text body
                                                    div { class: "pc-body",
                                                        if !surname_s.is_empty() {
                                                            span { class: "pc-last", "{surname_s}" }
                                                        }
                                                        if !given_s.is_empty() {
                                                            span { class: "pc-first", "{given_s}" }
                                                        }
                                                        if !has_name {
                                                            span { class: "pc-first", {i18n.t("common.unknown")} }
                                                        }
                                                        if !birth_s.is_empty() || !death_s.is_empty() {
                                                            div { class: "pc-dates",
                                                                span { class: "pc-born",
                                                                    {
                                                                        match (!birth_s.is_empty(), !death_s.is_empty()) {
                                                                            (true, true)  => format!("{birth_s}-{death_s}"),
                                                                            (true, false) => format!("{birth_s}-"),
                                                                            (false, true) => format!("-{death_s}"),
                                                                            _ => String::new(),
                                                                        }
                                                                    }
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
