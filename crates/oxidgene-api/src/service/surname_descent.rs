//! The descent of a family name, shared by REST and GraphQL: its bearers
//! grouped into branches, as the Dictionary's family-name row unfolds them
//! (`docs/ui-dictionary.md` §7.2).
//!
//! The rows are read in a fixed number of grouped queries
//! ([`DictionaryRepo::load_family_name_descent`]) and the whole forest is built
//! in memory on every request: a name's bearers are at most a few thousand,
//! and building it whole is what lets a page or an unfolded person be cut out
//! of the same forest every time. Only what an answer shows is written out.
//!
//! - A **bearer** carries the name on a name other than a married name. A
//!   married name never starts a branch: its carrier shows as the spouse of
//!   the bearer they married, or among the answer's `by_marriage` persons.
//! - A **root** is a bearer none of whose parents bears the name. Branches
//!   are ordered by the root's earliest date, undated roots last.
//! - Under each bearer, each union (by marriage date, undated last), then its
//!   bearing children (by birth date, undated last, then record order); the
//!   union's other children are counted on one `others` line.
//! - Each branch is grown generation by generation, so a person reachable
//!   twice — both parents bearing the name, a child of two families, a loop
//!   of parentage — is shown in full at the first place reached, branches in
//!   order, and anywhere else as a `reference` line. A bearer reachable from
//!   no root (only through such a loop) starts a branch of their own after
//!   the others. Nothing is visited twice, so nothing loops.
//! - An answer holds [`DESCENT_BRANCH_PAGE`] branches from `offset`, and
//!   unfolds them generation by generation across all of them until
//!   [`DESCENT_LINE_BUDGET`] lines; the bearers left are folded behind `more`
//!   lines, which ask for the same descent with the folded bearer's
//!   `person_id`. That answer always unfolds that bearer's own unions.

use std::collections::{HashMap, HashSet, VecDeque};

use chrono::NaiveDate;
use oxidgene_core::enums::{EventType, NameType, Sex, SpouseRole};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{
    DESCENT_BRANCH_PAGE, DESCENT_LINE_BUDGET, DescentBranch, DescentLine, DescentLineKind,
    DescentPerson, Event, PersonName, QualifiedYear, SurnameDescent, join_surname_particle,
};
use oxidgene_db::repo::{DescentRows, DictionaryRepo};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

use crate::profile::ProfileService;

/// What a descent is asked for.
#[derive(Debug, Clone, Default)]
pub struct DescentRequest {
    /// The family name, exactly as the dictionary lists it.
    pub value: String,
    /// The branches to skip, for the next page.
    pub offset: Option<u32>,
    /// A bearer whose folded descent to unfold, instead of a page.
    pub person_id: Option<Uuid>,
}

/// The descent of a family name of `tree_id`: a page of its branches, or the
/// descent of one of its bearers.
///
/// # Errors
///
/// [`OxidGeneError::Validation`] for a blank name, and
/// [`OxidGeneError::NotFound`] for a `person_id` that is not a bearer of it.
pub async fn descent(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    request: DescentRequest,
) -> Result<SurnameDescent, OxidGeneError> {
    let value = request.value.trim();
    if value.is_empty() {
        return Err(OxidGeneError::Validation(
            "a family name is required".to_string(),
        ));
    }
    let rows = DictionaryRepo::load_family_name_descent(db, tree_id, value).await?;
    let graph = Graph::new(rows);
    let shown: Vec<Uuid> = graph.shown().collect();
    let sosa = profiles.sosa_ancestors_among(tree_id, &shown).await?;
    let forest = Forest::build(&graph);
    let people = People {
        graph: &graph,
        sosa: &sosa,
    };
    forest.answer(&people, value, &request)
}

// ── The rows, indexed ───────────────────────────────────────────────────

/// A person's dates, as the lines show them.
#[derive(Debug, Clone, Default)]
struct Vitals {
    birth: Option<QualifiedYear>,
    /// The date children are ordered by: the birth's, or the baptism's.
    birth_date: Option<NaiveDate>,
    death: Option<QualifiedYear>,
    death_date: Option<NaiveDate>,
    deceased: bool,
}

