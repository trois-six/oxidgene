//! Vertical bidirectional pedigree chart with pan/zoom, icon sidebar, and event panel.
//!
//! Layout: `.pedigree-outer` (flex row)
//!   → `.isb` (icon sidebar: depth/zoom controls)
//!   → `.pedigree-viewport` (pannable/zoomable canvas)
//!   → `.ev-panel` (selected-person event list)

use std::collections::HashMap;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use uuid::Uuid;

use oxidgene_core::types::{Event as DomainEvent, FamilyChild, FamilySpouse, Person, PersonName};
use oxidgene_core::{EventType, Sex};

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
        format!("{}…", &date[..10])
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
        EventType::Birth => ("✦", "ev-ic ev-ic-birth", "Birth"),
        EventType::Baptism => ("✦", "ev-ic ev-ic-birth", "Baptism"),
        EventType::Death => ("✝", "ev-ic ev-ic-death", "Death"),
        EventType::Burial => ("✝", "ev-ic ev-ic-death", "Burial"),
        EventType::Cremation => ("✝", "ev-ic ev-ic-death", "Cremation"),
        EventType::Marriage => ("♥", "ev-ic ev-ic-marry", "Marriage"),
        EventType::Engagement => ("♥", "ev-ic ev-ic-marry", "Engagement"),
        EventType::MarriageBann => ("♥", "ev-ic ev-ic-marry", "Banns"),
        EventType::MarriageContract => ("♥", "ev-ic ev-ic-marry", "Contract"),
        EventType::MarriageLicense => ("♥", "ev-ic ev-ic-marry", "License"),
        EventType::MarriageSettlement => ("♥", "ev-ic ev-ic-marry", "Settlement"),
        EventType::Divorce => ("⊗", "ev-ic ev-ic-marry", "Divorce"),
        EventType::Annulment => ("⊗", "ev-ic ev-ic-marry", "Annulment"),
        EventType::Graduation => ("◆", "ev-ic ev-ic-other", "Graduation"),
        EventType::Immigration => ("◆", "ev-ic ev-ic-other", "Immigration"),
        EventType::Emigration => ("◆", "ev-ic ev-ic-other", "Emigration"),
        EventType::Naturalization => ("◆", "ev-ic ev-ic-other", "Naturalization"),
        EventType::Census => ("◆", "ev-ic ev-ic-other", "Census"),
        EventType::Occupation => ("◆", "ev-ic ev-ic-other", "Occupation"),
        EventType::Residence => ("◆", "ev-ic ev-ic-other", "Residence"),
        EventType::Retirement => ("◆", "ev-ic ev-ic-other", "Retirement"),
        EventType::Will => ("◆", "ev-ic ev-ic-other", "Will"),
        EventType::Probate => ("◆", "ev-ic ev-ic-other", "Probate"),
        EventType::Other => ("◆", "ev-ic ev-ic-other", "Event"),
    }
}

/// Data needed to render the pedigree chart, pre-computed from API data.
#[derive(Clone, Debug)]
pub struct PedigreeData {
    /// All persons in the tree, keyed by ID.
    pub persons: HashMap<Uuid, Person>,
    /// Person names, keyed by person ID.
    pub names: HashMap<Uuid, Vec<PersonName>>,
    /// Family spouses, keyed by family ID.
    pub spouses_by_family: HashMap<Uuid, Vec<FamilySpouse>>,
    /// Family children, keyed by family ID.
    pub children_by_family: HashMap<Uuid, Vec<FamilyChild>>,
    /// Map from person ID → list of family IDs where they are a child.
    pub families_as_child: HashMap<Uuid, Vec<Uuid>>,
    /// Map from person ID → list of family IDs where they are a spouse.
    pub families_as_spouse: HashMap<Uuid, Vec<Uuid>>,
    /// Events keyed by person ID (individual events).
    pub events_by_person: HashMap<Uuid, Vec<DomainEvent>>,
    /// Events keyed by family ID (family events such as marriage).
    pub events_by_family: HashMap<Uuid, Vec<DomainEvent>>,
}

// Manual PartialEq: always consider data changed to ensure re-renders.
impl PartialEq for PedigreeData {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl PedigreeData {
    /// Build pedigree data from raw API results.
    pub fn build(
        persons: &[Person],
        names: HashMap<Uuid, Vec<PersonName>>,
        all_spouses: &[FamilySpouse],
        all_children: &[FamilyChild],
        events: Vec<DomainEvent>,
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
        }
    }

    /// Get parents of a person as (father_id, mother_id).
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

