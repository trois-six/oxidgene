//! Horizontal pedigree chart component for the Geneanet-style tree view.
//!
//! Renders a 5-generation ancestor chart centered on a "root person", with
//! clickable [`PersonNode`] boxes and [`EmptySlot`] placeholders for unknown
//! ancestors. Right-clicking or long-pressing a person box opens a
//! [`ContextMenu`].

use std::collections::HashMap;

use dioxus::prelude::*;
use uuid::Uuid;

use oxidgene_core::Sex;
use oxidgene_core::types::{FamilyChild, FamilySpouse, Person, PersonName};

use crate::utils::{resolve_name, sex_icon_class, sex_symbol};

/// Maximum number of ancestor generations to display.
const MAX_GENERATIONS: usize = 5;

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

    /// Get parents of a person as (father_id, mother_id) by looking up the
    /// family where this person is a child and finding the spouses.
    fn parents_of(&self, person_id: Uuid) -> (Option<Uuid>, Option<Uuid>) {
        let Some(family_ids) = self.families_as_child.get(&person_id) else {
            return (None, None);
        };
        // Use the first family where this person is a child.
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
                        // Assign to first empty slot
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

/// A slot in the pedigree chart: either a known person or an empty placeholder.
#[derive(Clone, Debug)]
enum PedigreeSlot {
    Person(Uuid),
    Empty,
}

/// Build the ancestor slots for the pedigree chart.
/// Returns a Vec of generations, where each generation is a Vec of slots.
/// Generation 0 = root person, generation 1 = parents (2 slots), etc.
fn build_ancestor_slots(root_id: Uuid, data: &PedigreeData) -> Vec<Vec<PedigreeSlot>> {
    let mut generations: Vec<Vec<PedigreeSlot>> = Vec::new();

    // Generation 0: root person
    generations.push(vec![PedigreeSlot::Person(root_id)]);

    for _depth in 0..MAX_GENERATIONS {
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
    /// Called when the user clicks an empty slot (to add a parent).
    pub on_empty_slot: EventHandler<(Uuid, bool)>,
}

/// Horizontal 5-generation pedigree (ancestor) chart.
///
/// Renders as a set of generation columns flowing left-to-right. The root
/// person is in generation 0 (leftmost), parents in generation 1, etc.
#[component]
pub fn PedigreeChart(props: PedigreeChartProps) -> Element {
    let generations = build_ancestor_slots(props.root_person_id, &props.data);

    // Only render generations that have at least one person.
    let max_gen_idx = generations
        .iter()
        .rposition(|g| g.iter().any(|s| matches!(s, PedigreeSlot::Person(_))))
        .unwrap_or(0);

    // Limit to max_gen_idx + 1 generations (but show at least parents if root exists).
    let display_gens = (max_gen_idx + 1).max(2).min(generations.len());

    rsx! {
        div { class: "pedigree-chart",
            for (gen_idx, gen_slots) in generations.iter().take(display_gens).enumerate() {
                {
                    let gen_label = match gen_idx {
                        0 => "Root".to_string(),
                        1 => "Parents".to_string(),
                        2 => "Grandparents".to_string(),
                        3 => "Great-GP".to_string(),
                        4 => "2x Great-GP".to_string(),
                        5 => "3x Great-GP".to_string(),
                        n => format!("{n}x Great-GP"),
                    };
                    rsx! {
                        div { class: "pedigree-gen-col",
                            div { class: "pedigree-gen-label", "{gen_label}" }
                            div { class: "pedigree-gen-slots",
                                for (slot_idx, slot) in gen_slots.iter().enumerate() {
                                    {
                                        match slot {
                                            PedigreeSlot::Person(pid) => {
                                                let pid = *pid;
                                                let name = props.data.display_name(pid);
                                                let sex = props.data.sex_of(pid);
                                                let is_root = pid == props.root_person_id;
                                                let icon_class = format!("sex-icon {}", sex_icon_class(&sex));
                                                let symbol = sex_symbol(&sex);
                                                let node_class = if is_root {
                                                    "pedigree-node current"
                                                } else {
                                                    "pedigree-node"
                                                };
                                                let on_click = props.on_person_click;
                                                rsx! {
                                                    div {
                                                        class: "pedigree-slot",
                                                        div {
                                                            class: node_class,
                                                            onclick: move |evt: Event<MouseData>| {
                                                                let coords = evt.client_coordinates();
                                                                on_click.call((pid, coords.x, coords.y));
                                                            },
                                                            span { class: icon_class, "{symbol}" }
                                                            span { class: "pedigree-node-name", "{name}" }
                                                        }
                                                    }
                                                }
                                            },
                                            PedigreeSlot::Empty => {
                                                // Determine which parent slot this is.
                                                // Even slots (0, 2, 4...) = father, odd = mother.
                                                let is_father = slot_idx % 2 == 0;
                                                // Find the child this slot belongs to (from previous generation).
                                                let child_idx = slot_idx / 2;
                                                let parent_gen = if gen_idx > 0 { gen_idx - 1 } else { 0 };
                                                let child_person_id = if parent_gen < generations.len() {
                                                    if let Some(PedigreeSlot::Person(pid)) = generations[parent_gen].get(child_idx) {
                                                        Some(*pid)
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                };
                                                let label = "+";
                                                if let Some(child_id) = child_person_id {
                                                    let on_empty = props.on_empty_slot;
                                                    rsx! {
                                                        div { class: "pedigree-slot",
                                                            button {
                                                                class: "pedigree-node empty-slot",
                                                                onclick: move |_| on_empty.call((child_id, is_father)),
                                                                "{label}"
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    rsx! {
                                                        div { class: "pedigree-slot",
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
                }
            }
        }
    }
}