/// A union of a bearer: the family and its other spouse.
#[derive(Debug, Clone, Copy)]
struct Union {
    family: Uuid,
    spouse: Option<Uuid>,
}

/// The descent's rows, indexed by person and by family.
struct Graph {
    bearers: HashSet<Uuid>,
    /// Carriers of the name only as a married name.
    married_only: Vec<Uuid>,
    /// Each bearer's unions, in order.
    unions: HashMap<Uuid, Vec<Union>>,
    /// Each family's live children, in order.
    children: HashMap<Uuid, Vec<Uuid>>,
    /// Each bearer's live parents.
    parents: HashMap<Uuid, Vec<Uuid>>,
    vitals: HashMap<Uuid, Vitals>,
    /// Each family's marriage: its year and the date it is ordered by.
    marriages: HashMap<Uuid, (Option<QualifiedYear>, Option<NaiveDate>)>,
    sexes: HashMap<Uuid, Sex>,
    /// The name each person is shown with.
    names: HashMap<Uuid, PersonName>,
}

impl Graph {
    fn new(rows: DescentRows) -> Self {
        let DescentRows {
            carrier_names,
            spouses,
            children,
            sexes,
            names,
            events,
        } = rows;
        let (bearers, married_only) = bearers(&carrier_names, &sexes);
        let marriages = marriages(&events);
        let vitals = vitals(&events);
        let spouses_of = spouses_by_family(&spouses, &sexes);
        let children = children_by_family(children, &sexes);
        let parents = parents_of(&bearers, &children, &spouses_of);
        let unions = unions_of(&bearers, &spouses, &spouses_of, &marriages);
        let names = shown_names(carrier_names, names);
        let children = ordered_children(children, &vitals);
        Self {
            bearers,
            married_only,
            unions,
            children,
            parents,
            vitals,
            marriages,
            sexes,
            names,
        }
    }

    /// Everybody a line can show: the carriers and their spouses.
    fn shown(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.names.keys().copied()
    }

    fn unions(&self, person: Uuid) -> &[Union] {
        self.unions.get(&person).map_or(&[], Vec::as_slice)
    }

    fn children(&self, family: Uuid) -> &[Uuid] {
        self.children.get(&family).map_or(&[], Vec::as_slice)
    }

    /// The person has a parent in the tree and none of them bears the name:
    /// where a root's `«` link leads.
    fn has_other_parents(&self, person: Uuid) -> bool {
        self.parents.get(&person).is_some_and(|p| !p.is_empty()) && !self.has_bearing_parent(person)
    }

    fn has_bearing_parent(&self, person: Uuid) -> bool {
        self.parents
            .get(&person)
            .is_some_and(|p| p.iter().any(|parent| self.bearers.contains(parent)))
    }

    /// The earliest date known of a root: their birth, death or a marriage.
    fn earliest(&self, person: Uuid) -> Option<NaiveDate> {
        let vitals = self.vitals.get(&person);
        let own = vitals
            .into_iter()
            .flat_map(|v| [v.birth_date, v.death_date]);
        let married = self
            .unions(person)
            .iter()
            .map(|u| self.marriages.get(&u.family).and_then(|(_, date)| *date));
        own.chain(married).flatten().min()
    }

    /// How roots are ordered: earliest date, undated last, then name.
    fn root_key(&self, person: Uuid) -> (bool, Option<NaiveDate>, String, Uuid) {
        let date = self.earliest(person);
        let name = self
            .names
            .get(&person)
            .and_then(|n| n.given_names.as_deref())
            .unwrap_or_default()
            .to_lowercase();
        (date.is_none(), date, name, person)
    }
}

