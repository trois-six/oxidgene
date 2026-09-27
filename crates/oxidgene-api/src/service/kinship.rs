//! How two persons of a tree are related, shared by REST and GraphQL.
//!
//! The tree's family links are read in one query and walked in memory: a
//! relationship can run through any number of generations and branches, which
//! a recursive SQL walk per candidate ancestor would pay for in round trips.
//!
//! Blood relationships come first. Each one is a pair of lines descending
//! from a common ancestor — or from both spouses of a common ancestral couple
//! — to the two persons, sharing nobody but that ancestor: were they to meet
//! lower down, the person they met at would be a closer common ancestor, and
//! the relationship would be reported through them instead. Pedigree implex
//! is what makes several such pairs exist.
//!
//! Only when the two persons share no ancestor are paths through unions
//! searched, and then only the shortest ones.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use oxidgene_core::enums::SpouseRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Kinship, KinshipPath, KinshipSegment};
use oxidgene_db::repo::{AncestryRepo, FamilyLink, PersonRepo};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::profile::service::ProfileService;

/// The most paths one answer lists. Enough to show every relationship of a
/// tree with ordinary implex; a tree with more reports itself truncated.
pub const MAX_KINSHIP_PATHS: usize = 32;

/// The most lines enumerated from one ancestor down to one person. Heavy
/// implex multiplies them, and the closest are found first.
const MAX_LINES_PER_ANCESTOR: usize = 16;

/// The most shortest walks enumerated through unions before they are folded
/// into distinct paths.
const MAX_UNION_WALKS: usize = 256;

/// Hard ceiling on generations, as for the ancestry walks: a cycle in the
/// family links, which corrupt imports produce, must not recurse forever.
const MAX_GENERATIONS: usize = 64;

/// Every way found to go from `from` to `to` in `tree_id`.
///
/// # Errors
///
/// `NotFound` if either person is missing from the tree; `Validation` if both
/// are the same person.
pub async fn find_kinship(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    from: Uuid,
    to: Uuid,
) -> Result<Kinship, OxidGeneError> {
    if from == to {
        return Err(OxidGeneError::Validation(
            "a person has no relationship with themself".to_string(),
        ));
    }
    PersonRepo::get_in_tree(db, tree_id, from).await?;
    PersonRepo::get_in_tree(db, tree_id, to).await?;

    let links = AncestryRepo::family_links(db, tree_id).await?;
    let (paths, truncated) = FamilyGraph::new(&links).kinship(from, to);

    let mut named = vec![from, to];
    let mut seen: HashSet<Uuid> = named.iter().copied().collect();
    for segment in paths.iter().flat_map(|path| &path.segments) {
        for &id in segment
            .ancestor_ids
            .iter()
            .chain(&segment.from_line)
            .chain(&segment.to_line)
        {
            if seen.insert(id) {
                named.push(id);
            }
        }
    }
    let persons = profiles.search_entries(tree_id, &named).await?;

    Ok(Kinship {
        from_person_id: from,
        to_person_id: to,
        paths,
        truncated,
        persons,
    })
}

/// A step of a walk through unions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Step {
    Up,
    Down,
    Union(Uuid),
}

/// Where a walk through unions stands: on a person, and whether it has
/// started back down. A segment climbs, then descends; only a union lets it
/// climb again.
type WalkState = (Uuid, bool);

/// The family links of a tree, indexed both ways.
#[derive(Debug, Default)]
struct FamilyGraph {
    /// person → (family, parent)
    parents: HashMap<Uuid, Vec<(Uuid, Uuid)>>,
    /// person → (family, child)
    children: HashMap<Uuid, Vec<(Uuid, Uuid)>>,
    /// person → (family, spouse)
    spouses: HashMap<Uuid, Vec<(Uuid, Uuid)>>,
    /// family → its spouses, husband first
    couples: HashMap<Uuid, Vec<Uuid>>,
}

