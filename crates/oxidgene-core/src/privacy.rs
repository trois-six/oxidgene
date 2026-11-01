//! The privacy rules of a tree (`docs/privacy.md` §3–§4): how a record's
//! privacy resolves, and which persons are contemporary.
//!
//! Pure functions over the records, so that every reader applies the same
//! rule: the web build's `person_visibility` rows, refreshed with each
//! write, and the export of both builds, which applies it to the records it
//! reads (§8).
//!
//! A person is contemporary unless a death, burial or cremation is recorded
//! for them, or their *latest possible birth year* is at most the current
//! year minus the tree's threshold. That bound is the smallest of what their
//! own dates and their relatives' give (§4.2, §4.3); a person nothing bounds
//! is contemporary, so the rule fails closed.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use uuid::Uuid;

use crate::calendar;
use crate::enums::{
    Calendar, DateQualifier, EventType, ExclusionKind, Privacy, TreeDefaultPrivacy,
};
use crate::types::{Event, FamilyChild, FamilySpouse, Person};

/// The default of a tree's `contemporary_years`.
pub const DEFAULT_CONTEMPORARY_YEARS: i32 = 100;
/// The smallest `contemporary_years` a tree may set.
pub const MIN_CONTEMPORARY_YEARS: i32 = 50;
/// The largest `contemporary_years` a tree may set.
pub const MAX_CONTEMPORARY_YEARS: i32 = 150;

/// The minimum gap between two generations, in years: a descendant at depth
/// `d` born at the latest in `Y` places the person's birth at the latest in
/// `Y − d × GENERATION_GAP` (§4.3).
pub const GENERATION_GAP: i32 = 15;
/// How many generations of descendants the inference reads (§4.3).
pub const DESCENT_DEPTH: usize = 10;

/// What a dated marriage of the person proves: born at the latest this many
/// years before it (§4.3).
const MARRIAGE_AGE: i32 = 14;
/// How far an approximate date may stray from its year, each way (§4.2).
const APPROXIMATE_SLACK: i32 = 5;
/// The latest birth a parent's birth allows, in years after it (§4.3).
const PARENT_BIRTH_SPAN: i32 = 70;
/// The latest birth a parent's death allows, in years after it.
const PARENT_DEATH_SPAN: i32 = 1;
/// The latest birth a spouse's birth allows, in years after it.
const SPOUSE_BIRTH_SPAN: i32 = 50;
/// The latest birth a sibling's birth allows, in years after it.
const SIBLING_BIRTH_SPAN: i32 = 30;
/// The youngest age at which a parent's birth bounds the person's from
/// below (§4.3, earliest bounds).
const MIN_PARENT_AGE: i32 = 15;

/// The version of the derivation the stored `person_visibility` rows follow.
/// A change to what they hold — a margin, an evidence, the way a date reads —
/// bumps it, and every tree's rows are rebuilt (§4.5), as the projections
/// are for `PROJECTION_SCHEMA_VERSION`.
pub const VISIBILITY_RULE_VERSION: i32 = 1;

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// The years a recorded date allows, each end open when the date does not
/// bound it: `before 1900` has no earliest year, `after 1900` no latest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct YearSpan {
    pub earliest: Option<i32>,
    pub latest: Option<i32>,
}

impl YearSpan {
    /// The span `event`'s date allows: none for an event without a date
    /// OxidGene could read.
    #[must_use]
    pub fn of_event(event: &Event) -> Option<Self> {
        Self::of_date(
            event.date_sort,
            event.date_qualifier,
            event.date_value2.as_deref(),
            event.calendar,
        )
    }