/// The carriers by a name other than a married name, and the others.
fn bearers(names: &[PersonName], sexes: &HashMap<Uuid, Sex>) -> (HashSet<Uuid>, Vec<Uuid>) {
    let bearers: HashSet<Uuid> = names
        .iter()
        .filter(|n| n.name_type != NameType::Married && sexes.contains_key(&n.person_id))
        .map(|n| n.person_id)
        .collect();
    let mut married_only: Vec<Uuid> = names
        .iter()
        .map(|n| n.person_id)
        .filter(|id| !bearers.contains(id) && sexes.contains_key(id))
        .collect();
    married_only.sort_unstable();
    married_only.dedup();
    (bearers, married_only)
}

/// The year of a dated event and the date it is ordered by: its own date,
/// else the first of January of its year.
fn dated(event: &Event) -> Option<(QualifiedYear, NaiveDate)> {
    let year = event.qualified_year()?;
    let date = event
        .date_sort
        .or_else(|| NaiveDate::from_ymd_opt(year.year, 1, 1))?;
    Some((year, date))
}

/// The first dated event of `kind` among `events`, by date.
fn first_dated<'a>(
    events: impl Iterator<Item = &'a Event>,
    kind: EventType,
) -> Option<(QualifiedYear, NaiveDate)> {
    events
        .filter(|e| e.event_type == kind)
        .filter_map(|e| dated(e).map(|d| (d, e.id)))
        .min_by_key(|((_, date), id)| (*date, *id))
        .map(|(d, _)| d)
}

fn marriages(events: &[Event]) -> HashMap<Uuid, (Option<QualifiedYear>, Option<NaiveDate>)> {
    let mut by_family: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    for event in events
        .iter()
        .filter(|e| e.event_type == EventType::Marriage)
    {
        if let Some(family) = event.family_id {
            by_family.entry(family).or_default().push(event);
        }
    }
    by_family
        .into_iter()
        .map(|(family, events)| {
            let first = first_dated(events.into_iter(), EventType::Marriage);
            (family, (first.map(|(y, _)| y), first.map(|(_, d)| d)))
        })
        .collect()
}

/// Each person's dates: the birth, else the baptism; the death, else the
/// burial — each the first dated one.
fn vitals(events: &[Event]) -> HashMap<Uuid, Vitals> {
    let mut by_person: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    for event in events {
        if let Some(person) = event.person_id {
            by_person.entry(person).or_default().push(event);
        }
    }
    by_person
        .into_iter()
        .map(|(person, events)| {
            let pick = |first: EventType, then: EventType| {
                first_dated(events.iter().copied(), first)
                    .or_else(|| first_dated(events.iter().copied(), then))
            };
            let birth = pick(EventType::Birth, EventType::Baptism);
            let death = pick(EventType::Death, EventType::Burial);
            let vitals = Vitals {
                birth: birth.map(|(y, _)| y),
                birth_date: birth.map(|(_, d)| d),
                death: death.map(|(y, _)| y),
                death_date: death.map(|(_, d)| d),
                deceased: events
                    .iter()
                    .any(|e| matches!(e.event_type, EventType::Death | EventType::Burial)),
            };
            (person, vitals)
        })
        .collect()
}

/// Each family's live spouses, husband first, then wife, then partners.
fn spouses_by_family(
    spouses: &[oxidgene_core::types::FamilySpouse],
    sexes: &HashMap<Uuid, Sex>,
) -> HashMap<Uuid, Vec<Uuid>> {
    let mut by_family: HashMap<Uuid, Vec<(u8, i32, Uuid)>> = HashMap::new();
    for s in spouses.iter().filter(|s| sexes.contains_key(&s.person_id)) {
        let role = match s.role {
            SpouseRole::Husband => 0,
            SpouseRole::Wife => 1,
            SpouseRole::Partner => 2,
        };
        by_family
            .entry(s.family_id)
            .or_default()
            .push((role, s.sort_order, s.person_id));
    }
    by_family
        .into_iter()
        .map(|(family, mut members)| {
            members.sort_unstable();
            members.dedup_by_key(|(_, _, person)| *person);
            (family, members.into_iter().map(|(_, _, p)| p).collect())
        })
        .collect()
}