    /// Get the sex of a person.
    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.persons
            .get(&person_id)
            .map(|p| p.sex)
            .unwrap_or(Sex::Unknown)
    }

    /// Get primary name parts: `(given_names, surname, nickname)`.
    fn name_parts(&self, person_id: Uuid) -> (Option<String>, Option<String>, Option<String>) {
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

    /// Get birth year (or baptism year as fallback).
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

    /// Get death year (or burial year as fallback).
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

    /// Get marriage year for a family.
    fn marriage_date_for_family(&self, family_id: Uuid) -> Option<String> {
        let events = self.events_by_family.get(&family_id)?;
        events
            .iter()
            .find(|e| e.event_type == EventType::Marriage)
            .and_then(|e| e.date_value.as_deref().map(fmt_year))
    }

    /// Get the first family_id where this person is a child (their parents' family).
    fn parent_family_of(&self, person_id: Uuid) -> Option<Uuid> {
        self.families_as_child.get(&person_id)?.first().copied()
    }
}

/// A slot in the ancestor pedigree chart: either a known person or an empty placeholder.
#[derive(Clone, Debug)]
enum PedigreeSlot {
    Person(Uuid),
    Empty,
}

/// Build ancestor slots up to `levels` generations.
/// Returns a Vec of generations: index 0 = root, 1 = parents, etc.
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

/// A family group in the descendant tree: parent + optional spouse + children.
#[derive(Clone, Debug)]
struct DescendantFamily {
    /// The parent person from the previous generation.
    #[allow(dead_code)]
    parent_id: Uuid,
    /// The spouse/partner in this family (if any).
    #[allow(dead_code)]
    spouse_id: Option<Uuid>,
    /// The family ID linking them.
    family_id: Uuid,
    /// Children of this family.
    children: Vec<Uuid>,
}

/// A descendant generation: a list of family groups.
#[derive(Clone, Debug)]
struct DescendantGeneration {
    families: Vec<DescendantFamily>,
}

/// Build descendant generations grouped by family, for proper branching connectors.
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
                    continue; // already processed this family via the other spouse
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

/// Props for [`PedigreeChart`].
#[derive(Props, Clone, PartialEq)]
pub struct PedigreeChartProps {
    /// The root person to center the chart on.
    pub root_person_id: Uuid,
    /// Pre-computed pedigree data.
    pub data: PedigreeData,
    /// Tree ID string for navigation links.
    pub tree_id: String,
    /// Called when the user clicks the edit FAB on the root person (to open context menu).
    pub on_person_click: EventHandler<(Uuid, f64, f64)>,
    /// Called when the user left-clicks any person node (to re-root the chart on that person).
    pub on_person_navigate: EventHandler<Uuid>,
    /// Called when the user clicks an empty ancestor slot (to add a parent).
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
}

