//! The steps of a pedigree window's assembly that need no database: which
//! generation each person sits at, the nodes, edges, family events and
//! family memberships drawn from the projections, and which persons outside
//! the window still have to be fetched.
//!
//! [`super::ProfileService`] runs the queries between these steps.

use std::collections::HashMap;

use oxidgene_core::enums::ChildType;
use oxidgene_core::projection::{
    PedigreeEdge, PedigreeFamily, PedigreeFamilyMember, PedigreeNode, PersonProfile, ProfileEvent,
};
use oxidgene_core::types::AncestryLink;
use uuid::Uuid;

use super::builder::build_pedigree_node;

/// Each person's generation from `root`: negative for ancestors, positive for
/// descendants, 0 for the root.
///
/// The walk already reports each person at their shortest distance, but
/// someone can be both an ancestor and a descendant (implex), so the one
/// closest to the root wins between the two lists.
pub(super) fn generations(
    root: Uuid,
    ancestors: &[AncestryLink],
    descendants: &[AncestryLink],
) -> HashMap<Uuid, i32> {
    let mut depths = HashMap::from([(root, 0)]);
    let walked = ancestors
        .iter()
        .map(|a| (a.person_id, -a.depth))
        .chain(descendants.iter().map(|d| (d.person_id, d.depth)));
    for (person_id, generation) in walked {
        depths
            .entry(person_id)
            .and_modify(|existing: &mut i32| {
                if generation.abs() < existing.abs() {
                    *existing = generation;
                }
            })
            .or_insert(generation);
    }
    depths
}

/// The spouses of the window's persons that are not in it themselves: they
/// still need a node for display.
pub(super) fn spouses_outside(persons: &HashMap<Uuid, &PersonProfile>) -> Vec<Uuid> {
    let mut spouse_ids: Vec<Uuid> = Vec::new();
    for sid in persons
        .values()
        .flat_map(|p| &p.families_as_spouse)
        .filter_map(|link| link.spouse_id)
    {
        if !persons.contains_key(&sid) && !spouse_ids.contains(&sid) {
            spouse_ids.push(sid);
        }
    }
    spouse_ids
}

/// The generation of a spouse fetched from outside the window: their
/// partner's.
pub(super) fn partner_generation(spouse: &PersonProfile, depths: &HashMap<Uuid, i32>) -> i32 {
    spouse
        .families_as_spouse
        .iter()
        .filter_map(|link| link.spouse_id)
        .find_map(|sid| depths.get(&sid).copied())
        .unwrap_or(0)
}

/// A node for each of `person_ids` with a projection.
///
/// Sosa numbering depends on the path from the root, which ancestor
/// membership alone does not give, so only the root carries one here; the UI
/// derives the rest from the layout.
pub(super) fn nodes(
    person_ids: &[Uuid],
    persons: &HashMap<Uuid, &PersonProfile>,
    depths: &HashMap<Uuid, i32>,
    root: Uuid,
) -> HashMap<Uuid, PedigreeNode> {
    person_ids
        .iter()
        .filter_map(|pid| {
            let person = persons.get(pid)?;
            let generation = depths.get(pid).copied().unwrap_or(0);
            let sosa = (*pid == root).then_some(1);
            Some((*pid, build_pedigree_node(person, generation, sosa)))
        })
        .collect()
}

/// One edge per parent and child both in the window, from the parents'
/// family links.
pub(super) fn edges(
    persons: &HashMap<Uuid, &PersonProfile>,
    nodes: &HashMap<Uuid, PedigreeNode>,
) -> Vec<PedigreeEdge> {
    let mut edges: Vec<PedigreeEdge> = persons
        .values()
        .filter(|parent| nodes.contains_key(&parent.person_id))
        .flat_map(|parent| {
            parent.families_as_spouse.iter().flat_map(move |link| {
                link.children_ids
                    .iter()
                    .filter(|child_id| nodes.contains_key(child_id))
                    .map(move |&child_id| PedigreeEdge {
                        parent_id: parent.person_id,
                        child_id,
                        family_id: link.family_id,
                        edge_type: ChildType::Biological,
                    })
            })
        })
        .collect();
    // A child has two parents, and each spouse's link lists the child.
    edges.sort_by(|a, b| {
        a.parent_id
            .cmp(&b.parent_id)
            .then(a.child_id.cmp(&b.child_id))
    });
    edges.dedup_by(|a, b| a.parent_id == b.parent_id && a.child_id == b.child_id);
    edges
}

/// Each family's events, from the spouses' family links, once each.
pub(super) fn family_events(
    persons: &HashMap<Uuid, &PersonProfile>,
) -> HashMap<Uuid, Vec<ProfileEvent>> {
    let mut family_events: HashMap<Uuid, Vec<ProfileEvent>> = HashMap::new();
    for link in persons.values().flat_map(|p| &p.families_as_spouse) {
        if !link.events.is_empty() {
            family_events
                .entry(link.family_id)
                .or_default()
                .extend(link.events.iter().cloned());
        }
    }
    // Both spouses contribute the same events.
    for events in family_events.values_mut() {
        events.sort_by_key(|e| e.event_id);
        events.dedup_by_key(|e| e.event_id);
    }
    family_events
}