    /// The span of a date as the event columns hold it (`docs/privacy.md`
    /// §4.2): its normalized Gregorian `date_sort`, its qualifier, a range's
    /// second value and the calendar it was written in.
    ///
    /// `date_sort` is the first Gregorian day of the period the date names,
    /// so a date written in another calendar may end in the next Gregorian
    /// year: its latest year is taken one later. A free-text phrase has no
    /// `date_sort`, and no span.
    #[must_use]
    pub fn of_date(
        date_sort: Option<NaiveDate>,
        qualifier: DateQualifier,
        value2: Option<&str>,
        calendar: Calendar,
    ) -> Option<Self> {
        let first = date_sort?.year();
        let first_latest = match calendar {
            Calendar::Gregorian => first,
            _ => first + 1,
        };
        let second = || value2.and_then(|value| latest_gregorian_year(value, calendar));
        let span = match qualifier {
            DateQualifier::Exact => Self {
                earliest: Some(first),
                latest: Some(first_latest),
            },
            DateQualifier::About
            | DateQualifier::Calculated
            | DateQualifier::Estimated
            | DateQualifier::Perhaps
            | DateQualifier::FromAge => Self {
                earliest: Some(first - APPROXIMATE_SLACK),
                latest: Some(first_latest + APPROXIMATE_SLACK),
            },
            DateQualifier::Before => Self {
                earliest: None,
                latest: Some(first - 1),
            },
            DateQualifier::After => Self {
                earliest: Some(first + 1),
                latest: None,
            },
            DateQualifier::Between | DateQualifier::Or => Self {
                earliest: Some(first),
                latest: second().map(|second| second.max(first_latest)),
            },
        };
        Some(span)
    }
}

/// The last Gregorian year a date value written in `calendar` may fall in:
/// the year its year ends in. The value's last number is its year (`1910`,
/// `12 MAR 1910`, `15 VEND 7`).
fn latest_gregorian_year(value: &str, calendar: Calendar) -> Option<i32> {
    let year: i32 = value
        .split_whitespace()
        .rev()
        .find_map(|token| token.parse().ok())?;
    if calendar == Calendar::Gregorian {
        return Some(year);
    }
    let next = if year == -1 { 1 } else { year + 1 };
    let last_day = calendar::to_jdn(calendar, next, 1, 1)? - 1;
    calendar::from_jdn(Calendar::Gregorian, last_day).map(|(year, ..)| year)
}

/// The smallest of two optional bounds.
fn min_bound(a: Option<i32>, b: Option<i32>) -> Option<i32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The largest of two optional bounds.
fn max_bound(a: Option<i32>, b: Option<i32>) -> Option<i32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

// ---------------------------------------------------------------------------
// One person's own evidence
// ---------------------------------------------------------------------------

/// What a person's own records say about their life, for the rule: the
/// facts every bound of §4 is computed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LifeEvidence {
    /// A death, burial or cremation is recorded, dated or not (§4.1).
    pub deceased: bool,
    /// The birth, else the baptism or christening, that gives a bound: its
    /// span (§4.2).
    pub birth: Option<YearSpan>,
    /// The latest possible birth year the person's other dated events give:
    /// the year of each at the latest, less 14 for their own marriage
    /// (§4.3).
    pub other_latest: Option<i32>,
    /// The latest year the person's death or burial may have happened.
    pub death_latest: Option<i32>,
}

/// Whether `kind` records a birth.
fn is_birth(kind: EventType) -> bool {
    kind == EventType::Birth
}

/// Whether `kind` records a baptism, which stands in for a missing birth.
fn is_baptism(kind: EventType) -> bool {
    matches!(kind, EventType::Baptism | EventType::Christening)
}

/// Whether `kind` records that the person died.
fn is_end_of_life(kind: EventType) -> bool {
    matches!(
        kind,
        EventType::Death | EventType::Burial | EventType::Cremation
    )
}

