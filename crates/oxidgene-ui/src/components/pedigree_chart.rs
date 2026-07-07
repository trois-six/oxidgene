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
use crate::components::tree_icon_sidebar::{TreeIconSidebar, TreeSidebarView};

use oxidgene_cache::types::CachedPedigree;
use oxidgene_core::types::{
    Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName, Place,
};
use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType, Privacy, Sex, SpouseRole};

use crate::i18n::use_i18n;

use crate::utils::truncate_text_to_fit;

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

// ── Card inner SVG geometry (rect / photo / text positions) ──────────────

const CARD_BORDER_RADIUS: f64 = 5.0;
const CARD_PADDING: f64 = 5.0;
const CARD_INNER_W: f64 = 175.0;
const CARD_INNER_H: f64 = 67.0;
const COMPACT_INNER_W: f64 = 82.0;
const COMPACT_INNER_H: f64 = 115.0;
const PHOTO_W: f64 = 50.0;
const PHOTO_H: f64 = 50.0;
const PHOTO_Y: f64 = 10.0;
const PHOTO_X_FULL: f64 = 10.0;
const PHOTO_X_COMPACT: f64 = 20.0;

const TEXT_X_FULL: f64 = 70.0;
const TEXT_X_COMPACT: f64 = 10.0;
const TEXT_Y_FULL: f64 = 21.0;
const TEXT_Y_COMPACT: f64 = 81.0;
const TEXT_MAX_WIDTH_FULL: f32 = 105.0;
const SURNAME_FONT_SIZE_PX: f32 = 11.0;
const GIVEN_FONT_SIZE_PX: f32 = 10.0;
const SOSA_CX_FULL: f64 = 57.5;
const SOSA_CX_COMPACT: f64 = 67.5;
const SOSA_CY: f64 = 57.5;
const SOSA_R: f64 = 7.5;
const EDIT_FAB_R: f64 = 14.0;
const EDIT_FAB_GAP: f64 = 16.0;
const COMPACT_TEXT_TRUNCATE: usize = 7;

// ── Connector / Bézier path parameters ───────────────────────────────────

/// Card-bottom Y offset where downward connectors enter (sh − 23).
const CARD_BOTTOM_OFFSET: f64 = 23.0;
/// Card-top Y offset where upward connectors exit (n_y + 4).
const CARD_TOP_OFFSET: f64 = 4.0;
/// Small vertical indent for entry/exit segments (sy − 5, ey + 5).
const CARD_TOP_INDENT: f64 = 5.0;
/// Horizontal control-point offset for S-curve segments.
const BEZIER_CTRL_OFFSET: f64 = 8.0;
/// Spouse link inset from card edges.
const SPOUSE_LINK_INSET: f64 = 15.0;

// ── Layout spacing ───────────────────────────────────────────────────────

/// Outer margin around the rendered tree before computing the SVG viewBox.
const LAYOUT_MARGIN: f64 = 50.0;
/// Horizontal spacing between root biological siblings.
const SIBLING_SPACING: f64 = 200.0;
/// Per-sibling vertical step in spouse/child connector rows.
const SIBLING_VERTICAL_STEP: f64 = 4.0;
/// Minimum sibling vertical offset.
const SIBLING_MIN_OFFSET: f64 = 6.0;

// ── Viewport / zoom ──────────────────────────────────────────────────────

const VIEWPORT_DEFAULT_W: f64 = 800.0;
const VIEWPORT_DEFAULT_H: f64 = 600.0;
const ZOOM_FACTOR: f64 = 1.2;
const ZOOM_MIN: f64 = 0.3;
const ZOOM_MAX: f64 = 2.0;
// ── Year extraction ──────────────────────────────────────────────────────

const YEAR_MIN: u32 = 1000;
const YEAR_MAX: u32 = 2099;

// ── Default portraits (embedded as data URIs) ────────────────────────────

const PORTRAIT_MALE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_male.b64"
));
const PORTRAIT_FEMALE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_female.b64"
));
const PORTRAIT_UNKNOWN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/portrait_unknown.b64"
));

pub(crate) fn default_portrait(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => PORTRAIT_MALE,
        Sex::Female => PORTRAIT_FEMALE,
        Sex::Unknown => PORTRAIT_UNKNOWN,
    }
}

// ── Helper functions ─────────────────────────────────────────────────────