impl FamilyGraph {
    fn new(links: &[FamilyLink]) -> Self {
        // Sorted, so the same tree always yields the same paths in the same
        // order.
        let mut families: BTreeMap<Uuid, (Vec<&FamilyLink>, Vec<&FamilyLink>)> = BTreeMap::new();
        for link in links {
            let (spouses, children) = families.entry(link.family_id).or_default();
            if link.spouse_role.is_some() {
                spouses.push(link);
            } else {
                children.push(link);
            }
        }

        let mut graph = Self::default();
        for (family_id, (mut spouses, mut children)) in families {
            spouses.sort_by_key(|link| {
                let rank = match link.spouse_role {
                    Some(SpouseRole::Husband) => 0,
                    Some(SpouseRole::Wife) => 1,
                    _ => 2,
                };
                (rank, link.sort_order, link.person_id)
            });
            children.sort_by_key(|link| (link.sort_order, link.person_id));
            let mut couple: Vec<Uuid> = Vec::new();
            for link in spouses {
                if !couple.contains(&link.person_id) {
                    couple.push(link.person_id);
                }
            }
            let mut seen_children = HashSet::new();
            for child in children.iter().map(|link| link.person_id) {
                if !seen_children.insert(child) {
                    continue;
                }
                for &parent in couple.iter().filter(|&&parent| parent != child) {
                    graph
                        .parents
                        .entry(child)
                        .or_default()
                        .push((family_id, parent));
                    graph
                        .children
                        .entry(parent)
                        .or_default()
                        .push((family_id, child));
                }
            }
            for &one in &couple {
                for &other in couple.iter().filter(|&&other| other != one) {
                    graph
                        .spouses
                        .entry(one)
                        .or_default()
                        .push((family_id, other));
                }
            }
            graph.couples.insert(family_id, couple);
        }
        graph
    }

    /// The paths from `from` to `to`, closest first, and whether more exist.
    fn kinship(&self, from: Uuid, to: Uuid) -> (Vec<KinshipPath>, bool) {
        let (blood, truncated) = self.blood(from, to);
        if blood.is_empty() {
            self.through_unions(from, to)
        } else {
            (blood, truncated)
        }
    }

    // ── Blood ────────────────────────────────────────────────────────────

    /// `start` and each of its ancestors, at their shortest distance.
    fn ancestors(&self, start: Uuid) -> HashMap<Uuid, usize> {
        let mut distance = HashMap::from([(start, 0)]);
        let mut queue = VecDeque::from([start]);
        while let Some(person) = queue.pop_front() {
            let depth = distance[&person];
            if depth >= MAX_GENERATIONS {
                continue;
            }
            for &(_, parent) in self.parents.get(&person).into_iter().flatten() {
                if let Entry::Vacant(slot) = distance.entry(parent) {
                    slot.insert(depth + 1);
                    queue.push_back(parent);
                }
            }
        }
        distance
    }