impl LifeEvidence {
    /// The evidence of a person's own events and of the family events of
    /// the unions they are a spouse in. Deleted events must not be given.
    ///
    /// Of several births (or baptisms) that disagree, the latest bound is
    /// kept, so contradictory records never make a person older than one of
    /// them says.
    pub fn from_events<'a>(
        own: impl IntoIterator<Item = &'a Event>,
        unions: impl IntoIterator<Item = &'a Event>,
    ) -> Self {
        let mut evidence = Self::default();
        let mut births: Option<YearSpan> = None;
        let mut baptisms: Option<YearSpan> = None;
        let keep_latest = |slot: &mut Option<YearSpan>, span: YearSpan| {
            if span.latest.is_none() {
                return;
            }
            *slot = Some(match *slot {
                None => span,
                Some(kept) if span.latest > kept.latest => span,
                Some(kept) => kept,
            });
        };
        for event in own {
            let span = YearSpan::of_event(event);
            if is_end_of_life(event.event_type) {
                evidence.deceased = true;
                let latest = span.and_then(|span| span.latest);
                evidence.death_latest = max_bound(evidence.death_latest, latest);
            }
            if is_birth(event.event_type) {
                if let Some(span) = span {
                    keep_latest(&mut births, span);
                }
            } else if is_baptism(event.event_type) {
                if let Some(span) = span {
                    keep_latest(&mut baptisms, span);
                }
            } else if let Some(latest) = span.and_then(|span| span.latest) {
                evidence.other_latest = min_bound(evidence.other_latest, Some(latest));
            }
        }
        for event in unions {
            let Some(latest) = YearSpan::of_event(event).and_then(|span| span.latest) else {
                continue;
            };
            let latest = if event.event_type == EventType::Marriage {
                latest - MARRIAGE_AGE
            } else {
                latest
            };
            evidence.other_latest = min_bound(evidence.other_latest, Some(latest));
        }
        evidence.birth = births.or(baptisms);
        evidence
    }

    /// The latest possible birth year of the birth, else the baptism.
    #[must_use]
    pub fn birth_latest(&self) -> Option<i32> {
        self.birth.and_then(|span| span.latest)
    }

    /// The bound the person's recorded dates give as a descendant's
    /// evidence: the birth, else the baptism, else the other events (§4.3).
    #[must_use]
    pub fn recorded_latest(&self) -> Option<i32> {
        self.birth_latest().or(self.other_latest)
    }

    /// The bound the person's own records give: the smallest of the birth
    /// (or baptism) and the other events.
    #[must_use]
    pub fn own_latest(&self) -> Option<i32> {
        min_bound(self.birth_latest(), self.other_latest)
    }
}

// ---------------------------------------------------------------------------
// Relatives
// ---------------------------------------------------------------------------

/// The latest possible birth year the person's own records and those of
/// their relatives one link away give (§4.3), descendants aside.
#[must_use]
pub fn kin_latest<'a>(
    own: &LifeEvidence,
    parents: impl IntoIterator<Item = &'a LifeEvidence>,
    spouses: impl IntoIterator<Item = &'a LifeEvidence>,
    siblings: impl IntoIterator<Item = &'a LifeEvidence>,
) -> Option<i32> {
    let mut bound = own.own_latest();
    for parent in parents {
        bound = min_bound(bound, parent.birth_latest().map(|y| y + PARENT_BIRTH_SPAN));
        bound = min_bound(bound, parent.death_latest.map(|y| y + PARENT_DEATH_SPAN));
    }
    for spouse in spouses {
        bound = min_bound(bound, spouse.birth_latest().map(|y| y + SPOUSE_BIRTH_SPAN));
    }
    for sibling in siblings {
        bound = min_bound(
            bound,
            sibling.birth_latest().map(|y| y + SIBLING_BIRTH_SPAN),
        );
    }
    bound
}

/// The earliest possible birth year: the person's own birth (or baptism),
/// and a parent's birth with the youngest parental age (§4.3), the largest
/// winning. It never shows a person old; it says why one is contemporary.
#[must_use]
pub fn kin_earliest<'a>(
    own: &LifeEvidence,
    parents: impl IntoIterator<Item = &'a LifeEvidence>,
) -> Option<i32> {
    let mut bound = own.birth.and_then(|span| span.earliest);
    for parent in parents {
        let from_parent = parent
            .birth
            .and_then(|span| span.earliest)
            .map(|y| y + MIN_PARENT_AGE);
        bound = max_bound(bound, from_parent);
    }
    bound
}