/// Each family's live child rows, once each: `(sort order, row, child)`.
fn children_by_family(
    children: Vec<oxidgene_core::types::FamilyChild>,
    sexes: &HashMap<Uuid, Sex>,
) -> HashMap<Uuid, Vec<(i32, Uuid, Uuid)>> {
    let mut by_family: HashMap<Uuid, Vec<(i32, Uuid, Uuid)>> = HashMap::new();
    let mut rows = HashSet::new();
    for c in children {
        if sexes.contains_key(&c.person_id) && rows.insert(c.id) {
            by_family
                .entry(c.family_id)
                .or_default()
                .push((c.sort_order, c.id, c.person_id));
        }
    }
    by_family
}

/// Each bearer's live parents: the spouses of the families they are a child
/// of.
fn parents_of(
    bearers: &HashSet<Uuid>,
    children: &HashMap<Uuid, Vec<(i32, Uuid, Uuid)>>,
    spouses_of: &HashMap<Uuid, Vec<Uuid>>,
) -> HashMap<Uuid, Vec<Uuid>> {
    let mut parents: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (family, kids) in children {
        let Some(spouses) = spouses_of.get(family) else {
            continue;
        };
        for (_, _, child) in kids.iter().filter(|(_, _, c)| bearers.contains(c)) {
            let entry = parents.entry(*child).or_default();
            entry.extend(spouses.iter().filter(|p| *p != child));
        }
    }
    parents
}

/// How a bearer's unions are ordered: undated last, by marriage date, by the
/// bearer's own family order, then by family.
type UnionKey = (bool, Option<NaiveDate>, i32, Uuid);

/// Each bearer's unions, by marriage date (undated last), then the order of
/// the bearer's families, then family.
fn unions_of(
    bearers: &HashSet<Uuid>,
    spouses: &[oxidgene_core::types::FamilySpouse],
    spouses_of: &HashMap<Uuid, Vec<Uuid>>,
    marriages: &HashMap<Uuid, (Option<QualifiedYear>, Option<NaiveDate>)>,
) -> HashMap<Uuid, Vec<Union>> {
    let mut keyed: HashMap<Uuid, Vec<UnionKey>> = HashMap::new();
    for s in spouses.iter().filter(|s| bearers.contains(&s.person_id)) {
        let date = marriages.get(&s.family_id).and_then(|(_, d)| *d);
        keyed.entry(s.person_id).or_default().push((
            date.is_none(),
            date,
            s.sort_order,
            s.family_id,
        ));
    }
    keyed
        .into_iter()
        .map(|(person, mut families)| {
            families.sort_unstable();
            families.dedup_by_key(|(_, _, _, family)| *family);
            let unions = families
                .into_iter()
                .map(|(_, _, _, family)| Union {
                    family,
                    spouse: spouses_of
                        .get(&family)
                        .and_then(|members| members.iter().find(|p| **p != person).copied()),
                })
                .collect();
            (person, unions)
        })
        .collect()
}

/// The name each person is shown with: a bearer's row of the family name
/// other than a married name (the primary one first, then in order), anyone
/// else's primary name (else their first) — a spouse who took the name by
/// marriage included. A bearer's row without given names takes their
/// primary name's.
fn shown_names(
    carrier_names: Vec<PersonName>,
    names: Vec<PersonName>,
) -> HashMap<Uuid, PersonName> {
    let bearing = carrier_names
        .into_iter()
        .filter(|n| n.name_type != NameType::Married)
        .collect();
    let key = |n: &PersonName| (!n.is_primary, n.sort_order);
    let mut shown = best_names(bearing, key);
    let primary = best_names(names, key);
    for (person, name) in primary {
        match shown.get_mut(&person) {
            Some(row) if trimmed(row.given_names.as_deref()).is_none() => {
                row.given_names = name.given_names;
            }
            Some(_) => {}
            None => {
                shown.insert(person, name);
            }
        }
    }
    shown
}