/// Extract a 4-digit year from a GEDCOM date string (e.g. "ABT 1842", "1 JAN 1900").
fn fmt_year(date: &str) -> String {
    for word in date.split_whitespace() {
        if word.len() == 4
            && word
                .parse::<u32>()
                .is_ok_and(|y| (YEAR_MIN..=YEAR_MAX).contains(&y))
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

/// Format a "birth-death" lifespan string from optional years.
pub(crate) fn format_lifespan(birth: Option<i32>, death: Option<i32>) -> String {
    match (birth, death) {
        (Some(b), Some(d)) => format!("{b}-{d}"),
        (Some(b), None) => format!("{b}-"),
        (None, Some(d)) => format!("-{d}"),
        _ => String::new(),
    }
}

/// Extract a 4-digit year from a free-text GEDCOM-style date value
/// (e.g. "ABT 1796" -> `Some(1796)`).
pub(crate) fn extract_year(date_value: &str) -> Option<i32> {
    date_value
        .split_whitespace()
        .find(|w| w.len() == 4 && w.parse::<u32>().is_ok())
        .and_then(|w| w.parse::<i32>().ok())
}

/// CSS variable for the gender-coded card border stroke.
fn gender_stroke(sex: Sex) -> &'static str {
    match sex {
        Sex::Male => "var(--pn-male-line)",
        Sex::Female => "var(--pn-female-line)",
        _ => "#888888",
    }
}

/// CSS variable for the card background fill.
fn card_bg(is_focus: bool, is_sibling: bool) -> &'static str {
    if is_focus {
        "var(--pn-root-bg)"
    } else if is_sibling {
        "var(--pn-spouse-bg)"
    } else {
        "var(--pn-bg)"
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
        EventType::CivilUnion => ("\u{1F48D}", "ev-ic ev-ic-marry", "event.type.civil_union"),
        EventType::Separation => ("\u{2696}", "ev-ic ev-ic-other", "event.type.separation"),
        EventType::DivorceFiled => ("\u{2696}", "ev-ic ev-ic-other", "event.type.divorce_filed"),
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
        EventType::Confirmation
        | EventType::FirstCommunion
        | EventType::BarBatMitzvah
        | EventType::MilitaryService
        | EventType::Adoption => ("\u{25C6}", "ev-ic ev-ic-other", "event.type.other"),
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
                privacy: Privacy::default(),
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
                    date_qualifier: DateQualifier::default(),
                    date_value2: None,
                    calendar: Calendar::default(),
                    witnesses: vec![],
                    cause: None,
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
                    date_qualifier: DateQualifier::default(),
                    date_value2: None,
                    calendar: Calendar::default(),
                    witnesses: vec![],
                    cause: None,
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
                    date_qualifier: DateQualifier::default(),
                    date_value2: None,
                    calendar: Calendar::default(),
                    witnesses: vec![],
                    cause: None,
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
                    privacy: Privacy::default(),
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
                        date_qualifier: DateQualifier::default(),
                        date_value2: None,
                        calendar: Calendar::default(),
                        witnesses: vec![],
                        cause: None,
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
                        date_qualifier: DateQualifier::default(),
                        date_value2: None,
                        calendar: Calendar::default(),
                        witnesses: vec![],
                        cause: None,
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
            .and_then(extract_year);

        let death_year = data
            .events_by_person
            .get(&id)
            .and_then(|evts| evts.iter().find(|e| e.event_type == EventType::Death))
            .and_then(|e| e.date_value.as_deref())
            .and_then(extract_year);

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
        } else {
            // Empty father slot (always shown when father is missing).
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
        } else {
            // Empty mother slot (always shown when mother is missing).
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

            // Attach point for children: the newly created spouse node, if
            // any. An unrecorded/unknown co-parent must not hide the
            // children — many older records name only one parent. In that
            // case we still render an empty "+" placeholder for the missing
            // spouse, mirroring the always-shown empty parent slots on the
            // ascending side.
            let spouse_arena_idx = match spouse_id {
                Some(sid) if visited.contains(&sid) => continue,
                Some(sid) => {
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
                    Some(spouse_arena_idx)
                }
                None => {
                    let empty_arena_idx = arena.len();
                    arena.push(TreeNode::new_empty(depth, Some(pid), false));
                    arena[node_idx].siblings.push(empty_arena_idx);
                    None
                }
            };

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
                    child_node.parent2 = spouse_arena_idx;
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

/// Port of JS `treeSeparation(a, b)`.
///
/// In the reference, tree nodes have no `parent` property, so
/// `a.parent == b.parent` is always `undefined == undefined` = true.
/// The function therefore NEVER returns 2 — it returns 0.5 when the
/// left node is at the deepest (compact) level, and 1.0 otherwise.
fn tree_separation(
    wrap: &[WrapNode],
    arena: &[TreeNode],
    a: usize,
    _b: usize,
    last_level: i32,
) -> f64 {
    let a_depth = arena[wrap[a].orig].depth;
    if a_depth == last_level { 0.5 } else { 1.0 }
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

        let sep = tree_separation(wrap, arena, vim_next, vip_next, last_level);
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
            // Filter children belonging to first sibling (spouse). A child
            // with no recorded second parent (`parent2 == None`) belongs to
            // an empty/unknown first-sibling placeholder — without this, such
            // children match neither branch, `effective_children` comes back
            // empty, and the centering/shift logic below is skipped entirely
            // for this node, leaving its subtree adrift.
            let first_sib = node_siblings[0];
            let first_sib_orig = wrap[first_sib].orig;
            let first_sib_is_empty = arena[first_sib_orig].id.is_none();
            orig_children
                .iter()
                .copied()
                .filter(|&ci| {
                    wrap[ci].parent2.map(|p2| wrap[p2].orig) == Some(first_sib_orig)
                        || (first_sib_is_empty && wrap[ci].parent2.is_none())
                })
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
                    // Consecutive siblings share the same parent: separation is 0.5 at
                    // last_level (compact deepest ancestors), 1 everywhere else.
                    let v_orig = wrap[v].orig;
                    // JS treeSeparation only checks a.depth (the current node), not b.depth.
                    let sep = if arena[v_orig].depth == last_level {
                        0.5
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
            let v_orig = wrap[v].orig;
            // JS treeSeparation only checks a.depth (the current node), not b.depth.
            let sep = if arena[v_orig].depth == last_level {
                0.5
            } else {
                1.0
            };
            wrap[v].z = wrap[w].z + w_sib_z + sep;
        }

        // Multi-spouse positioning (simplified port).
        let mut last_z = 0.0f64;
        let node_siblings_clone = wrap[v].siblings.clone();
        let orig_children_clone = wrap[v].children.clone();

        if !node_siblings_clone.is_empty() && arena[v].after == 1 && effective_children.len() != 1 {
            wrap[v].m -= 0.5;
            // Port of JS `firstSibWithChild` correction:
            // if the FIRST sibling (index 0) is the parent2 of the children,
            // firstSibWithChild = 0 - 1 = -1 → node.m -= (-1) → m += 1.
            // Net result for the common case (first spouse has the children): m += 0.5.
            let mut first_sib_with_child = 0i32;
            if !orig_children_clone.is_empty() {
                let first_child_p2 = wrap[orig_children_clone[0]].parent2;
                for (index, &sib_wi) in node_siblings_clone.iter().enumerate() {
                    let sib_is_empty = arena[wrap[sib_wi].orig].id.is_none();
                    if first_child_p2 == Some(sib_wi)
                        || (first_child_p2.is_none() && sib_is_empty)
                    {
                        first_sib_with_child = index as i32 - 1;
                    }
                }
            }
            wrap[v].m -= first_sib_with_child as f64;
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
    let children = arena[node].children.clone();

    // Recurse into children FIRST (post-order). A shift applied while fixing
    // a deeper level can widen this node's own subtree (e.g. pushing a leaf
    // sibling's married-in spouse rightward), so the gap check below must see
    // the final, already-corrected width of each child — not the
    // RT-computed width from before any deeper fix-up ran. Checking gaps
    // top-down (parent before children) let an inner shift silently eat into
    // a buffer an outer check had already validated, re-introducing the
    // exact overlap this pass exists to prevent.
    for &ci in &children {
        fix_spouse_group_overlaps(arena, ci);
    }

    let siblings = arena[node].siblings.clone();

    // This pass only ever matters for nodes that themselves have a recorded
    // spouse (`siblings` non-empty) — that's how the descending tree marks a
    // couple whose children may come from more than one spouse. Ascending
    // trees never populate `siblings`, so this is a no-op there and the
    // RT-computed father/mother spacing (including the compact 0.5-unit
    // separation at the deepest level) is left untouched.
    if !siblings.is_empty() && !children.is_empty() {
        // Sweep every adjacent pair of children — full siblings as well as
        // half-sibling group boundaries. This also covers the case where a
        // child is itself a leaf with a married-in spouse card, which the
        // Reingold-Tilford contour walk does not always widen for.
        for i in 1..children.len() {
            let prev = children[i - 1];
            let curr = children[i];

            let mut prev_max_x = f64::NEG_INFINITY;
            collect_max_x(arena, prev, &mut prev_max_x);
            let mut curr_min_x = f64::INFINITY;
            collect_min_x(arena, curr, &mut curr_min_x);

            let gap = curr_min_x - prev_max_x;
            if gap < 1.0 {
                let shift = 1.0 - gap;
                for &ci in &children[i..] {
                    shift_subtree(arena, ci, shift);
                }
                // If `curr` starts a new parent2 (half-sibling) group, shift
                // the matching spouse sibling of `node` along with it.
                if arena[curr].parent2 != arena[prev].parent2
                    && let Some(pos) = siblings
                        .iter()
                        .position(|&s| Some(s) == arena[curr].parent2)
                {
                    for &si in &siblings[pos..] {
                        arena[si].x += shift;
                    }
                }
            }
        }
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
    // Note: size_node handles siblings internally (matching JS sizeNode which calls
    // node.siblings.forEach(sizeNode)). preOrderTraversal only visits children.
    let mut stack = vec![0usize];
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(n) = stack.pop() {
        if !visited.insert(n) {
            continue;
        }
        size_node(arena, n, translate_x, translate_depth, last_level);
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

/// Horizontal control-point X for an S-curve, stepping `BEZIER_CTRL_OFFSET`
/// inward toward the destination from the source.
fn ctrl_x_toward(src: f64, dst: f64) -> f64 {
    if src > dst {
        dst + BEZIER_CTRL_OFFSET
    } else {
        dst - BEZIER_CTRL_OFFSET
    }
}

/// Horizontal control-point X stepping `BEZIER_CTRL_OFFSET` outward from the source.
fn ctrl_x_outward(src: f64, dst: f64) -> f64 {
    if src > dst {
        src - BEZIER_CTRL_OFFSET
    } else {
        src + BEZIER_CTRL_OFFSET
    }
}

/// Horizontal line between spouses (from right edge of node to left edge of spouse).
fn diagonal_spouse_link(n1_x: f64, n1_y: f64, n2_x: f64, _n2_y: f64, y_offset: f64) -> String {
    let x1 = n1_x + CARD_W - SPOUSE_LINK_INSET;
    let x2 = n2_x + CARD_PADDING;
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
    let sy = n1_y + CARD_H - CARD_BOTTOM_OFFSET;
    let ex = n2_x + CARD_W / 2.0;
    let ey = n2_y + CARD_PADDING;
    let m = (sy + ey) / 2.0;

    if is_first_or_last && (sx - ex).abs() > 0.5 {
        let ctrl_offset = ctrl_x_toward(sx, ex);
        format!(
            "M{sx},{sy} L{sx},{m} L{ctrl_offset},{m} S{ex},{m} {ex},{} L{ex},{ey}",
            m + BEZIER_CTRL_OFFSET
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
    let sy = n1_y + CARD_TOP_OFFSET;
    let ex = n2_x + sw / 2.0;
    let ey = n2_y + sh - CARD_BOTTOM_OFFSET;
    let m = (sy + ey) / 2.0;

    // Simple path when root has siblings that would cause crossings.
    // Only goes to (ex, m) — the parent x at the midpoint — matching JS p[5], not all the way to (ex, ey).
    if n1_depth == 0 && ((n1_before_sib && sx > ex) || (n1_after_sib && sx < ex)) {
        let c1x = ctrl_x_outward(sx, ex);
        return format!(
            "M{sx},{sy} L{sx},{} S{sx},{m} {c1x},{m} L{ex},{m}",
            sy - CARD_TOP_INDENT
        );
    }

    let c1x = ctrl_x_outward(sx, ex);
    let c2x = ctrl_x_toward(sx, ex);
    format!(
        "M{sx},{sy} L{sx},{} S{sx},{m} {c1x},{m} L{c2x},{m} S{ex},{m} {ex},{} L{ex},{ey}",
        sy - CARD_TOP_INDENT,
        ey + CARD_TOP_INDENT
    )
}

/// Path from a parent node up to a root biological sibling.
/// Port of `diagonalSibling` from layout.js.
#[allow(clippy::too_many_arguments)]
fn diagonal_sibling(
    n1_x: f64,
    n1_y: f64,
    n1_depth: i32,
    n2_x: f64,
    n2_y: f64,
    index: usize,
    nb_children: usize,
    simple: bool,
    last_level: i32,
) -> String {
    let sw = if n1_depth == last_level {
        COMPACT_W
    } else {
        CARD_W
    };
    let sh = if n1_depth == last_level {
        COMPACT_H
    } else {
        CARD_H
    };

    let s_x = n1_x + sw / 2.0;
    let s_y = n1_y + sh - CARD_BOTTOM_OFFSET;
    let e_x = n2_x + CARD_W / 2.0;
    let e_y = n2_y + CARD_TOP_OFFSET;
    let m = (s_y + e_y) / 2.0;

    // Simple straight path for all but the last sibling; S-curve for the last one.
    if (index != nb_children.saturating_sub(1)) || (s_x - e_x).abs() < 0.001 || simple {
        format!("M{s_x},{s_y} L{s_x},{m} {e_x},{m} {e_x},{e_y}")
    } else {
        let ctrl_x = ctrl_x_toward(s_x, e_x);
        format!(
            "M{s_x},{s_y} L{s_x},{m} {ctrl_x},{m} S{e_x},{m} {e_x},{} L{e_x},{e_y}",
            m + BEZIER_CTRL_OFFSET
        )
    }
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
    let ey = child_y + CARD_TOP_OFFSET;
    let m = child_y + (CARD_H - CARD_BOTTOM_OFFSET) / 2.0 - LAYOUT_MARGIN;

    if is_first_or_last && (sx - ex).abs() > 0.5 {
        let ctrl_x = ctrl_x_toward(sx, ex);
        format!(
            "M{sx},{sy} L{sx},{m} {ctrl_x},{m} S{ex},{m} {ex},{} L{ex},{ey}",
            m + BEZIER_CTRL_OFFSET
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
                let exit = CARD_H - CARD_BOTTOM_OFFSET;
                let base =
                    exit.min((SIBLING_VERTICAL_STEP * node.siblings.len() as f64 + exit) / 2.0);
                base.max(SIBLING_MIN_OFFSET)
            };

            for (si, &sib_ni) in node.siblings.iter().enumerate() {
                let y = if node.after != 1 {
                    (center - SIBLING_VERTICAL_STEP * si as f64).max(SIBLING_MIN_OFFSET)
                } else {
                    (center - SIBLING_VERTICAL_STEP * (node.siblings.len() - si) as f64)
                        .max(SIBLING_MIN_OFFSET)
                };

                // Spouse connector.
                links.push(ConnectorPath {
                    d: diagonal_spouse_link(node.x, node.y, arena[sib_ni].x, arena[sib_ni].y, y),
                });

                // Children of this spouse. A child with no recorded second
                // parent (`parent2 == None`) is attributed to the empty
                // spouse placeholder (`id.is_none()`), if one is present.
                let sib_node = &arena[sib_ni];
                let children_of_sib: Vec<usize> = node
                    .children
                    .iter()
                    .copied()
                    .filter(|&ci| {
                        arena[ci].parent2 == Some(sib_ni)
                            || (arena[ci].parent2.is_none() && sib_node.id.is_none())
                    })
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
    is_sibling: bool,
}

/// Result of the pedigree layout computation.
///
/// The ascending and descending trees are kept in their own coordinate spaces so
/// that their SVG groups can each receive the correct `translate()` transform —
/// mirroring how `generate.js` + `renderSVG` work in the reference project.
struct PedigreeLayout {
    /// Ascending tree nodes in ascending-tree coordinate space.
    asc_nodes: Vec<LayoutNode>,
    /// Descending tree nodes in descending-tree coordinate space.
    desc_nodes: Vec<LayoutNode>,
    /// SVG connector paths for the ascending tree (in ascending coordinate space).
    asc_links: Vec<ConnectorPath>,
    /// SVG connector paths for the descending tree (in descending coordinate space).
    desc_links: Vec<ConnectorPath>,
    /// X translate applied to the outer SVG group (shifts content so x ≥ 0).
    main_tx: f64,
    /// Y translate applied to the outer SVG group (shifts content so y ≥ 0).
    main_ty: f64,
    /// X translate for the descending `<g>` (aligns desc root X with asc root X).
    desc_tx: f64,
    /// Y translate for the descending `<g>` (= asc root y, aligns vertically).
    desc_ty: f64,
    /// Total SVG viewport width.
    total_w: f64,
    /// Total SVG viewport height.
    total_h: f64,
    /// Real graph content centre x in final SVG coordinates, excluding margins.
    content_cx: f64,
    /// Real graph content centre y in final SVG coordinates, excluding margins.
    content_cy: f64,
    /// Real graph content width, excluding margins.
    content_w: f64,
    /// Real graph content height, excluding margins.
    content_h: f64,
    /// Root card centre x in final SVG coordinates (for auto-centering).
    root_cx: f64,
    /// Root card centre y in final SVG coordinates (for auto-centering).
    root_cy: f64,
}

/// Compute the RT layout for both ascending and descending trees.
///
/// Uses two independent `LayoutTreeService`-equivalent passes (one per tree) and
/// computes the SVG group transforms needed to make the root card appear at the
/// same canvas position in both trees — exactly as `generate.js` / `renderSVG`
/// do in the JS reference implementation.
fn compute_layout(
    root_id: Uuid,
    data: &PedigreeData,
    ancestor_levels: usize,
    descendant_levels: usize,
) -> PedigreeLayout {
    let sosa_root_id = data.sosa_root_id;
    let sosa_ancestors = &data.sosa_ancestors;
    let last_asc_level = -(ancestor_levels as i32);

    // ── Ascending tree ──
    let mut asc_arena =
        build_ascending_tree(root_id, data, ancestor_levels, sosa_root_id, sosa_ancestors);
    layout_tree(&mut asc_arena, last_asc_level);
    let mut asc_links = collect_links(&asc_arena, last_asc_level);

    // ── Descending tree ──
    let mut desc_arena = build_descending_tree(
        root_id,
        data,
        descendant_levels,
        sosa_root_id,
        sosa_ancestors,
    );
    layout_tree(&mut desc_arena, 0);
    let desc_links = collect_links(&desc_arena, 0);

    // Root is always at arena index 0 in both trees.
    let asc_root_x = asc_arena[0].x;
    let asc_root_y = asc_arena[0].y;
    let desc_root_x = desc_arena[0].x;
    let desc_root_y = desc_arena[0].y;

    // Descending-group SVG transform that aligns desc root with asc root:
    //   global_x(desc_node) = desc_node.x + desc_tx + main_tx
    //   global_x(asc_node)  = asc_node.x          + main_tx
    // At root: asc_root_x = desc_root_x + desc_tx  →  desc_tx = asc_root_x - desc_root_x
    let desc_tx = asc_root_x - desc_root_x;
    let desc_ty = asc_root_y - desc_root_y; // desc_root_y is 0 after layout_tree

    // ── Root biological siblings (placed outside RT layout, same row as root) ──
    // Port of the siblingsBefore/siblingsAfter logic from generate.js.
    let mut extra_asc_nodes: Vec<LayoutNode> = Vec::new();
    {
        let all_siblings = get_siblings(root_id, data);
        if all_siblings.len() > 1 {
            let root_sib_idx = all_siblings.iter().position(|&s| s == root_id).unwrap_or(0);
            let sibs_before = &all_siblings[..root_sib_idx];
            let sibs_after = &all_siblings[root_sib_idx + 1..];

            // Extend minX/maxX based on desc root's spouses (mirrors generate.js).
            let mut sib_min_x = asc_root_x;
            let mut sib_max_x = asc_root_x;
            if desc_arena[0].after == 0 && !desc_arena[0].siblings.is_empty() {
                // Male root: wife is to the right → extend maxX.
                let last_si = *desc_arena[0].siblings.last().unwrap();
                sib_max_x += desc_arena[last_si].x - desc_root_x;
            } else if !desc_arena[0].siblings.is_empty() {
                // Female root: husband is to the left → extend minX.
                let first_si = desc_arena[0].siblings[0];
                sib_min_x -= desc_root_x - desc_arena[first_si].x;
            }

            // Father: asc_arena[0].children[0], Mother: children[1] (if present).
            let father_data = asc_arena[0]
                .children
                .first()
                .map(|&ci| (asc_arena[ci].x, asc_arena[ci].y, asc_arena[ci].depth));
            let parent_idx = if asc_arena[0].children.len() > 1 {
                1
            } else {
                0
            };
            let mother_data = asc_arena[0]
                .children
                .get(parent_idx)
                .map(|&ci| (asc_arena[ci].x, asc_arena[ci].y, asc_arena[ci].depth));

            let len_before = sibs_before.len();
            let len_after = sibs_after.len();

            for (i, &sib_id) in sibs_before.iter().enumerate() {
                let sib_x = sib_min_x - SIBLING_SPACING * (len_before - i) as f64;
                let sib_y = asc_root_y;
                let pn = PersonNode::from_data(sib_id, data, sosa_root_id, sosa_ancestors);
                extra_asc_nodes.push(LayoutNode {
                    id: Some(sib_id),
                    x: sib_x,
                    y: sib_y,
                    sex: pn.sex,
                    label_surname: pn.surname,
                    label_given: pn.given,
                    birth_year: pn.birth_year,
                    death_year: pn.death_year,
                    photo_url: pn.photo_url,
                    sosa_badge: pn.sosa_badge,
                    is_compact: false,
                    child_of: None,
                    is_father: false,
                    is_sibling: false,
                });
                // Link from father node (index reversed so furthest sibling is "last").
                if let Some((fx, fy, fd)) = father_data {
                    let rev_idx = len_before - i - 1;
                    let simple = sib_x >= fx;
                    asc_links.push(ConnectorPath {
                        d: diagonal_sibling(
                            fx,
                            fy,
                            fd,
                            sib_x,
                            sib_y,
                            rev_idx,
                            len_before,
                            simple,
                            last_asc_level,
                        ),
                    });
                }
            }

            for (i, &sib_id) in sibs_after.iter().enumerate() {
                let sib_x = sib_max_x + SIBLING_SPACING * (i + 1) as f64;
                let sib_y = asc_root_y;
                let pn = PersonNode::from_data(sib_id, data, sosa_root_id, sosa_ancestors);
                extra_asc_nodes.push(LayoutNode {
                    id: Some(sib_id),
                    x: sib_x,
                    y: sib_y,
                    sex: pn.sex,
                    label_surname: pn.surname,
                    label_given: pn.given,
                    birth_year: pn.birth_year,
                    death_year: pn.death_year,
                    photo_url: pn.photo_url,
                    sosa_badge: pn.sosa_badge,
                    is_compact: false,
                    child_of: None,
                    is_father: false,
                    is_sibling: false,
                });
                // Link from mother (or father if no mother).
                if let Some((px, py, pd)) = mother_data {
                    let simple = sib_x <= px;
                    asc_links.push(ConnectorPath {
                        d: diagonal_sibling(
                            px,
                            py,
                            pd,
                            sib_x,
                            sib_y,
                            i,
                            len_after,
                            simple,
                            last_asc_level,
                        ),
                    });
                }
            }
        }
    }

    // ── Global bounding box (descending nodes shifted by desc_tx/ty) ──
    let asc_all = collect_all_nodes(&asc_arena);
    let desc_all = collect_all_nodes(&desc_arena);

    let mut gmin_x = f64::INFINITY;
    let mut gmax_x = f64::NEG_INFINITY;
    let mut gmin_y = f64::INFINITY;
    let mut gmax_y = f64::NEG_INFINITY;

    for &ni in &asc_all {
        let tn = &asc_arena[ni];
        let cw = if tn.depth == last_asc_level {
            COMPACT_W
        } else {
            CARD_W
        };
        let ch = if tn.depth == last_asc_level {
            COMPACT_H
        } else {
            CARD_H
        };
        gmin_x = gmin_x.min(tn.x);
        gmax_x = gmax_x.max(tn.x + cw);
        gmin_y = gmin_y.min(tn.y);
        gmax_y = gmax_y.max(tn.y + ch);
    }
    for &ni in &desc_all {
        let tn = &desc_arena[ni];
        let ch = if tn.depth > 0 { DESC_H } else { CARD_H };
        let gx = tn.x + desc_tx;
        let gy = tn.y + desc_ty;
        gmin_x = gmin_x.min(gx);
        gmax_x = gmax_x.max(gx + CARD_W);
        gmin_y = gmin_y.min(gy);
        gmax_y = gmax_y.max(gy + ch);
    }
    // Include root biological siblings in bounding box.
    for node in &extra_asc_nodes {
        gmin_x = gmin_x.min(node.x);
        gmax_x = gmax_x.max(node.x + CARD_W);
        gmin_y = gmin_y.min(node.y);
        gmax_y = gmax_y.max(node.y + CARD_H);
    }

    let margin = LAYOUT_MARGIN;
    // Shift so that no node has a negative coordinate inside the main group.
    let main_tx = (-gmin_x + margin).max(margin);
    let main_ty = (-gmin_y + margin).max(margin);
    let total_w = (gmax_x - gmin_x + 2.0 * margin).max(CARD_W);
    let total_h = (gmax_y - gmin_y + 2.0 * margin).max(CARD_H);
    let content_cx = (gmin_x + gmax_x) / 2.0 + main_tx;
    let content_cy = (gmin_y + gmax_y) / 2.0 + main_ty;
    let content_w = (gmax_x - gmin_x).max(CARD_W);
    let content_h = (gmax_y - gmin_y).max(CARD_H);

    // Root card centre in final SVG coordinates (used for auto-centering).
    let root_cx = asc_root_x + main_tx + CARD_W / 2.0;
    let root_cy = asc_root_y + main_ty + CARD_H / 2.0;

    // ── Build LayoutNode lists ──
    let make_node = |ni: usize, arena: &Vec<TreeNode>, is_compact: bool| LayoutNode {
        id: arena[ni].id,
        x: arena[ni].x,
        y: arena[ni].y,
        sex: arena[ni].sex,
        label_surname: arena[ni].label_surname.clone(),
        label_given: arena[ni].label_given.clone(),
        birth_year: arena[ni].birth_year,
        death_year: arena[ni].death_year,
        photo_url: arena[ni].photo_url.clone(),
        sosa_badge: arena[ni].sosa_badge.clone(),
        is_compact,
        child_of: arena[ni].child_of,
        is_father: arena[ni].is_father,
        is_sibling: arena[ni].is_sibling,
    };

    let mut asc_nodes: Vec<LayoutNode> = asc_all
        .iter()
        .map(|&ni| make_node(ni, &asc_arena, asc_arena[ni].depth == last_asc_level))
        .collect();
    asc_nodes.extend(extra_asc_nodes);
    let desc_nodes: Vec<LayoutNode> = desc_all
        .iter()
        .map(|&ni| make_node(ni, &desc_arena, false))
        .collect();

    PedigreeLayout {
        asc_nodes,
        desc_nodes,
        asc_links,
        desc_links,
        main_tx,
        main_ty,
        desc_tx,
        desc_ty,
        total_w,
        total_h,
        content_cx,
        content_cy,
        content_w,
        content_h,
        root_cx,
        root_cy,
    }
}

// ── Component ────────────────────────────────────────────────────────────

/// Fixed zoom level for [`MiniPedigree`] — not user-adjustable. 66% of the
/// 1.4 baseline that matched the "Family" narrative section's font size.
const MINI_PEDIGREE_SCALE: f64 = 0.8;

/// Default viewport size for [`MiniPedigree`] before the actual DOM element
/// has been measured (see `needs_center` below).
const MINI_PEDIGREE_VIEWPORT_W: f64 = 400.0;
const MINI_PEDIGREE_VIEWPORT_H: f64 = 280.0;

/// Bottom padding (viewport px) kept below the root card when it's anchored
/// near the bottom of the canvas (no descendants to show underneath it).
const MINI_PEDIGREE_BOTTOM_MARGIN: f64 = 60.0;

/// Props for [`MiniPedigree`] — a small pedigree fragment, focused and
/// centered on `root_person_id`, for embedding outside the main tree canvas
/// (e.g. on the person detail page). Panning is enabled but the zoom level
/// is fixed (see [`MINI_PEDIGREE_SCALE`]) — there is no zoom control.
#[derive(Props, Clone, PartialEq)]
pub struct MiniPedigreeProps {
    pub root_person_id: Uuid,
    pub data: PedigreeData,
    pub ancestor_levels: usize,
    pub descendant_levels: usize,
    /// Called when the user clicks a person card (navigate to their page).
    /// Empty ancestor/descendant slots are not clickable.
    pub on_person_navigate: EventHandler<Uuid>,
}

/// A small, pannable (but not zoomable) pedigree fragment (e.g. "parents &
/// grandparents"), always centered on `root_person_id` at a fixed zoom
/// level. Reuses the same layout engine and card renderer as the full
/// interactive [`PedigreeChart`].
#[component]
pub fn MiniPedigree(props: MiniPedigreeProps) -> Element {
    let selected_person_id = use_signal(|| props.root_person_id);
    let noop_click = EventHandler::new(|_: (Uuid, f64, f64)| {});
    let noop_empty_slot = EventHandler::new(|_: (Uuid, bool)| {});

    // ── Pan state (no zoom signal — the scale is the fixed constant above) ──
    let mut offset_x = use_signal(|| 0.0f64);
    let mut offset_y = use_signal(|| 0.0f64);
    let mut dragging = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0f64);
    let mut drag_start_y = use_signal(|| 0.0f64);
    let mut drag_origin_x = use_signal(|| 0.0f64);
    let mut drag_origin_y = use_signal(|| 0.0f64);

    let layout = compute_layout(
        props.root_person_id,
        &props.data,
        props.ancestor_levels,
        props.descendant_levels,
    );

    // ── Center the root person in the viewport on first render and root change ──
    let mut prev_root = use_signal(|| props.root_person_id);
    let mut needs_center = use_signal(|| true);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        needs_center.set(true);
    }
    if needs_center() {
        needs_center.set(false);
        let root_cx = layout.root_cx;
        let root_cy = layout.root_cy;
        // When there are no descendants to show below the root, anchor it
        // near the bottom of the viewport instead of the vertical middle —
        // otherwise the ancestor rows above waste half the canvas.
        // (`desc_nodes` always contains at least the root card itself, even
        // at descendant_levels == 0, so check the prop directly instead.)
        let anchor_bottom = props.descendant_levels == 0;
        spawn(async move {
            // Small delay so the DOM has rendered the viewport element.
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            if let Ok(val) = document::eval(
                "var el = document.querySelector('.mini-pedigree'); return el ? [el.clientWidth, el.clientHeight] : [400, 280]"
            ).await {
                let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(MINI_PEDIGREE_VIEWPORT_W);
                let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(MINI_PEDIGREE_VIEWPORT_H);
                offset_x.set(vw / 2.0 - root_cx * MINI_PEDIGREE_SCALE);
                let target_y = if anchor_bottom {
                    vh - MINI_PEDIGREE_BOTTOM_MARGIN
                } else {
                    vh / 2.0
                };
                offset_y.set(target_y - root_cy * MINI_PEDIGREE_SCALE);
            }
        });
    }

    let transform = format!(
        "translate({}px, {}px) scale({MINI_PEDIGREE_SCALE})",
        offset_x(),
        offset_y(),
    );

    rsx! {
        div {
            class: "mini-pedigree",
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
            onpointerup: move |_| dragging.set(false),
            onpointerleave: move |_| dragging.set(false),

            div {
                class: "mini-pedigree-inner",
                style: "transform: {transform};",
                svg {
                    width: "{layout.total_w}",
                    height: "{layout.total_h}",
                    "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                    style: "display: block; overflow: visible;",
                    g { transform: "translate({layout.main_tx},{layout.main_ty})",
                        g {
                            for (si, path) in layout.asc_links.iter().enumerate() {
                                path { key: "al-{si}", d: "{path.d}", class: "pedigree-connector-path", fill: "none" }
                            }
                            for (ni, node) in layout.asc_nodes.iter().enumerate() {
                                {render_pedigree_card(
                                    node,
                                    ni,
                                    "an",
                                    props.root_person_id,
                                    selected_person_id,
                                    props.on_person_navigate,
                                    noop_click,
                                    noop_empty_slot,
                                    false,
                                )}
                            }
                        }
                        g {
                            transform: "translate({layout.desc_tx},{layout.desc_ty})",
                            for (si, path) in layout.desc_links.iter().enumerate() {
                                path { key: "dl-{si}", d: "{path.d}", class: "pedigree-connector-path", fill: "none" }
                            }
                            for (ni, node) in layout.desc_nodes.iter().enumerate() {
                                {render_pedigree_card(
                                    node,
                                    ni,
                                    "dn",
                                    props.root_person_id,
                                    selected_person_id,
                                    props.on_person_navigate,
                                    noop_click,
                                    noop_empty_slot,
                                    false,
                                )}
                            }
                        }
                    }
                }
            }
        }
    }
}

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
    /// Called when the user clicks the empty "+" placeholder for a missing
    /// spouse on the descending side (the person needing a spouse).
    pub on_add_spouse_slot: EventHandler<Uuid>,
    #[props(default)]
    pub on_add_person: EventHandler<()>,
    #[props(default)]
    pub on_profile_view: EventHandler<Uuid>,
    #[props(default)]
    pub on_settings: EventHandler<()>,
}

/// Render one card (person or empty slot) of the pedigree as an SVG `<g>`.
///
/// Used for both ascending and descending trees — pass the matching key
/// prefix (`"an"` / `"dn"`) and `allow_empty_click=true` on both sides, so
/// missing parents (ascending) and missing spouses (descending) can be
/// added inline.
#[allow(clippy::too_many_arguments)]
fn render_pedigree_card(
    node: &LayoutNode,
    ni: usize,
    key_prefix: &str,
    root_person_id: Uuid,
    mut selected_person_id: Signal<Uuid>,
    on_person_navigate: EventHandler<Uuid>,
    on_person_click: EventHandler<(Uuid, f64, f64)>,
    on_empty_slot: EventHandler<(Uuid, bool)>,
    allow_empty_click: bool,
) -> Element {
    let is_compact = node.is_compact;
    let (rw, rh) = if is_compact {
        (COMPACT_INNER_W, COMPACT_INNER_H)
    } else {
        (CARD_INNER_W, CARD_INNER_H)
    };
    let (gl_path, ph_x) = if is_compact {
        ("M19,10 L19,60", PHOTO_X_COMPACT)
    } else {
        ("M9,10 L9,60", PHOTO_X_FULL)
    };
    let (tx, ty) = if is_compact {
        (TEXT_X_COMPACT, TEXT_Y_COMPACT)
    } else {
        (TEXT_X_FULL, TEXT_Y_FULL)
    };
    let sosa_cx = if is_compact {
        SOSA_CX_COMPACT
    } else {
        SOSA_CX_FULL
    };
    let sosa_cy = SOSA_CY;
    let key = format!("{key_prefix}-{ni}");
    let nx = node.x;
    let ny = node.y;

    match node.id {
        Some(pid) => {
            let is_focus = pid == root_person_id;
            let is_selected = selected_person_id() == pid;
            let bg = card_bg(is_focus, node.is_sibling);
            let text_fill = if is_focus {
                "#ffffff"
            } else {
                "var(--pn-text)"
            };
            let stroke = gender_stroke(node.sex);
            let label_surname = node
                .label_surname
                .split(",")
                .next()
                .unwrap_or("")
                .to_string();
            let surname_up = label_surname.to_uppercase();
            let surname_disp = if is_compact {
                surname_up
                    .chars()
                    .take(COMPACT_TEXT_TRUNCATE)
                    .collect::<String>()
            } else {
                truncate_text_to_fit(&surname_up, TEXT_MAX_WIDTH_FULL, SURNAME_FONT_SIZE_PX)
            };
            let label_given = node.label_given.split(",").next().unwrap_or("").to_string();
            let given_disp = if is_compact {
                label_given
                    .chars()
                    .take(COMPACT_TEXT_TRUNCATE)
                    .collect::<String>()
            } else {
                truncate_text_to_fit(&label_given, TEXT_MAX_WIDTH_FULL, GIVEN_FONT_SIZE_PX)
            };
            let date_s = format_lifespan(node.birth_year, node.death_year);
            let has_surname = !surname_disp.is_empty();
            let has_given = !given_disp.is_empty();
            let has_date = !date_s.is_empty();
            let given_y = ty;
            let surname_y = if has_given { ty + 14.0 } else { ty };
            let date_y = if has_surname {
                surname_y + 14.0
            } else if has_given {
                given_y + 14.0
            } else {
                ty
            };
            let photo_url = node.photo_url.clone();
            let portrait_src = photo_url
                .as_deref()
                .unwrap_or_else(|| default_portrait(node.sex))
                .to_string();
            let is_sosa_root = matches!(node.sosa_badge, SosaBadge::Root);
            let is_sosa_direct = matches!(node.sosa_badge, SosaBadge::Direct);
            let fab_x = CARD_PADDING + rw / 2.0;
            let fab_y = CARD_PADDING + rh + EDIT_FAB_GAP;
            rsx! {
                g {
                    key: "{key}",
                    transform: "translate({nx},{ny})",
                    style: "cursor:pointer",
                    onclick: move |_| { selected_person_id.set(pid); on_person_navigate.call(pid); },
                    rect { x: "{CARD_PADDING}", y: "{CARD_PADDING}", rx: "{CARD_BORDER_RADIUS}", ry: "{CARD_BORDER_RADIUS}", width: "{rw}", height: "{rh}", style: "fill:{bg};stroke:#888888;stroke-width:1" }
                    if is_selected || is_focus {
                        rect { x: "4", y: "4", rx: "6", ry: "6", width: "{rw+2.0}", height: "{rh+2.0}", style: "fill:none;stroke:var(--orange);stroke-width:2;pointer-events:none" }
                    }
                    path { d: "{gl_path}", style: "stroke:{stroke};stroke-width:2;fill:none" }
                    rect { x: "{ph_x}", y: "{PHOTO_Y}", width: "{PHOTO_W}", height: "{PHOTO_H}", style: "fill:#ffffff" }
                    image { "href": "{portrait_src}", x: "{ph_x}", y: "{PHOTO_Y}", width: "{PHOTO_W}", height: "{PHOTO_H}", style: "object-fit:cover" }
                    if is_sosa_root {
                        g {
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{SOSA_R}", style: "fill:rgb(109,161,24)" }
                            text { x: "{sosa_cx}", y: "{sosa_cy+4.0}", style: "fill:#fff;font-size:10px;font-weight:700;text-anchor:middle;font-family:Arial,sans-serif", "1" }
                        }
                    } else if is_sosa_direct {
                        g {
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "{SOSA_R}", style: "fill:rgb(149,196,23)" }
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "5", style: "fill:#fff" }
                            circle { cx: "{sosa_cx}", cy: "{sosa_cy}", r: "3", style: "fill:rgb(149,196,23)" }
                        }
                    }
                    text {
                        if has_given {
                            tspan { x: "{tx}", y: "{given_y}", style: "font-size:10px;font-family:'Lato',sans-serif;fill:{text_fill}", "{given_disp}" }
                        }
                        if has_surname {
                            tspan { x: "{tx}", y: "{surname_y}", style: "font-size:11px;font-weight:700;font-family:'Lato',sans-serif;fill:{text_fill}", "{surname_disp}" }
                        }
                        if has_date {
                            tspan { x: "{tx}", y: "{date_y}", style: "font-size:10px;font-family:'Lato',sans-serif;fill:{text_fill}", "{date_s}" }
                        }
                    }
                    if is_focus {
                        g {
                            transform: "translate({fab_x},{fab_y})",
                            style: "cursor:pointer",
                            onclick: move |evt: Event<MouseData>| {
                                evt.stop_propagation();
                                let coords = evt.client_coordinates();
                                on_person_click.call((pid, coords.x, coords.y));
                            },
                            circle { r: "{EDIT_FAB_R}", style: "fill:var(--pn-root-bg);stroke:#fff;stroke-width:2" }
                            text { x: "0", y: "6", style: "fill:#fff;font-size:16px;text-anchor:middle;font-family:serif", "\u{270E}" }
                        }
                    }
                }
            }
        }
        None => {
            let child_id = node.child_of;
            let is_father = node.is_father;
            let plus_x = CARD_PADDING + rw / 2.0;
            let plus_y = CARD_PADDING + rh / 2.0 + 8.0;
            rsx! {
                g { key: "{key}", transform: "translate({nx},{ny})",
                    if let (true, Some(cid)) = (allow_empty_click, child_id) {
                        g {
                            style: "cursor:pointer",
                            onclick: move |_| on_empty_slot.call((cid, is_father)),
                            rect { x: "{CARD_PADDING}", y: "{CARD_PADDING}", rx: "{CARD_BORDER_RADIUS}", ry: "{CARD_BORDER_RADIUS}", width: "{rw}", height: "{rh}", style: "fill:var(--pn-bg);stroke:#888;stroke-width:1;stroke-dasharray:4,4" }
                            text { x: "{plus_x}", y: "{plus_y}", style: "fill:var(--pn-root-bg);font-size:22px;font-weight:700;text-anchor:middle;font-family:sans-serif", "+" }
                        }
                    } else {
                        rect { x: "{CARD_PADDING}", y: "{CARD_PADDING}", rx: "{CARD_BORDER_RADIUS}", ry: "{CARD_BORDER_RADIUS}", width: "{rw}", height: "{rh}", style: "fill:var(--pn-bg);stroke:#888;stroke-width:1;stroke-dasharray:4,4;opacity:0.3" }
                    }
                }
            }
        }
    }
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
    let layout = compute_layout(
        props.root_person_id,
        &data_with_sosa,
        ancestor_levels(),
        descendant_levels(),
    );

    // ── Center root card in viewport when needed ──
    if needs_center() {
        let root_center = Some((layout.root_cx, layout.root_cy));
        if let Some((rcx, rcy)) = root_center {
            needs_center.set(false);
            spawn(async move {
                // Small delay so the DOM has rendered the viewport element.
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                if let Ok(val) = document::eval(
                    "var el = document.querySelector('.pedigree-viewport'); return el ? [el.clientWidth, el.clientHeight] : [800, 600]"
                ).await {
                    let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(VIEWPORT_DEFAULT_W);
                    let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(VIEWPORT_DEFAULT_H);
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
    let sel_portrait_src = props
        .data
        .photos
        .get(&sel_pid)
        .map(|url| url.as_str())
        .unwrap_or_else(|| default_portrait(props.data.sex_of(sel_pid)))
        .to_string();
    let sel_birth = props.data.birth_date(sel_pid).unwrap_or_default();
    let sel_death = props.data.death_date(sel_pid).unwrap_or_default();
    let sel_dates = match (sel_birth.is_empty(), sel_death.is_empty()) {
        (true, true) => String::new(),
        (false, true) => i18n.t_args("pedigree.birth_year_abbr", &[("year", &sel_birth)]),
        (true, false) => i18n.t_args("pedigree.death_year_abbr", &[("year", &sel_death)]),
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
    sel_events.sort_by_key(|a| a.id);
    sel_events.dedup_by_key(|e| e.id);
    sel_events.sort_by_key(|a| a.date_sort);

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
    let fit_content_cx = layout.content_cx;
    let fit_content_cy = layout.content_cy;
    let fit_content_w = layout.content_w;
    let fit_content_h = layout.content_h;

    // Adapt the descending side's empty "+" slot (missing spouse) onto the
    // dedicated add-spouse callback — the `bool` (father/mother) from
    // `on_empty_slot` doesn't apply here, only the person needing a spouse.
    let on_add_spouse_slot = props.on_add_spouse_slot;
    let desc_empty_slot_adapter =
        EventHandler::new(move |(pid, _): (Uuid, bool)| on_add_spouse_slot.call(pid));

    rsx! {
        div { class: "pedigree-outer",

            // ══════════════════════════════════
            // ICON SIDEBAR
            // ══════════════════════════════════
            TreeIconSidebar {
                active_view: TreeSidebarView::Pedigree,
                selected_person_id: Some(selected_person_id()),
                on_profile_view: move |pid| {
                    if let Some(pid) = pid {
                        props.on_profile_view.call(pid);
                    }
                },
                on_pedigree_view: move |_| {},
                on_add_person: props.on_add_person,
                on_settings: props.on_settings,

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
                    onclick: move |_| scale.set((scale() * ZOOM_FACTOR).clamp(ZOOM_MIN, ZOOM_MAX)),
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
                    onclick: move |_| scale.set((scale() / ZOOM_FACTOR).clamp(ZOOM_MIN, ZOOM_MAX)),
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
                                r#"
                                const viewport = document.querySelector('.pedigree-viewport');
                                if (!viewport) return [800, 600, 0];
                                const rect = viewport.getBoundingClientRect();
                                const panel = document.querySelector('.ev-panel:not(.ev-panel-collapsed)');
                                let availableLeft = 0;
                                let availableRight = rect.width;
                                if (panel) {
                                    const panelRect = panel.getBoundingClientRect();
                                    const overlapsX = panelRect.left < rect.right && panelRect.right > rect.left;
                                    if (overlapsX) {
                                        availableRight = Math.min(availableRight, panelRect.left - rect.left);
                                    }
                                }
                                return [Math.max(1, availableRight - availableLeft), rect.height, availableLeft];
                                "#
                            ).await {
                                let vw = val.get(0).and_then(|v| v.as_f64()).unwrap_or(VIEWPORT_DEFAULT_W);
                                let vh = val.get(1).and_then(|v| v.as_f64()).unwrap_or(VIEWPORT_DEFAULT_H);
                                let vx = val.get(2).and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let fit_scale = (vw / fit_content_w).min(vh / fit_content_h).clamp(ZOOM_MIN, ZOOM_MAX);
                                scale.set(fit_scale);
                                offset_x.set(vx + vw / 2.0 - fit_content_cx * fit_scale);
                                offset_y.set(vh / 2.0 - fit_content_cy * fit_scale);
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
                    let old_scale = scale();
                    let new_scale = (old_scale * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                    if (new_scale - old_scale).abs() > f64::EPSILON {
                        let coords = evt.client_coordinates();
                        let old_offset_x = offset_x();
                        let old_offset_y = offset_y();
                        spawn(async move {
                            if let Ok(val) = document::eval(
                                r#"
                                const viewport = document.querySelector('.pedigree-viewport');
                                if (!viewport) return [0, 0];
                                const rect = viewport.getBoundingClientRect();
                                return [rect.left, rect.top];
                                "#
                            ).await {
                                let vx = val.get(0).and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let vy = val.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let mouse_x = coords.x - vx;
                                let mouse_y = coords.y - vy;
                                let world_x = (mouse_x - old_offset_x) / old_scale;
                                let world_y = (mouse_y - old_offset_y) / old_scale;
                                scale.set(new_scale);
                                offset_x.set(mouse_x - world_x * new_scale);
                                offset_y.set(mouse_y - world_y * new_scale);
                            }
                        });
                    }
                },

                div {
                    class: inner_class,
                    style: "transform: {transform};",

                    div {
                        class: "pedigree-tree",
                        style: "position: relative; width: {layout.total_w}px; height: {layout.total_h}px;",

                        svg {
                            "viewBox": "0 0 {layout.total_w} {layout.total_h}",
                            width: "{layout.total_w}",
                            height: "{layout.total_h}",
                            style: "display: block; overflow: visible;",

                            g { transform: "translate({layout.main_tx},{layout.main_ty})",

                                // ── Ascending tree ──
                                g {
                                    for (si, path) in layout.asc_links.iter().enumerate() {
                                        path { key: "al-{si}", d: "{path.d}", class: "pedigree-connector-path", fill: "none" }
                                    }
                                    for (ni, node) in layout.asc_nodes.iter().enumerate() {
                                        {render_pedigree_card(
                                            node,
                                            ni,
                                            "an",
                                            props.root_person_id,
                                            selected_person_id,
                                            props.on_person_navigate,
                                            props.on_person_click,
                                            props.on_empty_slot,
                                            true,
                                        )}
                                    }
                                }

                                // ── Descending tree ──
                                g {
                                    transform: "translate({layout.desc_tx},{layout.desc_ty})",
                                    for (si, path) in layout.desc_links.iter().enumerate() {
                                        path { key: "dl-{si}", d: "{path.d}", class: "pedigree-connector-path", fill: "none" }
                                    }
                                    for (ni, node) in layout.desc_nodes.iter().enumerate() {
                                        {render_pedigree_card(
                                            node,
                                            ni,
                                            "dn",
                                            props.root_person_id,
                                            selected_person_id,
                                            props.on_person_navigate,
                                            props.on_person_click,
                                            desc_empty_slot_adapter,
                                            true,
                                        )}
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
                        div { class: "evp-av",
                            img { src: "{sel_portrait_src}", alt: "" }
                        }
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

#[cfg(test)]
mod layout_overlap_tests {
    use super::*;

    fn person(depth: i32, sex: Sex, parent2: Option<usize>) -> TreeNode {
        let mut n = TreeNode::new_real(
            Uuid::now_v7(),
            depth,
            sex,
            "Given".to_string(),
            "Surname".to_string(),
            None,
            None,
            None,
            SosaBadge::None,
            if sex == Sex::Female { 1 } else { 0 },
            false,
            false,
        );
        n.parent2 = parent2;
        n
    }

    /// Pushes `node` onto `arena` and returns its index.
    fn push(arena: &mut Vec<TreeNode>, node: TreeNode) -> usize {
        arena.push(node);
        arena.len() - 1
    }

    /// Attaches `spouse_idx` as a sibling (spouse) of `node_idx`.
    fn marry(arena: &mut [TreeNode], node_idx: usize, spouse_idx: usize) {
        arena[node_idx].siblings.push(spouse_idx);
        arena[spouse_idx].is_sibling = true;
    }

    /// Reproduces a cross-cousin overlap: depth-1 siblings A (with two
    /// depth-2 children, the second one childless-but-married) and B (with
    /// a single depth-2 child) must not have their depth-2 rows collide.
    #[test]
    fn cousin_branches_do_not_overlap_at_depth_two() {
        let mut arena: Vec<TreeNode> = Vec::new();

        // Root couple.
        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        // Depth-1 children: a single childless sibling, then A (male) and B
        // (female) adjacent, then two more single childless siblings —
        // mirrors the real family shape (Brigitte, [Luc+spouse], [Daniel+
        // Elisabeth], Marc, Jean-Michel).
        let sib_before = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        let a = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let b = push(&mut arena, person(1, Sex::Female, Some(root_spouse)));
        let sib_after1 = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![sib_before, a, b, sib_after1];

        let a_spouse = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, a, a_spouse);

        let b_spouse = push(&mut arena, person(1, Sex::Male, None));
        marry(&mut arena, b, b_spouse);

        // A's children: first child has a spouse with no children of their
        // own (the trailing, "childless spouse" case); second is the last
        // child in A's branch.
        let a_child1 = push(&mut arena, person(2, Sex::Male, Some(a_spouse)));
        let a_child1_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, a_child1, a_child1_spouse);

        let a_child2 = push(&mut arena, person(2, Sex::Male, Some(a_spouse)));
        let a_child2_spouse = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, a_child2, a_child2_spouse);
        arena[a].children = vec![a_child1, a_child2];

        // B's single child (no spouse) — the cousin branch directly to the
        // right of A's branch.
        let b_child = push(&mut arena, person(2, Sex::Female, Some(b_spouse)));
        arena[b].children = vec![b_child];

        layout_tree(&mut arena, 0);

        // Every pair of cards at the same depth must not horizontally
        // overlap (allowing exact edge-touch).
        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        eprintln!(
            "root={root} root_spouse={root_spouse} sib_before={sib_before} a={a} b={b} \
             sib_after1={sib_after1} a_spouse={a_spouse} \
             b_spouse={b_spouse} a_child1={a_child1} a_child1_spouse={a_child1_spouse} \
             a_child2={a_child2} a_child2_spouse={a_child2_spouse} b_child={b_child}"
        );
        for (i, n) in arena.iter().enumerate() {
            eprintln!(
                "idx={i} depth={} x={} after={} sex={:?}",
                n.depth, n.x, n.after, n.sex
            );
        }

        for depth in [0, 1, 2] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Reproduces a case one level shallower than the cousin-branch test: a
    /// leaf sibling (Philippe) who has both a married-in spouse AND his own
    /// children, sitting next to a childless full sibling (Anthony). The
    /// spouse's extra width must still push the childless sibling (and
    /// everything after it) over, even though Philippe's own subtree has
    /// descendants of its own.
    #[test]
    fn leaf_with_spouse_and_children_does_not_overlap_childless_sibling() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let philippe = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let anthony = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let remi = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![philippe, anthony, remi];

        let marion = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, philippe, marion);

        let lily = push(&mut arena, person(2, Sex::Female, Some(marion)));
        let alban = push(&mut arena, person(2, Sex::Male, Some(marion)));
        arena[philippe].children = vec![lily, alban];

        layout_tree(&mut arena, 0);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        for depth in [1, 2] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Reproduces the exact real-world family shape that still overlapped
    /// after the first fix: depth-1 full siblings JMJA and Dominique (both
    /// children of the root couple). JMJA's branch has two childless
    /// children (Didier, Michel) before a third child (Philippe) who has
    /// both a spouse (Marion) and two children of his own (Lily, Alban).
    /// Dominique's branch immediately follows with two childless children
    /// (Fanny, Rémi). Marion's card must not collide with Fanny's.
    #[test]
    fn cross_uncle_branch_with_grandchildren_does_not_overlap() {
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));
        let root_spouse = push(&mut arena, person(0, Sex::Female, None));
        marry(&mut arena, root, root_spouse);

        let jmja = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        let dominique = push(&mut arena, person(1, Sex::Male, Some(root_spouse)));
        arena[root].children = vec![jmja, dominique];

        let augusta = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, jmja, augusta);

        let francine = push(&mut arena, person(1, Sex::Female, None));
        marry(&mut arena, dominique, francine);

        let didier = push(&mut arena, person(2, Sex::Male, Some(augusta)));
        let michel = push(&mut arena, person(2, Sex::Male, Some(augusta)));
        let philippe = push(&mut arena, person(2, Sex::Male, Some(augusta)));
        arena[jmja].children = vec![didier, michel, philippe];

        // Didier also has his own spouse + 3 children (Clément, Auriane,
        // Apolline) — omitting these in an earlier version of this test
        // masked the real bug.
        let anne_cecile = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, didier, anne_cecile);
        let clement = push(&mut arena, person(3, Sex::Male, Some(anne_cecile)));
        let auriane = push(&mut arena, person(3, Sex::Female, Some(anne_cecile)));
        let apolline = push(&mut arena, person(3, Sex::Female, Some(anne_cecile)));
        arena[didier].children = vec![clement, auriane, apolline];

        // Michel also has his own spouse + 1 child (Maël).
        let marie_charlotte = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, michel, marie_charlotte);
        let mael = push(&mut arena, person(3, Sex::Male, Some(marie_charlotte)));
        arena[michel].children = vec![mael];

        let marion = push(&mut arena, person(2, Sex::Female, None));
        marry(&mut arena, philippe, marion);

        let lily = push(&mut arena, person(3, Sex::Female, Some(marion)));
        let alban = push(&mut arena, person(3, Sex::Male, Some(marion)));
        arena[philippe].children = vec![lily, alban];

        let fanny = push(&mut arena, person(2, Sex::Female, Some(francine)));
        let remi = push(&mut arena, person(2, Sex::Male, Some(francine)));
        arena[dominique].children = vec![fanny, remi];

        layout_tree(&mut arena, 0);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        for depth in [1, 2, 3] {
            let mut xs = by_depth(depth);
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for w in xs.windows(2) {
                let gap = w[1] - w[0];
                assert!(
                    gap + 1e-6 >= CARD_W,
                    "depth {depth}: cards overlap (gap={gap}, need >= {CARD_W})"
                );
            }
        }
    }

    /// Ascending trees never populate a node's own `siblings` (married-in
    /// spouse) field, so `fix_spouse_group_overlaps` must stay a no-op there
    /// and leave the RT-computed compact 0.5-unit separation at the deepest
    /// (last_level) row untouched — i.e. grandparent pairs must stay packed
    /// at half the normal card spacing, not be force-spread to a full
    /// CARD_W gap.
    #[test]
    fn ascending_compact_row_keeps_half_width_separation() {
        let last_level = -2;
        let mut arena: Vec<TreeNode> = Vec::new();

        let root = push(&mut arena, person(0, Sex::Male, None));

        let father = push(&mut arena, person(-1, Sex::Male, None));
        let mother = push(&mut arena, person(-1, Sex::Female, None));
        arena[root].children = vec![father, mother];

        let father_father = push(&mut arena, person(last_level, Sex::Male, None));
        let father_mother = push(&mut arena, person(last_level, Sex::Female, None));
        arena[father].children = vec![father_father, father_mother];

        let mother_father = push(&mut arena, person(last_level, Sex::Male, None));
        let mother_mother = push(&mut arena, person(last_level, Sex::Female, None));
        arena[mother].children = vec![mother_father, mother_mother];

        layout_tree(&mut arena, last_level);

        let by_depth =
            |d: i32| -> Vec<f64> { arena.iter().filter(|n| n.depth == d).map(|n| n.x).collect() };

        let mut compact_xs = by_depth(last_level);
        compact_xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for w in compact_xs.windows(2) {
            let gap = w[1] - w[0];
            assert!(
                gap < CARD_W - 1e-6,
                "compact row: gap widened to a full card width (gap={gap}, expected < {CARD_W})"
            );
        }
    }
}