/// What a person's descendants say, by depth: `0` holds the bound their
/// children give, `k` the bound their descendants down to `k + 1`
/// generations give (§4.3). Kept per depth so that a change to one person
/// refreshes their ancestors from their children's stored values alone,
/// without walking every descendant again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Descent(pub [Option<i32>; DESCENT_DEPTH]);

impl Descent {
    /// What the person's descendants down to [`DESCENT_DEPTH`] generations
    /// give.
    #[must_use]
    pub fn bound(&self) -> Option<i32> {
        self.0[DESCENT_DEPTH - 1]
    }

    /// The descent of a person whose children each have the recorded bound
    /// and the descent given: a child's recorded date at depth 1, and their
    /// own descendants one generation further down, each a generation gap
    /// earlier.
    #[must_use]
    pub fn of_children<'a>(children: impl IntoIterator<Item = (Option<i32>, &'a Self)>) -> Self {
        let mut descent = Self::default();
        for (recorded, theirs) in children {
            let recorded = recorded.map(|y| y - GENERATION_GAP);
            for depth in 0..DESCENT_DEPTH {
                let deeper = match depth {
                    0 => None,
                    _ => theirs.0[depth - 1].map(|y| y - GENERATION_GAP),
                };
                descent.0[depth] = min_bound(descent.0[depth], min_bound(recorded, deeper));
            }
        }
        descent
    }

    /// The stored form: the bounds by depth, comma-separated, an empty field
    /// for none.
    #[must_use]
    pub fn encode(&self) -> String {
        self.0
            .iter()
            .map(|bound| bound.map(|y| y.to_string()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The descent [`Self::encode`] wrote; none for anything else, which a
    /// refresh then computes again.
    #[must_use]
    pub fn decode(text: &str) -> Option<Self> {
        let mut descent = Self::default();
        let mut fields = text.split(',');
        for slot in &mut descent.0 {
            let field = fields.next()?;
            *slot = match field {
                "" => None,
                year => Some(year.parse().ok()?),
            };
        }
        fields.next().is_none().then_some(descent)
    }
}

/// The stored inputs of the contemporary rule for one person
/// (`docs/data-model.md` §6.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PersonVisibility {
    pub privacy: Privacy,
    pub deceased: bool,
    /// The smallest bound of §4.2 and §4.3; none when nothing bounds it.
    pub latest_birth_year: Option<i32>,
    pub earliest_birth_year: Option<i32>,
    /// The bound the person's recorded dates give their ancestors.
    pub recorded_birth_year: Option<i32>,
    /// The bound of their own and their relatives' dates, descendants aside.
    pub kin_birth_year: Option<i32>,
    pub descent: Descent,
}

impl PersonVisibility {
    /// Set the descent, and the latest bound it takes part in.
    pub fn set_descent(&mut self, descent: Descent) {
        self.descent = descent;
        self.latest_birth_year = min_bound(self.kin_birth_year, descent.bound());
    }

    /// Whether the person is contemporary under `rule` in `current_year`
    /// (§4.1): no death recorded, and no bound showing them born at least
    /// the threshold ago. Always false when the tree turned the rule off.
    #[must_use]
    pub fn is_contemporary(&self, rule: ContemporaryRule, current_year: i32) -> bool {
        rule.applies(self.deceased, self.latest_birth_year, current_year)
    }
}

/// A tree's contemporary rule: whether it applies, and its threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContemporaryRule {
    pub enabled: bool,
    pub years: i32,
}

impl Default for ContemporaryRule {
    fn default() -> Self {
        Self {
            enabled: true,
            years: DEFAULT_CONTEMPORARY_YEARS,
        }
    }
}

impl ContemporaryRule {
    /// Whether a person with these stored inputs is contemporary in
    /// `current_year`: `NOT deceased AND (latest IS NULL OR latest >
    /// current_year - years)` (§4.5).
    #[must_use]
    pub fn applies(
        self,
        deceased: bool,
        latest_birth_year: Option<i32>,
        current_year: i32,
    ) -> bool {
        self.enabled
            && !deceased
            && latest_birth_year.is_none_or(|latest| latest > current_year - self.years)
    }
}