/// Each person's row of `names` with the lowest `key`.
fn best_names(
    names: Vec<PersonName>,
    key: impl Fn(&PersonName) -> (bool, i32),
) -> HashMap<Uuid, PersonName> {
    let mut best: HashMap<Uuid, PersonName> = HashMap::new();
    for n in names {
        if best
            .get(&n.person_id)
            .is_none_or(|kept| key(&n) < key(kept))
        {
            best.insert(n.person_id, n);
        }
    }
    best
}

/// Each family's children, once each: by birth date, undated last, then in
/// record order.
fn ordered_children(
    children: HashMap<Uuid, Vec<(i32, Uuid, Uuid)>>,
    vitals: &HashMap<Uuid, Vitals>,
) -> HashMap<Uuid, Vec<Uuid>> {
    children
        .into_iter()
        .map(|(family, mut kids)| {
            kids.sort_by_key(|(sort_order, row_id, child)| {
                let born = vitals.get(child).and_then(|v| v.birth_date);
                (born.is_none(), born, *sort_order, *row_id)
            });
            let mut seen = HashSet::new();
            let kids = kids
                .into_iter()
                .map(|(_, _, child)| child)
                .filter(|child| seen.insert(*child))
                .collect();
            (family, kids)
        })
        .collect()
}

// ── The persons a line shows ────────────────────────────────────────────

struct People<'a> {
    graph: &'a Graph,
    sosa: &'a HashSet<Uuid>,
}

impl People<'_> {
    fn person(&self, id: Uuid) -> DescentPerson {
        let name = self.graph.names.get(&id);
        let vitals = self.graph.vitals.get(&id).cloned().unwrap_or_default();
        DescentPerson {
            person_id: id,
            sex: self.graph.sexes.get(&id).copied().unwrap_or(Sex::Unknown),
            given_names: name.and_then(|n| trimmed(n.given_names.as_deref())),
            surname: name.and_then(|n| {
                trimmed(n.surname.as_deref())
                    .map(|root| join_surname_particle(n.surname_prefix.as_deref(), &root))
            }),
            birth: vitals.birth,
            death: vitals.death,
            deceased: vitals.deceased,
            sosa_ancestor: self.sosa.contains(&id),
        }
    }

    /// A line of `kind` about `person`, with `union` written on it.
    fn line(
        &self,
        kind: DescentLineKind,
        depth: u32,
        person: Uuid,
        union: Option<&Union>,
    ) -> DescentLine {
        let mut line = DescentLine::new(kind, depth);
        line.person = Some(self.person(person));
        if let Some(union) = union {
            line.marriage = self
                .graph
                .marriages
                .get(&union.family)
                .and_then(|(year, _)| *year);
            line.spouse = union.spouse.map(|spouse| self.person(spouse));
        }
        line
    }
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

// ── The forest ──────────────────────────────────────────────────────────

/// A child under a union: a bearer shown here, or one shown elsewhere.
#[derive(Debug, Clone, Copy)]
enum Child {
    Node(usize),
    Reference(Uuid),
}

#[derive(Debug, Clone)]
struct UnionNode {
    union: Union,
    children: Vec<Child>,
    /// Children not bearing the name.
    others: u32,
}

#[derive(Debug, Clone)]
struct Node {
    person: Uuid,
    parent: Option<usize>,
    /// The branch's number, from 1.
    branch: u32,
    unions: Vec<UnionNode>,
    /// The bearers of its descent shown here, itself included.
    size: u32,
}

impl Node {
    /// The lines unfolding it adds below its own: its next unions, its
    /// children and their `others` lines.
    fn cost(&self) -> usize {
        self.unions.len().saturating_sub(1)
            + self
                .unions
                .iter()
                .map(|u| u.children.len() + usize::from(u.others > 0))
                .sum::<usize>()
    }

    fn child_nodes(&self) -> impl Iterator<Item = usize> + '_ {
        self.unions
            .iter()
            .flat_map(|u| u.children.iter())
            .filter_map(|c| match c {
                Child::Node(i) => Some(*i),
                Child::Reference(_) => None,
            })
    }
}

