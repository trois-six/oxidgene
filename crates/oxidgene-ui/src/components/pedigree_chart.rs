//! Vertical bidirectional pedigree chart with pan/zoom.
//!
//! Renders a bidirectional tree:
//! - Ancestors grow upward from the root (0–6 levels, default 4)
//! - Descendants grow downward from the root (0–6 levels, default 3)
//! - The root person is in the middle
//! - A floating control panel (left side) for depth/zoom controls
//! - The chart is pannable via pointer drag and zoomable via mouse wheel

use std::collections::HashMap;

use dioxus::html::geometry::WheelDelta;
use dioxus::prelude::*;
use uuid::Uuid;

use oxidgene_core::Sex;
use oxidgene_core::types::{FamilyChild, FamilySpouse, Person, PersonName};

use crate::utils::{resolve_name, sex_icon_class, sex_symbol};

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

        Self {
            persons: persons_map,
            names,
            spouses_by_family,
            children_by_family,
            families_as_child,
            families_as_spouse,
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

    /// Get children of a person (union across all families they're a spouse in).
    fn children_of(&self, person_id: Uuid) -> Vec<Uuid> {
        let Some(family_ids) = self.families_as_spouse.get(&person_id) else {
            return vec![];
        };
        let mut children = Vec::new();
        for fid in family_ids {
            if let Some(fam_children) = self.children_by_family.get(fid) {
                for child in fam_children {
                    if !children.contains(&child.person_id) {
                        children.push(child.person_id);
                    }
                }
            }
        }
        children
    }

    /// Get the display name of a person.
    fn display_name(&self, person_id: Uuid) -> String {
        resolve_name(person_id, &self.names)
    }

    /// Get the sex of a person.
    fn sex_of(&self, person_id: Uuid) -> Sex {
        self.persons
            .get(&person_id)
            .map(|p| p.sex)
            .unwrap_or(Sex::Unknown)
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

/// Build descendant rows (one Vec<Uuid> per generation, starting from root's children).
fn build_descendant_levels(
    root_id: Uuid,
    data: &PedigreeData,
    max_levels: usize,
) -> Vec<Vec<Uuid>> {
    if max_levels == 0 {
        return vec![];
    }
    let mut result = Vec::new();
    let mut current = vec![root_id];

    for _ in 0..max_levels {
        let mut next = Vec::new();
        for &pid in &current {
            next.extend(data.children_of(pid));
        }
        if next.is_empty() {
            break;
        }
        result.push(next.clone());
        current = next;
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
    /// Called when the user clicks on a person box (to open context menu).
    pub on_person_click: EventHandler<(Uuid, f64, f64)>,
    /// Called when the user clicks an empty ancestor slot (to add a parent).
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
}

/// Vertical bidirectional pedigree chart with pan and zoom.
///
/// Ancestors appear above the root (deepest generation at the top), descendants
/// below. The user controls depth via a floating panel; the entire chart can be
/// panned by dragging and zoomed with the scroll wheel.
#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    // ── Depth controls ──
    let mut ancestor_levels = use_signal(|| 4usize);
    let mut descendant_levels = use_signal(|| 3usize);

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

    // ── Build ancestor tree ──
    let anc_gens = build_ancestor_slots(
        props.root_person_id,
        &props.data,
        ancestor_levels(),
    );

    // Trim to deepest generation that has at least one real person.
    let max_anc_gen_idx = anc_gens
        .iter()
        .rposition(|g| g.iter().any(|s| matches!(s, PedigreeSlot::Person(_))))
        .unwrap_or(0);

    // Show at least the parent row (gen 1) when ancestor_levels > 0,
    // so the user always sees "+" slots to add parents.
    let display_anc_gens = (max_anc_gen_idx + 1)
        .min(anc_gens.len())
        .max(if ancestor_levels() > 0 { 2 } else { 1 });

    // Index of the deepest visible ancestor generation (drives flex sizing).
    let deepest_anc = display_anc_gens.saturating_sub(1);

    // ── Build descendant rows ──
    let desc_gens = build_descendant_levels(
        props.root_person_id,
        &props.data,
        descendant_levels(),
    );

    // ── CSS transform for pan/zoom ──
    let transform = format!(
        "translate({}px, {}px) scale({})",
        offset_x(),
        offset_y(),
        scale()
    );
    let zoom_pct = (scale() * 100.0) as u32;

    rsx! {
        div {
            class: "pedigree-viewport",

            // ── Pointer events for panning ──
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

            // ── Mouse wheel for zooming ──
            onwheel: move |evt| {
                let delta_y = match evt.delta() {
                    WheelDelta::Lines(l) => l.y * 20.0,
                    WheelDelta::Pixels(p) => p.y,
                    WheelDelta::Pages(p) => p.y * 400.0,
                };
                let factor = if delta_y > 0.0 { 0.9 } else { 1.0 / 0.9 };
                scale.set((scale() * factor).clamp(0.3, 2.0));
            },

            // ── Pannable / zoomable inner content ──
            div {
                class: "pedigree-inner",
                style: "transform: {transform};",

                div {
                    class: "pedigree-tree",

                    // ══════════════════════════════════════════
                    // ANCESTOR ROWS — deepest generation at top
                    // ══════════════════════════════════════════
                    for gen_idx in (1..display_anc_gens).rev() {
                        {
                            // Clone what we need from borrowed data before entering rsx!
                            let gen_slots: Vec<PedigreeSlot> = anc_gens[gen_idx].clone();
                            let child_gen: Vec<PedigreeSlot> = anc_gens[gen_idx - 1].clone();
                            // Flex width: deepest gen has flex 1, shallower gens are wider.
                            let slot_flex = 1usize << deepest_anc.saturating_sub(gen_idx);
                            // Connector bracket groups and their flex width.
                            let num_groups = 1usize << (gen_idx - 1);
                            let group_flex = 1usize << deepest_anc.saturating_sub(gen_idx - 1);

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
                                                    let name = props.data.display_name(pid);
                                                    let sex = props.data.sex_of(pid);
                                                    let icon_class = format!("sex-icon {}", sex_icon_class(&sex));
                                                    let symbol = sex_symbol(&sex);
                                                    let on_click = props.on_person_click;
                                                    rsx! {
                                                        div {
                                                            key: "anc-{gen_idx}-{slot_idx}",
                                                            class: "pedigree-slot-cell",
                                                            style: style,
                                                            div {
                                                                class: "pedigree-node",
                                                                onclick: move |evt: Event<MouseData>| {
                                                                    let coords = evt.client_coordinates();
                                                                    on_click.call((pid, coords.x, coords.y));
                                                                },
                                                                span { class: icon_class, "{symbol}" }
                                                                span { class: "pedigree-node-name", "{name}" }
                                                            }
                                                        }
                                                    }
                                                }
                                                PedigreeSlot::Empty => {
                                                    // Determine which parent slot this is.
                                                    // Even slots = father, odd = mother.
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

                                // Connector row: bracket lines from this gen down to gen_idx-1
                                div {
                                    class: "pedigree-connector-row",
                                    for group_i in 0..num_groups {
                                        div {
                                            key: "conn-grp-{gen_idx}-{group_i}",
                                            class: "connector-group",
                                            style: "flex: {group_flex};",
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

                    // ══════════════════════════════════
                    // ROOT ROW
                    // ══════════════════════════════════
                    {
                        let root_id = props.root_person_id;
                        let name = props.data.display_name(root_id);
                        let sex = props.data.sex_of(root_id);
                        let icon_class = format!("sex-icon {}", sex_icon_class(&sex));
                        let symbol = sex_symbol(&sex);
                        let on_click = props.on_person_click;
                        let root_flex = 1usize << deepest_anc;
                        rsx! {
                            div {
                                class: "pedigree-gen-row",
                                div {
                                    class: "pedigree-slot-cell",
                                    style: "flex: {root_flex};",
                                    div {
                                        class: "pedigree-node current",
                                        onclick: move |evt: Event<MouseData>| {
                                            let coords = evt.client_coordinates();
                                            on_click.call((root_id, coords.x, coords.y));
                                        },
                                        span { class: icon_class, "{symbol}" }
                                        span { class: "pedigree-node-name", "{name}" }
                                    }
                                }
                            }
                        }
                    }

                    // ══════════════════════════════════════════
                    // DESCENDANT ROWS — children below root
                    // ══════════════════════════════════════════
                    for (desc_idx, desc_row) in desc_gens.iter().enumerate() {
                        {
                            let desc_row = desc_row.clone();
                            rsx! {
                                // Vertical connector above this descendant row
                                div {
                                    key: "desc-conn-{desc_idx}",
                                    class: "pedigree-desc-connector",
                                }

                                // Descendant generation row
                                div {
                                    class: "pedigree-gen-row pedigree-desc-row",
                                    for (di, pid) in desc_row.iter().enumerate() {
                                        {
                                            let pid = *pid;
                                            let name = props.data.display_name(pid);
                                            let sex = props.data.sex_of(pid);
                                            let icon_class = format!("sex-icon {}", sex_icon_class(&sex));
                                            let symbol = sex_symbol(&sex);
                                            let on_click = props.on_person_click;
                                            rsx! {
                                                div {
                                                    key: "desc-{desc_idx}-{di}",
                                                    class: "pedigree-slot-cell",
                                                    style: "flex: 1;",
                                                    div {
                                                        class: "pedigree-node",
                                                        onclick: move |evt: Event<MouseData>| {
                                                            let coords = evt.client_coordinates();
                                                            on_click.call((pid, coords.x, coords.y));
                                                        },
                                                        span { class: icon_class, "{symbol}" }
                                                        span { class: "pedigree-node-name", "{name}" }
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
            // FLOATING CONTROL PANEL (left side)
            // ══════════════════════════════════
            div {
                class: "pedigree-controls",

                div { class: "pedigree-controls-title", "Tree Depth" }

                div { class: "pedigree-controls-row",
                    label { class: "pedigree-controls-label", "Ancestors" }
                    select {
                        class: "pedigree-controls-select",
                        oninput: move |e: Event<FormData>| {
                            if let Ok(v) = e.value().parse::<usize>() {
                                ancestor_levels.set(v.min(6));
                            }
                        },
                        for lvl in 0..=6usize {
                            option {
                                value: "{lvl}",
                                selected: ancestor_levels() == lvl,
                                "{lvl}"
                            }
                        }
                    }
                }

                div { class: "pedigree-controls-row",
                    label { class: "pedigree-controls-label", "Descendants" }
                    select {
                        class: "pedigree-controls-select",
                        oninput: move |e: Event<FormData>| {
                            if let Ok(v) = e.value().parse::<usize>() {
                                descendant_levels.set(v.min(6));
                            }
                        },
                        for lvl in 0..=6usize {
                            option {
                                value: "{lvl}",
                                selected: descendant_levels() == lvl,
                                "{lvl}"
                            }
                        }
                    }
                }

                div { class: "pedigree-controls-divider" }

                div { class: "pedigree-controls-row pedigree-controls-zoom",
                    span { class: "pedigree-zoom-label", "Zoom: {zoom_pct}%" }
                    button {
                        class: "btn btn-outline btn-sm",
                        onclick: move |_| {
                            scale.set(1.0);
                            offset_x.set(0.0);
                            offset_y.set(0.0);
                        },
                        "Reset"
                    }
                }
            }
        }
    }
}