/// Record `person`'s families in `families`: as a spouse, with their
/// partner too when `with_partner`, and as a child, with both parents.
///
/// A spouse's link carries the family's authoritative, birth-order-sorted
/// children, so it replaces the list rather than extends it: families are
/// met in the order of a map, and a child seeded first from their own
/// `family_as_child` would otherwise stay ahead of elder siblings.
pub(super) fn record_membership(
    families: &mut HashMap<Uuid, PedigreeFamily>,
    person: &PersonProfile,
    with_partner: bool,
) {
    for link in &person.families_as_spouse {
        let family = families
            .entry(link.family_id)
            .or_insert_with(|| empty_family(link.family_id));
        let partner = link.spouse_id.filter(|_| with_partner);
        for spouse in std::iter::once(person.person_id).chain(partner) {
            push_new(&mut family.spouse_ids, spouse);
        }
        family.children_ids = link.children_ids.clone();
    }
    if let Some(child_link) = &person.family_as_child {
        let family = families
            .entry(child_link.family_id)
            .or_insert_with(|| empty_family(child_link.family_id));
        push_new(&mut family.children_ids, person.person_id);
        for parent in [child_link.father_id, child_link.mother_id]
            .into_iter()
            .flatten()
        {
            push_new(&mut family.spouse_ids, parent);
        }
    }
}

fn push_new(ids: &mut Vec<Uuid>, id: Uuid) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// An empty family unit, filled in as members are discovered.
pub(super) fn empty_family(family_id: Uuid) -> PedigreeFamily {
    PedigreeFamily {
        family_id,
        spouse_ids: Vec::new(),
        children_ids: Vec::new(),
        members: Vec::new(),
    }
}

/// For each family reached only as someone's parents, all of them outside
/// the window, one parent to fetch: their link lists the siblings, without
/// which only the person themselves would appear.
pub(super) fn parents_to_fetch(
    families: &HashMap<Uuid, PedigreeFamily>,
    persons: &HashMap<Uuid, &PersonProfile>,
) -> Vec<Uuid> {
    let mut parent_ids: Vec<Uuid> = Vec::new();
    for family in families.values() {
        let has_parent_in_window = family
            .spouse_ids
            .iter()
            .any(|sid| persons.contains_key(sid));
        if let Some(&pid) = family.spouse_ids.first()
            && !has_parent_in_window
        {
            push_new(&mut parent_ids, pid);
        }
    }
    parent_ids
}

/// Give each family the children list of a fetched parent's link to it.
///
/// That list is authoritative and birth-order-sorted, so it replaces the one
/// held: appending would leave whichever child was seeded first (the
/// pedigree root) stuck at index 0, scrambling sibling order for anyone but
/// the eldest.
pub(super) fn adopt_children_lists(
    families: &mut HashMap<Uuid, PedigreeFamily>,
    parents: &[PersonProfile],
) {
    let parents: HashMap<Uuid, &PersonProfile> = parents.iter().map(|p| (p.person_id, p)).collect();
    for family in families.values_mut() {
        let lists: Vec<Vec<Uuid>> = family
            .spouse_ids
            .iter()
            .filter_map(|sid| parents.get(sid))
            .filter_map(|parent| {
                parent
                    .families_as_spouse
                    .iter()
                    .find(|link| link.family_id == family.family_id)
            })
            .map(|link| link.children_ids.clone())
            .collect();
        if let Some(children) = lists.into_iter().last() {
            family.children_ids = children;
        }
    }
}

/// The children of the families that have no node of their own.
pub(super) fn members_outside(
    families: &HashMap<Uuid, PedigreeFamily>,
    nodes: &HashMap<Uuid, PedigreeNode>,
) -> Vec<Uuid> {
    let mut member_ids: Vec<Uuid> = Vec::new();
    for &cid in families.values().flat_map(|f| &f.children_ids) {
        if !nodes.contains_key(&cid) {
            push_new(&mut member_ids, cid);
        }
    }
    member_ids
}

/// Add the members fetched from outside the window to their families, for
/// the event panel, and their own families' spouses and children — by id
/// only, never fetched — for the hidden-relations indicator of their card.
pub(super) fn add_members_outside(
    families: &mut HashMap<Uuid, PedigreeFamily>,
    outside: &[PersonProfile],
) {
    let outside: HashMap<Uuid, &PersonProfile> = outside.iter().map(|p| (p.person_id, p)).collect();
    for family in families.values_mut() {
        let members: Vec<PedigreeFamilyMember> = family
            .children_ids
            .iter()
            .filter_map(|cid| outside.get(cid))
            .map(|person| family_member(person))
            .collect();
        family.members.extend(members);
    }
    for person in outside.values() {
        record_membership(families, person, true);
    }
}

fn family_member(person: &PersonProfile) -> PedigreeFamilyMember {
    let name = person.primary_name.as_ref();
    PedigreeFamilyMember {
        person_id: person.person_id,
        display_name: name.map(|n| n.display_name.clone()).unwrap_or_default(),
        given_names: name.and_then(|n| n.given_names.clone()),
        surname: name.and_then(|n| n.surname.clone()),
        sex: person.sex,
        birth: person.birth_or_baptism().cloned(),
        death: person.death_or_burial().cloned(),
    }
}