/// Every branch of the name, each person placed once.
struct Forest<'a> {
    graph: &'a Graph,
    nodes: Vec<Node>,
    roots: Vec<usize>,
    placed: HashMap<Uuid, usize>,
}

/// One step of writing a branch's lines.
enum Work {
    Node(usize, u32),
    Line(Box<DescentLine>),
}

impl<'a> Forest<'a> {
    fn build(graph: &'a Graph) -> Self {
        let mut forest = Self {
            graph,
            nodes: Vec::new(),
            roots: Vec::new(),
            placed: HashMap::new(),
        };
        let mut bearers: Vec<Uuid> = graph.bearers.iter().copied().collect();
        bearers.sort_by_cached_key(|b| graph.root_key(*b));
        let (roots, rest): (Vec<Uuid>, Vec<Uuid>) = bearers
            .into_iter()
            .partition(|b| !graph.has_bearing_parent(*b));
        // The natural roots first; then whoever only a loop reaches.
        for root in roots.into_iter().chain(rest) {
            if !forest.placed.contains_key(&root) {
                forest.grow(root);
            }
        }
        for i in (0..forest.nodes.len()).rev() {
            forest.nodes[i].size += 1;
            if let Some(parent) = forest.nodes[i].parent {
                forest.nodes[parent].size += forest.nodes[i].size;
            }
        }
        forest
    }

    fn place(&mut self, person: Uuid, parent: Option<usize>, branch: u32) -> usize {
        let index = self.nodes.len();
        self.nodes.push(Node {
            person,
            parent,
            branch,
            unions: Vec::new(),
            size: 0,
        });
        self.placed.insert(person, index);
        index
    }

    /// The branch of `root`, generation by generation.
    fn grow(&mut self, root: Uuid) {
        let branch = u32::try_from(self.roots.len() + 1).unwrap_or(u32::MAX);
        let index = self.place(root, None, branch);
        self.roots.push(index);
        let mut queue = VecDeque::from([index]);
        while let Some(i) = queue.pop_front() {
            let person = self.nodes[i].person;
            for union in self.graph.unions(person) {
                let mut node = UnionNode {
                    union: *union,
                    children: Vec::new(),
                    others: 0,
                };
                for &child in self.graph.children(union.family) {
                    if !self.graph.bearers.contains(&child) {
                        node.others += 1;
                    } else if self.placed.contains_key(&child) {
                        node.children.push(Child::Reference(child));
                    } else {
                        let j = self.place(child, Some(i), branch);
                        node.children.push(Child::Node(j));
                        queue.push_back(j);
                    }
                }
                self.nodes[i].unions.push(node);
            }
        }
    }

    /// The bearers unfolded from `starts`, generation by generation across
    /// all of them, until the line budget; the first start always is when
    /// `force_first`.
    fn unfolded(&self, starts: &[usize], force_first: bool) -> HashSet<usize> {
        let mut unfolded = HashSet::new();
        let mut used = 0;
        let mut queue: VecDeque<usize> = starts.iter().copied().collect();
        while let Some(i) = queue.pop_front() {
            let cost = self.nodes[i].cost();
            if cost == 0 {
                continue;
            }
            let forced = force_first && unfolded.is_empty();
            if !forced && used + cost > DESCENT_LINE_BUDGET {
                // Past the budget, everything left stays folded, so the
                // view is cut at one generation rather than ragged.
                break;
            }
            unfolded.insert(i);
            used += cost;
            queue.extend(self.nodes[i].child_nodes());
        }
        unfolded
    }

    /// The lines of the branch from `start`, its depth 0.
    fn lines(&self, people: &People, start: usize, unfolded: &HashSet<usize>) -> Vec<DescentLine> {
        let mut lines = Vec::new();
        let mut stack = vec![Work::Node(start, 0)];
        while let Some(work) = stack.pop() {
            match work {
                Work::Line(line) => lines.push(*line),
                Work::Node(i, depth) => {
                    let node = &self.nodes[i];
                    let first = node.unions.first().map(|u| &u.union);
                    lines.push(people.line(DescentLineKind::Person, depth, node.person, first));
                    if node.cost() == 0 {
                        continue;
                    }
                    if !unfolded.contains(&i) {
                        let mut more = DescentLine::new(DescentLineKind::More, depth + 1);
                        more.count = node.size - 1;
                        more.unfold_person_id = Some(node.person);
                        lines.push(more);
                        continue;
                    }
                    let work = self.unfolding(people, node, depth);
                    stack.extend(work.into_iter().rev());
                }
            }
        }
        lines
    }