// ---------------------------------------------------------------------------
// A whole tree
// ---------------------------------------------------------------------------

/// The members of one family, as the rule reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FamilyMembers {
    pub spouses: Vec<Uuid>,
    pub children: Vec<Uuid>,
}

/// The relatives one link away from each person, from the families they
/// belong to: parents and siblings through the families they are a child
/// of, spouses and children through those they are a spouse in. A sibling
/// shares a family as a child, so half-siblings through another union are
/// not counted.
#[derive(Debug, Default)]
pub struct Kinship {
    pub parents: HashMap<Uuid, Vec<Uuid>>,
    pub children: HashMap<Uuid, Vec<Uuid>>,
    pub spouses: HashMap<Uuid, Vec<Uuid>>,
    pub siblings: HashMap<Uuid, Vec<Uuid>>,
}

impl Kinship {
    #[must_use]
    pub fn of_families<'a>(families: impl IntoIterator<Item = &'a FamilyMembers>) -> Self {
        let mut kin = Self::default();
        for family in families {
            for &child in &family.children {
                let parents = kin.parents.entry(child).or_default();
                parents.extend(family.spouses.iter().copied().filter(|&p| p != child));
                let siblings = kin.siblings.entry(child).or_default();
                siblings.extend(family.children.iter().copied().filter(|&c| c != child));
            }
            for &spouse in &family.spouses {
                let children = kin.children.entry(spouse).or_default();
                children.extend(family.children.iter().copied().filter(|&c| c != spouse));
                let spouses = kin.spouses.entry(spouse).or_default();
                spouses.extend(family.spouses.iter().copied().filter(|&s| s != spouse));
            }
        }
        for relatives in [
            &mut kin.parents,
            &mut kin.children,
            &mut kin.spouses,
            &mut kin.siblings,
        ] {
            for ids in relatives.values_mut() {
                ids.sort_unstable();
                ids.dedup();
            }
        }
        kin
    }

    fn of(map: &HashMap<Uuid, Vec<Uuid>>, person: Uuid) -> &[Uuid] {
        map.get(&person).map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn parents_of(&self, person: Uuid) -> &[Uuid] {
        Self::of(&self.parents, person)
    }

    #[must_use]
    pub fn children_of(&self, person: Uuid) -> &[Uuid] {
        Self::of(&self.children, person)
    }

    #[must_use]
    pub fn spouses_of(&self, person: Uuid) -> &[Uuid] {
        Self::of(&self.spouses, person)
    }

    #[must_use]
    pub fn siblings_of(&self, person: Uuid) -> &[Uuid] {
        Self::of(&self.siblings, person)
    }
}

/// The visibility of a person from their evidence and their relatives':
/// everything but the descent, which [`derive_tree`] or an incremental
/// refresh sets.
#[must_use]
pub fn person_visibility(
    person: Uuid,
    privacy: Privacy,
    evidence: &HashMap<Uuid, LifeEvidence>,
    kin: &Kinship,
) -> PersonVisibility {
    let none = LifeEvidence::default();
    let own = evidence.get(&person).unwrap_or(&none);
    let of = |ids: &[Uuid]| -> Vec<&LifeEvidence> {
        ids.iter().filter_map(|id| evidence.get(id)).collect()
    };
    let parents = of(kin.parents_of(person));
    let kin_birth_year = kin_latest(
        own,
        parents.iter().copied(),
        of(kin.spouses_of(person)),
        of(kin.siblings_of(person)),
    );
    PersonVisibility {
        privacy,
        deceased: own.deceased,
        latest_birth_year: kin_birth_year,
        earliest_birth_year: kin_earliest(own, parents),
        recorded_birth_year: own.recorded_latest(),
        kin_birth_year,
        descent: Descent::default(),
    }
}

