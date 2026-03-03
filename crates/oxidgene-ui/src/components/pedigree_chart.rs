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

use oxidgene_core::types::{
    Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName, Place,
};
use oxidgene_core::{EventType, Sex};

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

/// Returns `(icon, css_class, label)` for an event type.
fn event_ui(et: EventType) -> (&'static str, &'static str, &'static str) {
    match et {
        EventType::Birth => ("\u{2726}", "ev-ic ev-ic-birth", "Birth"),
        EventType::Baptism => ("\u{271F}", "ev-ic ev-ic-birth", "Baptism"),
        EventType::Death => ("\u{271D}", "ev-ic ev-ic-death", "Death"),
        EventType::Burial => ("\u{26B0}", "ev-ic ev-ic-death", "Burial"),
        EventType::Cremation => ("\u{271D}", "ev-ic ev-ic-death", "Cremation"),
        EventType::Marriage => ("\u{1F48D}", "ev-ic ev-ic-marry", "Marriage"),
        EventType::Engagement => ("\u{1F48D}", "ev-ic ev-ic-marry", "Engagement"),
        EventType::MarriageBann => ("\u{1F48D}", "ev-ic ev-ic-marry", "Banns"),
        EventType::MarriageContract => ("\u{1F48D}", "ev-ic ev-ic-marry", "Contract"),
        EventType::MarriageLicense => ("\u{1F48D}", "ev-ic ev-ic-marry", "License"),
        EventType::MarriageSettlement => ("\u{1F48D}", "ev-ic ev-ic-marry", "Settlement"),
        EventType::Divorce => ("\u{2696}", "ev-ic ev-ic-other", "Divorce"),
        EventType::Annulment => ("\u{2696}", "ev-ic ev-ic-other", "Annulment"),
        EventType::Graduation => ("\u{25C6}", "ev-ic ev-ic-other", "Graduation"),
        EventType::Immigration => ("\u{25C6}", "ev-ic ev-ic-other", "Immigration"),
        EventType::Emigration => ("\u{25C6}", "ev-ic ev-ic-other", "Emigration"),
        EventType::Naturalization => ("\u{25C6}", "ev-ic ev-ic-other", "Naturalization"),
        EventType::Census => ("\u{1F4DC}", "ev-ic ev-ic-other", "Census"),
        EventType::Occupation => ("\u{2692}", "ev-ic ev-ic-other", "Occupation"),
        EventType::Residence => ("\u{1F3E1}", "ev-ic ev-ic-other", "Residence"),
        EventType::Retirement => ("\u{25C6}", "ev-ic ev-ic-other", "Retirement"),
        EventType::Will => ("\u{1F4DC}", "ev-ic ev-ic-other", "Will"),
        EventType::Probate => ("\u{1F4DC}", "ev-ic ev-ic-other", "Probate"),
        EventType::Other => ("\u{25C6}", "ev-ic ev-ic-other", "Event"),
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

    let total_width = anc_width.max(max_desc_width).max(CARD_W);
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

    // ── Root node ──
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

    // ── Descendant nodes + connectors ──
    // Track the center-x of each person in the current generation for connector drawing.
    let mut person_cx: HashMap<Uuid, f64> = HashMap::new();
    person_cx.insert(root_id, root_x + CARD_W / 2.0);

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

            // Find parent center x.
            let parent_cx = person_cx
                .get(&fam.parent_id)
                .copied()
                .or_else(|| fam.spouse_id.and_then(|sid| person_cx.get(&sid).copied()))
                .unwrap_or(x_cursor + fam_w / 2.0);

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
    pub on_person_click: EventHandler<(Uuid, f64, f64)>,
    pub on_person_navigate: EventHandler<Uuid>,
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
    #[props(default)]
    pub on_add_person: EventHandler<()>,
    #[props(default)]
    pub on_profile_view: EventHandler<Uuid>,
}

#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    // ── Depth controls (max 10) ──
    let mut ancestor_levels = use_signal(|| 4usize);
    let mut descendant_levels = use_signal(|| 3usize);
    let mut depth_hover = use_signal(|| false);
    let mut depth_hover_timeout = use_signal(|| None::<i32>);

    // ── Pan state ──
    let mut offset_x = use_signal(|| 0.0f64);
    let mut offset_y = use_signal(|| 0.0f64);
    let mut dragging = use_signal(|| false);
    let mut drag_start_x = use_signal(|| 0.0f64);
    let mut drag_start_y = use_signal(|| 0.0f64);
    let mut drag_origin_x = use_signal(|| 0.0f64);
    let mut drag_origin_y = use_signal(|| 0.0f64);

    // ── Zoom state ──
    let mut scale = use_signal(|| 1.0f64);

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
    let mut animating = use_signal(|| true);

    // ── Reset pan/zoom/selection when the root person changes ──
    let mut prev_root = use_signal(|| props.root_person_id);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        animating.set(false);
        offset_x.set(0.0);
        offset_y.set(0.0);
        scale.set(1.0);
        selected_person_id.set(props.root_person_id);
        // Re-enable animation after a frame.
        spawn(async move {
            gloo_timers::future::TimeoutFuture::new(50).await;
            animating.set(true);
        });
    }

    // ── Compute layout ──
    let (layout_nodes, connector_segments, total_w, total_h) = compute_layout(
        props.root_person_id,
        &props.data,
        ancestor_levels(),
        descendant_levels(),
    );

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
    let mut sel_events: Vec<DomainEvent> = props
        .data
        .events_by_person
        .get(&sel_pid)
        .cloned()
        .unwrap_or_default();
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
                    title: "Tree view",
                    "\u{1F333}" // 🌳
                }

                // Profile view
                {
                    let sel = selected_person_id();
                    let on_profile = props.on_profile_view;
                    rsx! {
                        button {
                            class: "isb-btn",
                            title: "Person profile",
                            onclick: move |_| on_profile.call(sel),
                            "\u{1F464}" // 👤
                        }
                    }
                }

                div { class: "isb-hr" }

                // Depth selector (hover popover)
                div {
                    class: "isb-depth-wrap",
                    onmouseenter: move |_| {
                        // Cancel any pending close timeout.
                        if let Some(tid) = depth_hover_timeout() {
                            document::eval(&format!("clearTimeout({})", tid));
                            depth_hover_timeout.set(None);
                        }
                        depth_hover.set(true);
                    },
                    onmouseleave: move |_| {
                        // Close after 150ms delay.
                        spawn(async move {
                            gloo_timers::future::TimeoutFuture::new(150).await;
                            depth_hover.set(false);
                        });
                    },
                    button {
                        class: "isb-btn",
                        title: "Generation depth",
                        "\u{2261}" // ≡
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
                    title: "Zoom in",
                    onclick: move |_| scale.set((scale() * 1.2).clamp(0.3, 2.0)),
                    "\u{2295}" // ⊕
                }
                button {
                    class: "isb-btn",
                    title: "Zoom out",
                    onclick: move |_| scale.set((scale() / 1.2).clamp(0.3, 2.0)),
                    "\u{2296}" // ⊖
                }
                span { class: "isb-zoom-val", "{zoom_pct}%" }
                button {
                    class: "isb-btn",
                    title: "Fit to screen",
                    onclick: move |_| {
                        // Estimate viewport as 800x600 (reasonable default).
                        // A proper implementation would measure the DOM element.
                        let vw = 800.0f64;
                        let vh = 600.0f64;
                        let fit_scale = (vw / fit_total_w).min(vh / fit_total_h).clamp(0.3, 2.0) * 0.85;
                        scale.set(fit_scale);
                        offset_x.set(0.0);
                        offset_y.set(0.0);
                    },
                    "FIT"
                }

                div { class: "isb-hr" }

                // Add person
                {
                    let on_add = props.on_add_person;
                    rsx! {
                        button {
                            class: "isb-btn",
                            title: "Add a person",
                            onclick: move |_| on_add.call(()),
                            "+\u{1F464}"
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
                                        let is_root = node.generation == 0;
                                        let role_class = if is_root {
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
                                        let sel_part = if is_sel || is_root { "selected" } else { "" };
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
                                                    div { class: "pc-ph", "{initials}" }
                                                    div { class: "pc-body",
                                                        div { class: "pc-name",
                                                            if !surname_s.is_empty() {
                                                                span { class: "pc-last", "{surname_s}" }
                                                            }
                                                            if !given_s.is_empty() {
                                                                span { class: "pc-first", "{given_s}" }
                                                            }
                                                            if !has_name {
                                                                span { class: "pc-first", "Unknown" }
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
                                                // Pencil FAB on root person
                                                if is_root {
                                                    button {
                                                        class: "pedigree-edit-fab",
                                                        title: "Edit / actions",
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
                    title: if panel_collapsed() { "Show events" } else { "Hide events" },
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
                    div { class: "evp-hd", "Events" }
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
                            div { class: "evp-empty", "No events recorded" }
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
                                                    let (icon, ic_class, label) = event_ui(evt.event_type);
                                                    let date_s = evt.date_value.clone().unwrap_or_default();
                                                    let place_s = evt.place_id
                                                        .and_then(|pid| props.data.place_name(pid).map(String::from))
                                                        .or_else(|| evt.description.clone())
                                                        .unwrap_or_default();
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
                                                                div { class: "ev-type", "{label}" }
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