    /// What follows an unfolded bearer's line, in order.
    fn unfolding(&self, people: &People, node: &Node, depth: u32) -> Vec<Work> {
        let mut work = Vec::new();
        for (k, union) in node.unions.iter().enumerate() {
            if k > 0 {
                let line = people.line(
                    DescentLineKind::Union,
                    depth,
                    node.person,
                    Some(&union.union),
                );
                work.push(Work::Line(Box::new(line)));
            }
            for child in &union.children {
                work.push(match child {
                    Child::Node(j) => Work::Node(*j, depth + 1),
                    Child::Reference(person) => {
                        let mut line =
                            people.line(DescentLineKind::Reference, depth + 1, *person, None);
                        line.branch = self.placed.get(person).map(|i| self.nodes[*i].branch);
                        Work::Line(Box::new(line))
                    }
                });
            }
            if union.others > 0 {
                let mut line = DescentLine::new(DescentLineKind::Others, depth + 1);
                line.count = union.others;
                work.push(Work::Line(Box::new(line)));
            }
        }
        work
    }

    fn branch(&self, people: &People, start: usize, unfolded: &HashSet<usize>) -> DescentBranch {
        let node = &self.nodes[start];
        DescentBranch {
            number: node.branch,
            root_has_parents: self.graph.has_other_parents(node.person),
            person_count: node.size,
            lines: self.lines(people, start, unfolded),
        }
    }

    /// The answer to `request`: a page of branches, or one bearer's descent.
    fn answer(
        &self,
        people: &People,
        value: &str,
        request: &DescentRequest,
    ) -> Result<SurnameDescent, OxidGeneError> {
        let mut answer = SurnameDescent {
            value: value.to_string(),
            bearer_count: count(self.graph.bearers.len()),
            branch_count: count(self.roots.len()),
            offset: 0,
            branches: Vec::new(),
            by_marriage: Vec::new(),
        };
        if let Some(person) = request.person_id {
            let start = *self.placed.get(&person).ok_or(OxidGeneError::NotFound {
                entity: "Person",
                id: person,
            })?;
            let unfolded = self.unfolded(&[start], true);
            answer.branches.push(self.branch(people, start, &unfolded));
            return Ok(answer);
        }
        let offset = request.offset.unwrap_or(0) as usize;
        let page: Vec<usize> = self
            .roots
            .iter()
            .copied()
            .skip(offset)
            .take(DESCENT_BRANCH_PAGE)
            .collect();
        let unfolded = self.unfolded(&page, false);
        answer.offset = count(offset.min(self.roots.len()));
        answer.branches = page
            .into_iter()
            .map(|start| self.branch(people, start, &unfolded))
            .collect();
        if offset == 0 {
            answer.by_marriage = self.by_marriage(people);
        }
        Ok(answer)
    }

    /// The carriers by marriage only that no union line shows as a spouse.
    fn by_marriage(&self, people: &People) -> Vec<DescentPerson> {
        let spouses: HashSet<Uuid> = self
            .graph
            .unions
            .values()
            .flatten()
            .filter_map(|u| u.spouse)
            .collect();
        let mut persons: Vec<DescentPerson> = self
            .graph
            .married_only
            .iter()
            .filter(|id| !spouses.contains(id))
            .map(|id| people.person(*id))
            .collect();
        persons.sort_by_cached_key(|p| {
            (
                p.given_names.as_deref().unwrap_or_default().to_lowercase(),
                p.person_id,
            )
        });
        persons
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "surname_descent_tests.rs"]
mod tests;