/// The visibility of every person of a tree, from each person's privacy and
/// evidence and the tree's families. Descendants are read down to
/// [`DESCENT_DEPTH`] generations, one depth at a time over every family
/// link, so a cycle the schema does not prevent cannot loop.
#[must_use]
pub fn derive_tree(
    persons: &[(Uuid, Privacy)],
    evidence: &HashMap<Uuid, LifeEvidence>,
    families: &[FamilyMembers],
) -> HashMap<Uuid, PersonVisibility> {
    let kin = Kinship::of_families(families);
    let mut rows: HashMap<Uuid, PersonVisibility> = persons
        .iter()
        .map(|&(id, privacy)| (id, person_visibility(id, privacy, evidence, &kin)))
        .collect();
    let live: HashSet<Uuid> = rows.keys().copied().collect();
    // Depth by depth: the descent at depth k reads the children's at k − 1.
    let mut descents: HashMap<Uuid, Descent> = HashMap::new();
    for _ in 0..DESCENT_DEPTH {
        let next: HashMap<Uuid, Descent> = live
            .iter()
            .map(|&person| {
                let none = Descent::default();
                let children = kin
                    .children_of(person)
                    .iter()
                    .filter(|child| live.contains(child))
                    .map(|child| {
                        (
                            rows[child].recorded_birth_year,
                            descents.get(child).unwrap_or(&none),
                        )
                    });
                (person, Descent::of_children(children))
            })
            .collect();
        descents = next;
    }
    for (person, descent) in descents {
        if let Some(row) = rows.get_mut(&person) {
            row.set_descent(descent);
        }
    }
    rows
}

/// The members of each family `spouses` and `children` name, persons
/// outside `live` left out: a deleted person links nobody.
#[must_use]
pub fn families_of(
    spouses: &[FamilySpouse],
    children: &[FamilyChild],
    live: &HashSet<Uuid>,
) -> Vec<FamilyMembers> {
    let mut families: HashMap<Uuid, FamilyMembers> = HashMap::new();
    for link in spouses.iter().filter(|l| live.contains(&l.person_id)) {
        let family = families.entry(link.family_id).or_default();
        family.spouses.push(link.person_id);
    }
    for link in children.iter().filter(|l| live.contains(&l.person_id)) {
        let family = families.entry(link.family_id).or_default();
        family.children.push(link.person_id);
    }
    families.into_values().collect()
}

/// The evidence of each person of `live` from `events`: their own events,
/// and the family events of the unions `spouses` makes them a spouse in.
#[must_use]
pub fn evidence_of(
    events: &[Event],
    spouses: &[FamilySpouse],
    live: &HashSet<Uuid>,
) -> HashMap<Uuid, LifeEvidence> {
    let mut own: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    let mut by_family: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    for event in events.iter().filter(|e| e.deleted_at.is_none()) {
        match (event.person_id, event.family_id) {
            (Some(person), _) => own.entry(person).or_default().push(event),
            (None, Some(family)) => by_family.entry(family).or_default().push(event),
            (None, None) => {}
        }
    }
    let mut unions: HashMap<Uuid, Vec<&Event>> = HashMap::new();
    for link in spouses {
        if let Some(events) = by_family.get(&link.family_id) {
            unions
                .entry(link.person_id)
                .or_default()
                .extend(events.iter().copied());
        }
    }
    live.iter()
        .map(|&person| {
            let evidence = LifeEvidence::from_events(
                own.get(&person).into_iter().flatten().copied(),
                unions.get(&person).into_iter().flatten().copied(),
            );
            (person, evidence)
        })
        .collect()
}