/// Vertical bidirectional pedigree chart with icon sidebar and event panel.
///
/// Layout: `.pedigree-outer` → [`.isb` | `.pedigree-viewport` | `.ev-panel`]
#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    // ── Depth controls ──
    let mut ancestor_levels = use_signal(|| 4usize);
    let mut descendant_levels = use_signal(|| 3usize);
    let mut show_depth_popover = use_signal(|| false);

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

    // ── Selected person (drives the event panel) ──
    let mut selected_person_id = use_signal(|| props.root_person_id);

    // ── Reset pan/zoom/selection when the root person changes ──
    let mut prev_root = use_signal(|| props.root_person_id);
    if prev_root() != props.root_person_id {
        prev_root.set(props.root_person_id);
        offset_x.set(0.0);
        offset_y.set(0.0);
        scale.set(1.0);
        selected_person_id.set(props.root_person_id);
    }

    // ── Build ancestor tree ──
    let anc_gens = build_ancestor_slots(props.root_person_id, &props.data, ancestor_levels());

    let max_anc_gen_idx = anc_gens
        .iter()
        .rposition(|g| g.iter().any(|s| matches!(s, PedigreeSlot::Person(_))))
        .unwrap_or(0);

    let display_anc_gens = (max_anc_gen_idx + 1)
        .min(anc_gens.len())
        .max(if ancestor_levels() > 0 { 2 } else { 1 });

    let deepest_anc = display_anc_gens.saturating_sub(1);

    // ── Build descendant generations (grouped by family) ──
    let desc_gens =
        build_descendant_generations(props.root_person_id, &props.data, descendant_levels());

    // ── CSS transform for pan/zoom ──
    let transform = format!(
        "translate({}px, {}px) scale({})",
        offset_x(),
        offset_y(),
        scale()
    );
    let zoom_pct = (scale() * 100.0) as u32;

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
        _ => format!("{sel_birth} – {sel_death}"),
    };
    let sel_events: Vec<DomainEvent> = props
        .data
        .events_by_person
        .get(&sel_pid)
        .cloned()
        .unwrap_or_default();

    rsx! {
        div { class: "pedigree-outer",

            // ══════════════════════════════════
            // ICON SIDEBAR
            // ══════════════════════════════════
            div { class: "isb",

                // Depth toggle with popover
                div { style: "position: relative;",
                    button {
                        class: "isb-btn",
                        title: "Generation depth",
                        onclick: move |_| show_depth_popover.toggle(),
                        "≡"
                    }
                    if show_depth_popover() {
                        div { class: "pedigree-depth-popover",
                            div { class: "pedigree-depth-title", "Generations" }

                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-label", "↑ Ancestors" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() > 0 { ancestor_levels -= 1; } },
                                    "−"
                                }
                                span { class: "pedigree-depth-val", "{ancestor_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if ancestor_levels() < 6 { ancestor_levels += 1; } },
                                    "+"
                                }
                            }

                            div { class: "pedigree-depth-row",
                                span { class: "pedigree-depth-label", "↓ Descendants" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() > 0 { descendant_levels -= 1; } },
                                    "−"
                                }
                                span { class: "pedigree-depth-val", "{descendant_levels()}" }
                                button {
                                    class: "pedigree-depth-btn",
                                    onclick: move |_| { if descendant_levels() < 6 { descendant_levels += 1; } },
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
                    onclick: move |_| scale.set((scale() * 1.2).clamp(0.3, 3.0)),
                    "⊕"
                }
                button {
                    class: "isb-btn",
                    title: "Zoom out",
                    onclick: move |_| scale.set((scale() / 1.2).clamp(0.3, 3.0)),
                    "⊖"
                }
                span { class: "isb-zoom-val", "{zoom_pct}%" }
                button {
                    class: "isb-btn",
                    title: "Reset view",
                    onclick: move |_| {
                        scale.set(1.0);
                        offset_x.set(0.0);
                        offset_y.set(0.0);
                    },
                    "⊙"
                }
            }

            // ══════════════════════════════════
            // CANVAS VIEWPORT (pannable/zoomable)
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
                    class: "pedigree-inner",
                    style: "transform: {transform};",

                    div { class: "pedigree-tree",

                        // ═══════════════════════════════════════════
                        // ANCESTOR ROWS — deepest generation at top
                        // ═══════════════════════════════════════════
                        for gen_idx in (1..display_anc_gens).rev() {
                            {
                                let gen_slots: Vec<PedigreeSlot> = anc_gens[gen_idx].clone();
                                let child_gen: Vec<PedigreeSlot> = anc_gens[gen_idx - 1].clone();
                                let slot_flex = 1usize << deepest_anc.saturating_sub(gen_idx);
                                let num_groups = 1usize << (gen_idx - 1);
                                let group_flex = 1usize << deepest_anc.saturating_sub(gen_idx - 1);

                                let marriage_dates: Vec<String> = (0..num_groups)
                                    .map(|gi| {
                                        match child_gen.get(gi) {
                                            Some(PedigreeSlot::Person(pid)) => props.data
                                                .parent_family_of(*pid)
                                                .and_then(|fid| props.data.marriage_date_for_family(fid))
                                                .unwrap_or_default(),
                                            _ => String::new(),
                                        }
                                    })
                                    .collect();

                                rsx! {
                                    // Generation row
                                    div {
                                        key: "anc-row-{gen_idx}",
                                        class: "pedigree-gen-row",
                                        for (slot_idx, slot) in gen_slots.iter().enumerate() {
                                            {
                                                let style = format!("flex: {slot_flex};");
                                                match slot {
                                                    PedigreeSlot::Person(pid) => {
                                                        let pid = *pid;
                                                        let sex = props.data.sex_of(pid);
                                                        let (given, surname, _) = props.data.name_parts(pid);
                                                        let has_name = given.is_some() || surname.is_some();
                                                        let given_s = given.unwrap_or_default();
                                                        let surname_s = surname.unwrap_or_default();
                                                        let birth_s = props.data.birth_date(pid).unwrap_or_default();
                                                        let death_s = props.data.death_date(pid).unwrap_or_default();
                                                        let initials = make_initials(&given_s, &surname_s);
                                                        let sex_class = match sex {
                                                            Sex::Male    => "pedigree-node male",
                                                            Sex::Female  => "pedigree-node female",
                                                            Sex::Unknown => "pedigree-node",
                                                        };
                                                        let is_sel = selected_person_id() == pid;
                                                        let node_class = if is_sel {
                                                            format!("{sex_class} selected")
                                                        } else {
                                                            sex_class.to_string()
                                                        };
                                                        let on_navigate = props.on_person_navigate;
                                                        rsx! {
                                                            div {
                                                                key: "anc-{gen_idx}-{slot_idx}",
                                                                class: "pedigree-slot-cell",
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
                                                                                    span { class: "pc-born", "✦ {birth_s}" }
                                                                                }
                                                                                if !death_s.is_empty() {
                                                                                    span { class: "pc-died", "✝ {death_s}" }
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    PedigreeSlot::Empty => {
                                                        let is_father = slot_idx % 2 == 0;
                                                        let child_idx = slot_idx / 2;
                                                        let child_pid = child_gen
                                                            .get(child_idx)
                                                            .and_then(|s| match s {
                                                                PedigreeSlot::Person(id) => Some(*id),
                                                                PedigreeSlot::Empty => None,
                                                            });
                                                        if let Some(child_id) = child_pid {
                                                            let on_empty = props.on_empty_slot;
                                                            rsx! {
                                                                div {
                                                                    key: "anc-{gen_idx}-{slot_idx}",
                                                                    class: "pedigree-slot-cell",
                                                                    style: style,
                                                                    button {
                                                                        class: "pedigree-node empty-slot",
                                                                        onclick: move |_| on_empty.call((child_id, is_father)),
                                                                        "+"
                                                                    }
                                                                }
                                                            }
                                                        } else {
                                                            rsx! {
                                                                div {
                                                                    key: "anc-{gen_idx}-{slot_idx}",
                                                                    class: "pedigree-slot-cell",
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

                                    // Connector bracket row
                                    div {
                                        class: "pedigree-connector-row",
                                        for group_i in 0..num_groups {
                                            {
                                                let md = marriage_dates[group_i].clone();
                                                rsx! {
                                                    div {
                                                        key: "conn-grp-{gen_idx}-{group_i}",
                                                        class: "connector-group",
                                                        style: "flex: {group_flex};",
                                                        if !md.is_empty() {
                                                            div { class: "pedigree-marriage-date", "♥ {md}" }
                                                        }
                                                        div { class: "connector-couple-bar" }
                                                        div { class: "connector-arms",
                                                            div { class: "connector-arm-left" }
                                                            div { class: "connector-arm-right" }
                                                        }
                                                        div { class: "connector-stem" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // ═══════════════════════
                        // ROOT ROW
                        // ═══════════════════════
                        {
                            let root_id = props.root_person_id;
                            let sex = props.data.sex_of(root_id);
                            let (given, surname, _) = props.data.name_parts(root_id);
                            let has_name = given.is_some() || surname.is_some();
                            let given_s = given.unwrap_or_default();
                            let surname_s = surname.unwrap_or_default();
                            let birth_s = props.data.birth_date(root_id).unwrap_or_default();
                            let death_s = props.data.death_date(root_id).unwrap_or_default();
                            let initials = make_initials(&given_s, &surname_s);
                            let sex_class = match sex {
                                Sex::Male    => "pedigree-node male current",
                                Sex::Female  => "pedigree-node female current",
                                Sex::Unknown => "pedigree-node current",
                            };
                            let on_click = props.on_person_click;
                            let root_flex = 1usize << deepest_anc;
                            rsx! {
                                div {
                                    class: "pedigree-gen-row",
                                    div {
                                        class: "pedigree-slot-cell",
                                        style: "flex: {root_flex};",
                                        div { class: "pedigree-root-wrapper",
                                            div {
                                                class: sex_class,
                                                onclick: move |_| selected_person_id.set(root_id),
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
                                                                span { class: "pc-born", "✦ {birth_s}" }
                                                            }
                                                            if !death_s.is_empty() {
                                                                span { class: "pc-died", "✝ {death_s}" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            button {
                                                class: "pedigree-edit-fab",
                                                title: "Edit / actions",
                                                onclick: move |evt: Event<MouseData>| {
                                                    evt.stop_propagation();
                                                    let coords = evt.client_coordinates();
                                                    on_click.call((root_id, coords.x, coords.y));
                                                },
                                                "✎"
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // ═══════════════════════════════════════════
                        // DESCENDANT ROWS — children below root
                        // ═══════════════════════════════════════════
                        for (desc_idx, desc_gen) in desc_gens.iter().enumerate() {
                            {
                                let families = desc_gen.families.clone();
                                rsx! {
                                    // Family blocks row: each family renders its own
                                    // connector bracket + children cards as a single unit
                                    div {
                                        key: "desc-gen-{desc_idx}",
                                        class: "pedigree-desc-gen",

                                        for (fi, family) in families.iter().enumerate() {
                                            {
                                                let children = family.children.clone();
                                                let child_count = children.len();
                                                let md = props.data.marriage_date_for_family(family.family_id)
                                                    .unwrap_or_default();

                                                rsx! {
                                                    div {
                                                        key: "desc-fam-{desc_idx}-{fi}",
                                                        class: "desc-family-block",

                                                        // Stem down from parent above
                                                        div { class: "desc-stem-up" }

                                                        // Marriage date (optional)
                                                        if !md.is_empty() {
                                                            div { class: "pedigree-marriage-date", "♥ {md}" }
                                                        }

                                                        // Branching connector: horizontal bar + stems down to each child
                                                        if child_count > 1 {
                                                            div { class: "desc-branch",
                                                                for ci in 0..child_count {
                                                                    {
                                                                        let arm_class = if ci == 0 {
                                                                            "desc-arm desc-arm-first"
                                                                        } else if ci == child_count - 1 {
                                                                            "desc-arm desc-arm-last"
                                                                        } else {
                                                                            "desc-arm desc-arm-mid"
                                                                        };
                                                                        rsx! {
                                                                            div {
                                                                                key: "desc-arm-{desc_idx}-{fi}-{ci}",
                                                                                class: arm_class,
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        } else {
                                                            // Single child: just a vertical line
                                                            div { class: "desc-stem-up" }
                                                        }

                                                        // Children cards row
                                                        div { class: "desc-children",
                                                            for (ci, pid) in children.iter().enumerate() {
                                                                {
                                                                    let pid = *pid;
                                                                    let sex = props.data.sex_of(pid);
                                                                    let (given, surname, _) = props.data.name_parts(pid);
                                                                    let has_name = given.is_some() || surname.is_some();
                                                                    let given_s = given.unwrap_or_default();
                                                                    let surname_s = surname.unwrap_or_default();
                                                                    let birth_s = props.data.birth_date(pid).unwrap_or_default();
                                                                    let death_s = props.data.death_date(pid).unwrap_or_default();
                                                                    let initials = make_initials(&given_s, &surname_s);
                                                                    let sex_class = match sex {
                                                                        Sex::Male    => "pedigree-node male",
                                                                        Sex::Female  => "pedigree-node female",
                                                                        Sex::Unknown => "pedigree-node",
                                                                    };
                                                                    let is_sel = selected_person_id() == pid;
                                                                    let node_class = if is_sel {
                                                                        format!("{sex_class} selected")
                                                                    } else {
                                                                        sex_class.to_string()
                                                                    };
                                                                    let on_navigate = props.on_person_navigate;
                                                                    let has_parents = {
                                                                        let (f, m) = props.data.parents_of(pid);
                                                                        f.is_some() || m.is_some()
                                                                    };
                                                                    let on_empty = props.on_empty_slot;
                                                                    rsx! {
                                                                        div {
                                                                            key: "desc-child-{desc_idx}-{fi}-{ci}",
                                                                            class: "desc-child-cell",
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
                                                                                                span { class: "pc-born", "✦ {birth_s}" }
                                                                                            }
                                                                                            if !death_s.is_empty() {
                                                                                                span { class: "pc-died", "✝ {death_s}" }
                                                                                            }
                                                                                        }
                                                                                    }
                                                                                }
                                                                            }
                                                                            // Add-parent "+" button for persons without parents
                                                                            if !has_parents {
                                                                                button {
                                                                                    class: "desc-add-parent-btn",
                                                                                    title: "Add parents",
                                                                                    onclick: move |evt: Event<MouseData>| {
                                                                                        evt.stop_propagation();
                                                                                        on_empty.call((pid, true));
                                                                                    },
                                                                                    "+"
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
            }

            // ══════════════════════════════════
            // EVENT PANEL (selected person)
            // ══════════════════════════════════
            div { class: "ev-panel",
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
                        for evt in sel_events.iter() {
                            {
                                let (icon, ic_class, label) = event_ui(evt.event_type);
                                let date_s = evt.date_value.clone().unwrap_or_default();
                                let desc_s = evt.description.clone().unwrap_or_default();
                                rsx! {
                                    div { class: "ev-item",
                                        div { class: ic_class, "{icon}" }
                                        div { class: "ev-info",
                                            div { class: "ev-type", "{label}" }
                                            if !date_s.is_empty() {
                                                div { class: "ev-date", "{date_s}" }
                                            }
                                            if !desc_s.is_empty() {
                                                div { class: "ev-place", "{desc_s}" }
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