    fn blood(&self, from: Uuid, to: Uuid) -> (Vec<KinshipPath>, bool) {
        let above_from = self.ancestors(from);
        let above_to = self.ancestors(to);

        let mut candidates: Vec<(usize, usize, Uuid)> = above_from
            .iter()
            .filter_map(|(&ancestor, &up)| {
                let down = *above_to.get(&ancestor)?;
                Some((up + down, up.max(down), ancestor))
            })
            .filter(|&(_, _, ancestor)| {
                self.lines_can_part(ancestor, from, &above_from, to, &above_to)
            })
            .collect();
        candidates.sort_unstable();

        let mut paths: Vec<KinshipPath> = Vec::new();
        let mut seen = HashSet::new();
        let mut truncated = false;
        'candidates: for (_, _, ancestor) in candidates {
            let (from_lines, from_cut) = self.lines_down(ancestor, from, &above_from);
            let (to_lines, to_cut) = self.lines_down(ancestor, to, &above_to);
            truncated |= from_cut || to_cut;
            for from_line in &from_lines {
                for to_line in &to_lines {
                    if from_line.iter().any(|person| to_line.contains(person)) {
                        continue;
                    }
                    let path = KinshipPath {
                        segments: vec![self.segment(
                            ancestor,
                            from_line.clone(),
                            to_line.clone(),
                            None,
                        )],
                    };
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    if paths.len() == MAX_KINSHIP_PATHS {
                        truncated = true;
                        break 'candidates;
                    }
                    paths.push(path);
                }
            }
        }
        paths.sort_by_key(|path| {
            let segment = &path.segments[0];
            let (up, down) = (segment.from_line.len(), segment.to_line.len());
            (up + down, up.max(down))
        });
        (paths, truncated)
    }

    /// Whether `ancestor` can be the top of a relationship: it must lead
    /// towards the two persons through two different children, or be one of
    /// them. Through a single child, every pair of lines would meet again at
    /// that child. This is what keeps the far ancestors of two siblings from
    /// being searched at all.
    fn lines_can_part(
        &self,
        ancestor: Uuid,
        from: Uuid,
        above_from: &HashMap<Uuid, usize>,
        to: Uuid,
        above_to: &HashMap<Uuid, usize>,
    ) -> bool {
        let towards = |target: Uuid, above: &HashMap<Uuid, usize>| -> Vec<Uuid> {
            if ancestor == target {
                return vec![ancestor];
            }
            self.children
                .get(&ancestor)
                .into_iter()
                .flatten()
                .map(|&(_, child)| child)
                .filter(|child| above.contains_key(child))
                .collect()
        };
        let from_side = towards(from, above_from);
        let to_side = towards(to, above_to);
        from_side
            .iter()
            .any(|one| to_side.iter().any(|other| one != other))
    }

    /// The lines from `top` down to `target`, each listed from `top`'s child
    /// down to `target`, the closest first; and whether some were left out.
    fn lines_down(
        &self,
        top: Uuid,
        target: Uuid,
        above: &HashMap<Uuid, usize>,
    ) -> (Vec<Vec<Uuid>>, bool) {
        if top == target {
            return (vec![Vec::new()], false);
        }
        let mut descent = Descent {
            graph: self,
            top,
            target,
            above,
            line: Vec::new(),
            lines: Vec::new(),
            cut: false,
        };
        descent.below(top);
        (descent.lines, descent.cut)
    }

    /// A segment topped by `top`, widened to the couple both lines descend
    /// from when they share one.
    fn segment(
        &self,
        top: Uuid,
        from_line: Vec<Uuid>,
        to_line: Vec<Uuid>,
        union_family_id: Option<Uuid>,
    ) -> KinshipSegment {
        let (ancestor_ids, family_id, half) = match (from_line.first(), to_line.first()) {
            (Some(one), Some(other)) => {
                let families_below = |child: &Uuid| -> Vec<Uuid> {
                    self.parents
                        .get(child)
                        .into_iter()
                        .flatten()
                        .filter(|&&(_, parent)| parent == top)
                        .map(|&(family, _)| family)
                        .collect()
                };
                let other_families = families_below(other);
                match families_below(one)
                    .into_iter()
                    .find(|family| other_families.contains(family))
                {
                    Some(family) => (self.couples[&family].clone(), Some(family), false),
                    None => (vec![top], None, true),
                }
            }
            _ => (vec![top], None, false),
        };
        KinshipSegment {
            ancestor_ids,
            family_id,
            from_line,
            to_line,
            half,
            union_family_id,
        }
    }

    // ── Through unions ───────────────────────────────────────────────────

    /// The shortest walks from `from` to `to` that may pass through unions,
    /// fewest unions first.
    fn through_unions(&self, from: Uuid, to: Uuid) -> (Vec<KinshipPath>, bool) {
        let start: WalkState = (from, false);
        let mut distance: HashMap<WalkState, usize> = HashMap::from([(start, 0)]);
        let mut previous: HashMap<WalkState, Vec<(WalkState, Step)>> = HashMap::new();
        let mut queue = VecDeque::from([start]);
        let mut reached_at = None;

        while let Some(state) = queue.pop_front() {
            let depth = distance[&state];
            // Finish the level the target was reached on, so every shortest
            // walk is recorded, and go no deeper.
            if reached_at.is_some_and(|reached| depth >= reached) {
                break;
            }
            for (next, step) in self.moves(state) {
                match distance.get(&next) {
                    None => {
                        distance.insert(next, depth + 1);
                        previous.insert(next, vec![(state, step)]);
                        if next.0 == to {
                            reached_at = Some(depth + 1);
                        } else {
                            queue.push_back(next);
                        }
                    }
                    Some(&known) if known == depth + 1 => {
                        previous.entry(next).or_default().push((state, step));
                    }
                    Some(_) => {}
                }
            }
        }
        if reached_at.is_none() {
            return (Vec::new(), false);
        }

        let mut walks: Vec<Vec<(Uuid, Step)>> = Vec::new();
        let mut truncated = false;
        for goal in [(to, false), (to, true)] {
            if distance.get(&goal) == reached_at.as_ref() {
                let mut tail = Vec::new();
                Self::unwind(
                    goal,
                    start,
                    &previous,
                    &mut tail,
                    &mut walks,
                    &mut truncated,
                );
            }
        }

        let mut paths: Vec<KinshipPath> = Vec::new();
        let mut seen = HashSet::new();
        for walk in walks {
            let path = self.fold_walk(from, &walk);
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        paths.sort_by_key(|path| path.segments.len());
        if paths.len() > MAX_KINSHIP_PATHS {
            paths.truncate(MAX_KINSHIP_PATHS);
            truncated = true;
        }
        (paths, truncated)
    }

    /// Where a walk can go next from `state`.
    fn moves(&self, (person, descending): WalkState) -> Vec<(WalkState, Step)> {
        let mut moves = Vec::new();
        if !descending {
            for &(_, parent) in self.parents.get(&person).into_iter().flatten() {
                moves.push(((parent, false), Step::Up));
            }
        }
        for &(_, child) in self.children.get(&person).into_iter().flatten() {
            moves.push(((child, true), Step::Down));
        }
        for &(family, spouse) in self.spouses.get(&person).into_iter().flatten() {
            moves.push(((spouse, false), Step::Union(family)));
        }
        moves
    }

    /// Every shortest walk ending at `state`, rebuilt backwards from the
    /// breadth-first search's predecessors.
    fn unwind(
        state: WalkState,
        start: WalkState,
        previous: &HashMap<WalkState, Vec<(WalkState, Step)>>,
        tail: &mut Vec<(Uuid, Step)>,
        walks: &mut Vec<Vec<(Uuid, Step)>>,
        truncated: &mut bool,
    ) {
        if state == start {
            if walks.len() == MAX_UNION_WALKS {
                *truncated = true;
            } else {
                walks.push(tail.iter().rev().copied().collect());
            }
            return;
        }
        for &(before, step) in previous.get(&state).into_iter().flatten() {
            if *truncated {
                return;
            }
            tail.push((state.0, step));
            Self::unwind(before, start, previous, tail, walks, truncated);
            tail.pop();
        }
    }

    /// Cut a walk into segments at its unions.
    fn fold_walk(&self, from: Uuid, walk: &[(Uuid, Step)]) -> KinshipPath {
        let mut segments = Vec::new();
        let mut persons = vec![from];
        let mut climbed = 0;
        let mut union = None;
        for &(person, step) in walk {
            match step {
                Step::Up => {
                    persons.push(person);
                    climbed += 1;
                }
                Step::Down => persons.push(person),
                Step::Union(family) => {
                    segments.push(self.segment_of(&persons, climbed, union));
                    persons = vec![person];
                    climbed = 0;
                    union = Some(family);
                }
            }
        }
        segments.push(self.segment_of(&persons, climbed, union));
        KinshipPath { segments }
    }

    /// The segment made of `persons`, the first `climbed` of which lead up
    /// to its top.
    fn segment_of(&self, persons: &[Uuid], climbed: usize, union: Option<Uuid>) -> KinshipSegment {
        let from_line = persons[..climbed].iter().rev().copied().collect();
        let to_line = persons[climbed + 1..].to_vec();
        self.segment(persons[climbed], from_line, to_line, union)
    }
}