/// The visibility of every live person of a tree from its records: the
/// persons, their events, and the family links. What a full rebuild of the
/// stored rows and the export of both builds apply (§4.5, §8).
#[must_use]
pub fn derive_records(
    persons: &[Person],
    events: &[Event],
    spouses: &[FamilySpouse],
    children: &[FamilyChild],
) -> HashMap<Uuid, PersonVisibility> {
    let live_persons: Vec<(Uuid, Privacy)> = persons
        .iter()
        .filter(|p| p.deleted_at.is_none())
        .map(|p| (p.id, p.privacy))
        .collect();
    let live: HashSet<Uuid> = live_persons.iter().map(|(id, _)| *id).collect();
    let evidence = evidence_of(events, spouses, &live);
    derive_tree(
        &live_persons,
        &evidence,
        &families_of(spouses, children, &live),
    )
}

// ---------------------------------------------------------------------------
// Exclusions
// ---------------------------------------------------------------------------

/// The deepest an exclusion walks when it names no depth: the bound every
/// ancestry walk has, the schema not preventing a cycle.
pub const MAX_EXCLUSION_GENERATIONS: u32 = 64;

/// What one exclusion from sharing names (`docs/privacy.md` §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExclusionScope {
    pub kind: ExclusionKind,
    pub root: Uuid,
    /// Generations walked; all of them when none.
    pub depth: Option<u32>,
    /// The root itself, for an ancestry or a descendancy.
    pub include_root: bool,
    /// The spouses of the descendants, for a descendancy.
    pub include_spouses: bool,
}

impl ExclusionScope {
    /// The persons this exclusion covers among the live persons `kin`
    /// links: none when its root is not one of `live`, as for a deleted
    /// root, whose exclusion applies again once it is restored.
    #[must_use]
    pub fn persons(&self, kin: &Kinship, live: &HashSet<Uuid>) -> HashSet<Uuid> {
        let mut covered = HashSet::new();
        if !live.contains(&self.root) {
            return covered;
        }
        let depth = self.depth.unwrap_or(MAX_EXCLUSION_GENERATIONS);
        let step = |person: Uuid| -> &[Uuid] {
            match self.kind {
                ExclusionKind::Ancestors => kin.parents_of(person),
                _ => kin.children_of(person),
            }
        };
        if self.kind != ExclusionKind::Person {
            let mut seen = HashSet::from([self.root]);
            let mut generation = vec![self.root];
            for _ in 0..depth {
                generation = generation
                    .iter()
                    .flat_map(|&person| step(person).iter().copied())
                    .filter(|person| live.contains(person) && seen.insert(*person))
                    .collect();
                if generation.is_empty() {
                    break;
                }
                covered.extend(generation.iter().copied());
            }
        }
        if self.kind == ExclusionKind::Descendants && self.include_spouses {
            let spouses: Vec<Uuid> = covered
                .iter()
                .flat_map(|&person| kin.spouses_of(person).iter().copied())
                .filter(|person| live.contains(person))
                .collect();
            covered.extend(spouses);
        }
        if self.kind == ExclusionKind::Person || self.include_root {
            covered.insert(self.root);
        }
        covered
    }
}

// ---------------------------------------------------------------------------
// Precedence
// ---------------------------------------------------------------------------

/// Why a record is hidden from a viewer who is not its owner: the first rule
/// of §3.1 that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Withheld {
    /// Rule 1: excluded from sharing, hidden from everyone but the owner.
    Excluded,
    /// Rule 2: private, or following a private tree.
    Private,
    /// Rule 3: a contemporary person, whatever their own privacy.
    Contemporary,
    /// Outside the access scope the viewer is limited to (Authorization
    /// §6.5): never chosen by [`withheld`], which knows no scope.
    OutOfScope,
}

/// The first rule of §3.1 hiding a record from a Visitor, or none when it is
/// shown (rule 4). `contemporary` is false for a record that is not a
/// person.
#[must_use]
pub fn withheld(
    excluded: bool,
    privacy: Privacy,
    tree_default: TreeDefaultPrivacy,
    contemporary: bool,
) -> Option<Withheld> {
    if excluded {
        Some(Withheld::Excluded)
    } else if tree_default.resolve(privacy) == TreeDefaultPrivacy::Private {
        Some(Withheld::Private)
    } else if contemporary {
        Some(Withheld::Contemporary)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "privacy_tests.rs"]
mod tests;