/// A depth-first enumeration of the lines from one ancestor down to one
/// person.
struct Descent<'a> {
    graph: &'a FamilyGraph,
    top: Uuid,
    target: Uuid,
    /// The target and its ancestors: only they lead to it.
    above: &'a HashMap<Uuid, usize>,
    line: Vec<Uuid>,
    lines: Vec<Vec<Uuid>>,
    cut: bool,
}

impl Descent<'_> {
    fn below(&mut self, person: Uuid) {
        if self.cut || self.line.len() >= MAX_GENERATIONS {
            return;
        }
        // The nearest children first, so a truncated list keeps the shortest
        // lines.
        let mut next: Vec<(usize, Uuid)> = Vec::new();
        for &(_, child) in self.graph.children.get(&person).into_iter().flatten() {
            if let Some(&distance) = self.above.get(&child)
                && child != self.top
                && !self.line.contains(&child)
                && !next.iter().any(|&(_, seen)| seen == child)
            {
                next.push((distance, child));
            }
        }
        next.sort_by_key(|&(distance, _)| distance);

        for (_, child) in next {
            self.line.push(child);
            if child == self.target {
                if self.lines.len() == MAX_LINES_PER_ANCESTOR {
                    self.cut = true;
                } else {
                    self.lines.push(self.line.clone());
                }
            } else {
                self.below(child);
            }
            self.line.pop();
            if self.cut {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree built from families of (spouses, children), each person named
    /// by a small number.
    struct Fixture {
        links: Vec<FamilyLink>,
        next_family: u128,
    }

    fn p(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                links: Vec::new(),
                next_family: 10_000,
            }
        }

        fn family(&mut self, spouses: &[u128], children: &[u128]) -> Uuid {
            self.next_family += 1;
            let family_id = Uuid::from_u128(self.next_family);
            for (index, &spouse) in spouses.iter().enumerate() {
                self.links.push(FamilyLink {
                    family_id,
                    person_id: p(spouse),
                    spouse_role: Some(if index == 0 {
                        SpouseRole::Husband
                    } else {
                        SpouseRole::Wife
                    }),
                    sort_order: index as i32,
                });
            }
            for (index, &child) in children.iter().enumerate() {
                self.links.push(FamilyLink {
                    family_id,
                    person_id: p(child),
                    spouse_role: None,
                    sort_order: index as i32,
                });
            }
            family_id
        }

        fn kinship(&self, from: u128, to: u128) -> (Vec<KinshipPath>, bool) {
            FamilyGraph::new(&self.links).kinship(p(from), p(to))
        }
    }

    fn ids(persons: &[u128]) -> Vec<Uuid> {
        persons.iter().copied().map(p).collect()
    }

    /// Two first cousins: the grandparents' couple sits at the top, one line
    /// through each of their children.
    #[test]
    fn first_cousins_meet_at_their_grandparents_couple() {
        let mut tree = Fixture::new();
        let grandparents = tree.family(&[1, 2], &[3, 4]);
        tree.family(&[3, 5], &[7]);
        tree.family(&[6, 4], &[8]);

        let (paths, truncated) = tree.kinship(7, 8);

        assert!(!truncated);
        assert_eq!(paths.len(), 1, "the couple is one relationship, not two");
        let segment = &paths[0].segments[0];
        assert_eq!(segment.ancestor_ids, ids(&[1, 2]));
        assert_eq!(segment.family_id, Some(grandparents));
        assert_eq!(segment.from_line, ids(&[3, 7]));
        assert_eq!(segment.to_line, ids(&[4, 8]));
        assert!(!segment.half);
        assert_eq!(segment.union_family_id, None);
    }

    #[test]
    fn a_direct_ancestor_is_the_top_of_one_empty_line() {
        let mut tree = Fixture::new();
        tree.family(&[1, 2], &[3]);
        tree.family(&[3, 4], &[5]);

        let (paths, _) = tree.kinship(5, 1);
        assert_eq!(paths.len(), 1);
        let segment = &paths[0].segments[0];
        assert_eq!(segment.ancestor_ids, ids(&[1]));
        assert_eq!(segment.from_line, ids(&[3, 5]));
        assert!(segment.to_line.is_empty());

        // And the other way round.
        let (paths, _) = tree.kinship(1, 5);
        let segment = &paths[0].segments[0];
        assert!(segment.from_line.is_empty());
        assert_eq!(segment.to_line, ids(&[3, 5]));
    }

    #[test]
    fn half_siblings_share_one_parent_through_two_unions() {
        let mut tree = Fixture::new();
        tree.family(&[1, 2], &[3]);
        tree.family(&[1, 4], &[5]);

        let (paths, _) = tree.kinship(3, 5);
        assert_eq!(paths.len(), 1);
        let segment = &paths[0].segments[0];
        assert_eq!(segment.ancestor_ids, ids(&[1]));
        assert_eq!(segment.family_id, None);
        assert!(segment.half);
    }

    /// Siblings are related through their parents only: the grandparents are
    /// common ancestors too, but reached through the same child, so they are
    /// no separate relationship.
    #[test]
    fn a_farther_ancestor_reached_through_a_closer_one_is_not_repeated() {
        let mut tree = Fixture::new();
        tree.family(&[1, 2], &[3]);
        tree.family(&[3, 4], &[5, 6]);

        let (paths, _) = tree.kinship(5, 6);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].segments[0].ancestor_ids, ids(&[3, 4]));
    }

    /// Implex: the two persons descend from one couple through two different
    /// lines on each side, so they are related twice over.
    #[test]
    fn implex_gives_every_distinct_relationship_closest_first() {
        let mut tree = Fixture::new();
        // Ancestral couple 1+2 with children 3 and 4.
        tree.family(&[1, 2], &[3, 4]);
        // 3 and 4 each have a child; those two marry.
        tree.family(&[3, 11], &[5]);
        tree.family(&[12, 4], &[6]);
        tree.family(&[5, 6], &[7]);
        // 8 is another descendant of 3, one generation further down.
        tree.family(&[3, 13], &[9]);
        tree.family(&[9, 14], &[8]);

        let (paths, _) = tree.kinship(7, 8);

        let lines: Vec<(Vec<Uuid>, Vec<Uuid>)> = paths
            .iter()
            .map(|path| {
                let segment = &path.segments[0];
                (segment.from_line.clone(), segment.to_line.clone())
            })
            .collect();
        // 7 descends from 3 through their father 5, and is related to 8
        // through 3 alone (half: different unions of 3); and through their
        // mother 6, via the couple 1+2.
        assert_eq!(
            lines,
            vec![
                (ids(&[5, 7]), ids(&[9, 8])),
                (ids(&[4, 6, 7]), ids(&[3, 9, 8])),
            ]
        );
        assert!(paths[0].segments[0].half);
        assert_eq!(paths[0].segments[0].ancestor_ids, ids(&[3]));
        assert_eq!(paths[1].segments[0].ancestor_ids, ids(&[1, 2]));
    }

    #[test]
    fn with_no_common_ancestor_the_shortest_path_runs_through_a_union() {
        let mut tree = Fixture::new();
        // 3 is the child of 1+2; 3 marries 4, whose parents are 5+6; 7 is
        // 4's sibling.
        tree.family(&[1, 2], &[3]);
        let marriage = tree.family(&[3, 4], &[]);
        tree.family(&[5, 6], &[4, 7]);

        let (paths, truncated) = tree.kinship(1, 7);

        assert!(!truncated);
        assert_eq!(paths.len(), 1);
        let segments = &paths[0].segments;
        assert_eq!(segments.len(), 2);
        // 1 down to their child 3...
        assert_eq!(segments[0].ancestor_ids, ids(&[1]));
        assert_eq!(segments[0].to_line, ids(&[3]));
        // ...married to 4, sibling of 7.
        assert_eq!(segments[1].union_family_id, Some(marriage));
        assert_eq!(segments[1].ancestor_ids, ids(&[5, 6]));
        assert_eq!(segments[1].from_line, ids(&[4]));
        assert_eq!(segments[1].to_line, ids(&[7]));
        assert_eq!(segments[1].first_person(), Some(p(4)));
    }

    #[test]
    fn spouses_are_one_union_apart() {
        let mut tree = Fixture::new();
        tree.family(&[1, 2], &[]);

        let (paths, _) = tree.kinship(1, 2);
        assert_eq!(paths.len(), 1);
        let segments = &paths[0].segments;
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].ancestor_ids, ids(&[1]));
        assert_eq!(segments[1].ancestor_ids, ids(&[2]));
    }

    #[test]
    fn strangers_have_no_path() {
        let mut tree = Fixture::new();
        tree.family(&[1, 2], &[3]);
        tree.family(&[4, 5], &[6]);

        assert_eq!(tree.kinship(3, 6), (Vec::new(), false));
    }

    #[test]
    fn a_cycle_in_the_links_terminates() {
        let mut tree = Fixture::new();
        // Corrupt data: 1 is their own grandparent.
        tree.family(&[1], &[2]);
        tree.family(&[2], &[1, 3]);

        let (paths, _) = tree.kinship(1, 3);
        assert!(!paths.is_empty());
    }

    #[test]
    fn heavy_implex_is_cut_and_reported() {
        let mut tree = Fixture::new();
        // Ten generations where every couple is the parents of both members
        // of the next couple: the number of lines doubles each generation.
        let mut couple = (1u128, 2u128);
        for generation in 0..10u128 {
            let next = (100 + generation * 2, 101 + generation * 2);
            tree.family(&[couple.0, couple.1], &[next.0, next.1]);
            couple = next;
        }
        tree.family(&[couple.0, couple.1], &[1_000, 1_001]);

        let (paths, truncated) = tree.kinship(1_000, 1_001);
        assert!(!paths.is_empty());
        assert!(paths.len() <= MAX_KINSHIP_PATHS);
        assert_eq!(
            paths[0].segments[0].ancestor_ids,
            ids(&[couple.0, couple.1])
        );
        assert!(truncated);
    }
}
