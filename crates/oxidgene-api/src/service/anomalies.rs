//! Anomalies: dates, filiations, unions, witnesses and records that are
//! impossible or unlikely (`docs/ui-tools.md` §3, which holds the whole
//! catalogue with the rules still proposed).
//!
//! Computed on each request from the person projections and the tree's
//! witness links; nothing is stored. A rule fires only when it holds for
//! every day its dates may stand for (see [`Span`]), so an imprecise date
//! never raises an anomaly its precision could explain.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::search::normalize_for_search;
use oxidgene_core::types::{EventWitness, Place};
use oxidgene_core::{ChildType, DateQualifier, EventType, OxidGeneError, Sex, SpouseRole};
use serde::Serialize;
use uuid::Uuid;

use crate::reference::{PlaceLocation, ReferenceLang};
use crate::service::event_date;
use crate::service::statistics::{PersonRef, PlaceUsage, locate_used, person_ref};

// ── Thresholds (`docs/ui-tools.md` §3.2) ──────────────────────────────────

/// A life longer than this is flagged whenever it was lived.
pub const MAX_LIFESPAN_YEARS: f64 = 105.0;
/// A life longer than this is flagged when it began before
/// [`CENTENARIAN_BEFORE_YEAR`], when centenarians were rarer still.
pub const OLD_CENTENARIAN_YEARS: f64 = 100.0;
pub const CENTENARIAN_BEFORE_YEAR: i32 = 1900;
/// A parent younger than this at a child's birth.
pub const MIN_PARENT_AGE_YEARS: f64 = 11.0;
/// A father older than this at a child's birth.
pub const MAX_FATHER_AGE_YEARS: f64 = 70.0;
/// A mother older than this at a child's birth.
pub const MAX_MOTHER_AGE_YEARS: f64 = 55.0;
/// A child born more than this many days after the father's death: a
/// posthumous child comes within about nine months.
pub const MAX_POSTHUMOUS_BIRTH_DAYS: i64 = 300;
/// Two children of a union born this many days apart or more, but fewer
/// than [`MIN_SIBLING_GAP_DAYS`]; closer births are twins.
pub const TWIN_DAYS: i64 = 11;
/// Seven months.
pub const MIN_SIBLING_GAP_DAYS: i64 = 213;
/// Two consecutive children of a union born further apart than this.
pub const MAX_SIBLING_GAP_YEARS: f64 = 50.0;
/// A spouse younger than this at the union.
pub const MIN_UNION_AGE_YEARS: f64 = 12.0;
/// A spouse older than this at the union.
pub const MAX_UNION_AGE_YEARS: f64 = 100.0;
/// Spouses born further apart than this.
pub const MAX_SPOUSE_GAP_YEARS: f64 = 50.0;
/// How far a date about, calculated or estimated may stray, each way.
pub const APPROXIMATE_SLACK_DAYS: i64 = 2 * 365;
/// The most items a rule lists; its count still says how many there are.
pub const MAX_ITEMS_PER_RULE: usize = 500;

const DAYS_PER_YEAR: f64 = 365.2425;

/// Every implemented rule, in the order of the catalogue:
/// `(id, category, severity)`.
const RULES: &[(&str, &str, &str)] = &[
    ("death_before_birth", "dates", "error"),
    ("burial_before_death", "dates", "error"),
    ("event_before_birth", "dates", "error"),
    ("baptism_after_death", "dates", "warning"),
    ("event_after_death", "dates", "warning"),
    ("burial_not_last", "dates", "warning"),
    ("lived_over_105", "dates", "warning"),
    ("centenarian_before_1900", "dates", "warning"),
    ("future_date", "dates", "error"),
    ("parent_born_after_child", "filiation", "error"),
    ("ancestor_born_after_descendant", "filiation", "error"),
    ("own_ancestor", "filiation", "error"),
    ("parent_too_young", "filiation", "warning"),
    ("father_too_old", "filiation", "warning"),
    ("mother_too_old", "filiation", "warning"),
    ("born_after_mother_death", "filiation", "error"),
    ("born_long_after_father_death", "filiation", "error"),
    ("siblings_too_close", "filiation", "warning"),
    ("siblings_far_apart", "filiation", "warning"),
    ("union_before_birth", "unions", "error"),
    ("union_too_young", "unions", "warning"),
    ("union_over_100", "unions", "warning"),
    ("union_after_death", "unions", "error"),
    ("spouses_age_gap", "unions", "warning"),
    ("repeated_union", "unions", "warning"),
    ("homonymous_spouses", "unions", "warning"),
    ("union_with_parent_or_child", "unions", "error"),
    ("union_many_spouses", "unions", "warning"),
    ("witness_before_birth", "witnesses", "error"),
    ("witness_after_death", "witnesses", "error"),
    ("godparent_sex", "witnesses", "warning"),
    ("spouse_role_sex", "data_quality", "warning"),
    ("same_role_spouses", "data_quality", "warning"),
    ("unreadable_date", "data_quality", "warning"),
    ("reversed_range", "data_quality", "warning"),
    ("no_name", "data_quality", "warning"),
];

/// A tree's anomalies.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct TreeAnomalies {
    /// Persons examined.
    pub persons: i64,
    /// The rules that found something, in catalogue order.
    pub rules: Vec<AnomalyRule>,
}

/// What one rule found.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AnomalyRule {
    pub rule: String,
    /// `dates`, `filiation`, `unions`, `witnesses` or `data_quality`.
    pub category: String,
    /// `error` for the impossible, `warning` for the unlikely.
    pub severity: String,
    /// Everything found, of which `items` lists at most
    /// [`MAX_ITEMS_PER_RULE`].
    pub count: i64,
    pub items: Vec<Anomaly>,
}

/// One anomaly: the persons concerned, the subject first, and what the
/// rule measured.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct Anomaly {
    pub persons: Vec<PersonRef>,
    /// The union concerned, for the rules about one.
    pub family_id: Option<String>,
    /// The figure the rule measured: whole years for ages and gaps in
    /// years, days for gaps between births and deaths.
    pub value: Option<i64>,
    /// The event concerned, an `EventType` in its snake_case form.
    pub event_type: Option<String>,
    /// Recorded text the rule is about: an unreadable date, a relation.
    pub text: Option<String>,
}

impl Anomaly {
    fn of(persons: Vec<PersonRef>) -> Self {
        Self {
            persons,
            family_id: None,
            value: None,
            event_type: None,
            text: None,
        }
    }

    fn with_value(mut self, value: f64) -> Self {
        self.value = Some(value.floor() as i64);
        self
    }

    fn with_event(mut self, event_type: EventType) -> Self {
        self.event_type = Some(event_type.to_string());
        self
    }

    fn with_family(mut self, family_id: Uuid) -> Self {
        self.family_id = Some(family_id.to_string());
        self
    }

    fn with_text(mut self, text: &str) -> Self {
        self.text = Some(text.to_string());
        self
    }
}

/// Loads a tree's projections and witness links and finds its anomalies.
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    profiles: &crate::profile::ProfileService,
    tree_id: Uuid,
) -> Result<TreeAnomalies, OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let persons = profiles.get_all_persons(db, tree_id).await?;
    let witnesses = oxidgene_db::repo::EventWitnessRepo::list_by_tree(db, tree_id).await?;
    let today = chrono::Utc::now().date_naive();
    tokio::task::spawn_blocking(move || compute(&persons, &witnesses, today))
        .await
        .map_err(|e| OxidGeneError::Internal(e.to_string()))
}

/// Loads a tree's used places and returns those that cannot be located, most
/// used first: the places the statistics count as not located, by the same
/// rule ([`crate::service::statistics::locate_used`]).
pub async fn load_unlocated_places(
    db: &sea_orm::DatabaseConnection,
    tree_id: Uuid,
) -> Result<Vec<PlaceUsage>, OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let places = oxidgene_db::repo::DictionaryRepo::places_with_usage(db, tree_id).await?;
    tokio::task::spawn_blocking(move || {
        // The language only names countries and regions, which this list
        // does not show.
        unlocated_places(&places, |labels| {
            crate::reference::locate_places(ReferenceLang::En, labels)
        })
    })
    .await
    .map_err(|e| OxidGeneError::Internal(e.to_string()))
}

/// The used places `locate` cannot place and that carry no coordinates of
/// their own.
pub fn unlocated_places(
    places: &[(Place, i64)],
    locate: impl FnOnce(&[(&str, i64)]) -> Vec<PlaceLocation>,
) -> Vec<PlaceUsage> {
    let (_, _, usages) = locate_used(places, locate);
    usages
        .into_iter()
        .filter(|place| place.latitude.is_none())
        .collect()
}

// ── Dates ─────────────────────────────────────────────────────────────────

/// The days a recorded date may stand for: a year alone its whole year, a
/// month its whole month, widened by [`APPROXIMATE_SLACK_DAYS`] each way for
/// a date about, calculated or estimated. Dates before, after, perhaps, or
/// ranges, say too little to compare and have no span.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    from: NaiveDate,
    to: NaiveDate,
}

impl Span {
    fn of(event: Option<&ProfileEvent>) -> Option<Self> {
        let event = event?;
        let slack = match event.date_qualifier {
            DateQualifier::Exact => 0,
            DateQualifier::About | DateQualifier::Calculated | DateQualifier::Estimated => {
                APPROXIMATE_SLACK_DAYS
            }
            _ => return None,
        };
        let from = event.date_sort?;
        let parts = event
            .date_value
            .as_deref()
            .map_or(0, |v| v.split_whitespace().count());
        // `date_sort` is the first day of the period a partial date names,
        // in any calendar; a Hebrew or Republican year straddles two
        // Gregorian ones, so the period runs from there.
        let length = match parts {
            0 | 1 => 365,
            2 => 30,
            _ => 0,
        };
        Some(Self {
            from: from - Duration::days(slack),
            to: from + Duration::days(length + slack),
        })
    }

    /// Wholly before `other`.
    fn before(self, other: Self) -> bool {
        self.to < other.from
    }

    /// The fewest days from this date to `other`, negative when `other`
    /// may come first.
    fn least_days_to(self, other: Self) -> i64 {
        (other.from - self.to).num_days()
    }

    /// The most days from this date to `other`.
    fn most_days_to(self, other: Self) -> i64 {
        (other.to - self.from).num_days()
    }

    fn least_years_to(self, other: Self) -> f64 {
        self.least_days_to(other) as f64 / DAYS_PER_YEAR
    }

    fn most_years_to(self, other: Self) -> f64 {
        self.most_days_to(other) as f64 / DAYS_PER_YEAR
    }
}

/// A person as the rules read them.
struct Life<'a> {
    profile: &'a PersonProfile,
    /// The birth, or the baptism when the birth carries no date.
    birth: Option<Span>,
    /// The death, or the burial when the death carries no date.
    death: Option<Span>,
}

impl<'a> Life<'a> {
    fn new(profile: &'a PersonProfile) -> Self {
        Self {
            profile,
            birth: Span::of(profile.birth_or_baptism()),
            death: Span::of(profile.death_or_burial()),
        }
    }

    fn at(&self) -> PersonRef {
        person_ref(self.profile)
    }

    /// The person's own events, the four key ones first.
    fn events(&self) -> impl Iterator<Item = &'a ProfileEvent> {
        let p = self.profile;
        [&p.birth, &p.baptism, &p.death, &p.burial]
            .into_iter()
            .flatten()
            .chain(p.other_events.iter())
    }
}

/// What the rules found so far, by rule.
#[derive(Default)]
struct Findings(BTreeMap<&'static str, Vec<Anomaly>>);

impl Findings {
    fn add(&mut self, rule: &'static str, anomaly: Anomaly) {
        debug_assert!(RULES.iter().any(|(id, _, _)| *id == rule), "{rule}");
        self.0.entry(rule).or_default().push(anomaly);
    }

    fn into_rules(mut self) -> Vec<AnomalyRule> {
        RULES
            .iter()
            .filter_map(|(id, category, severity)| {
                let mut items = self.0.remove(id)?;
                let count = items.len() as i64;
                items.truncate(MAX_ITEMS_PER_RULE);
                Some(AnomalyRule {
                    rule: (*id).to_string(),
                    category: (*category).to_string(),
                    severity: (*severity).to_string(),
                    count,
                    items,
                })
            })
            .collect()
    }
}

/// Events that may rightly follow a death: its burial or cremation, a
/// funeral, the probate of a will; the free-form events (`Other`, `Fact`),
/// whose own text says what they are — a succession, a mention, the
/// transcription of the death — and which the rules cannot judge; and the
/// LDS ordinances, performed by proxy for the dead.
fn after_death_allowed(event_type: EventType) -> bool {
    matches!(
        event_type,
        EventType::Death
            | EventType::Burial
            | EventType::Cremation
            | EventType::Funeral
            | EventType::Probate
            | EventType::Other
            | EventType::Fact
            | EventType::LdsBaptism
            | EventType::LdsConfirmation
            | EventType::Endowment
            | EventType::LdsDotation
            | EventType::SealingChild
            | EventType::SealingSpouse
            | EventType::SealingParent
    )
}

/// Events that make a union and so date it.
fn dates_union(event_type: EventType) -> bool {
    matches!(event_type, EventType::Marriage | EventType::CivilUnion)
}

/// Finds a tree's anomalies.
pub fn compute(
    profiles: &[PersonProfile],
    witnesses: &[EventWitness],
    today: NaiveDate,
) -> TreeAnomalies {
    let mut sorted: Vec<&PersonProfile> = profiles.iter().collect();
    sorted.sort_by_key(|p| p.person_id);
    let lives: HashMap<Uuid, Life<'_>> =
        sorted.iter().map(|p| (p.person_id, Life::new(p))).collect();
    let life = |id: &Uuid| lives.get(id);
    let mut found = Findings::default();

    for profile in &sorted {
        let me = &lives[&profile.person_id];
        dates(me, today, &mut found);
        data_quality(me, &mut found);
        filiation(me, &life, &mut found);
    }
    unions(&sorted, &lives, &mut found);
    lineage(&sorted, &lives, &mut found);
    witnessing(&sorted, &lives, witnesses, &mut found);

    TreeAnomalies {
        persons: profiles.len() as i64,
        rules: found.into_rules(),
    }
}

fn dates(me: &Life<'_>, today: NaiveDate, found: &mut Findings) {
    let p = me.profile;
    if let (Some(birth), Some(death)) = (me.birth, me.death) {
        if death.before(birth) {
            found.add("death_before_birth", Anomaly::of(vec![me.at()]));
        } else {
            let least = birth.least_years_to(death);
            if least > MAX_LIFESPAN_YEARS {
                found.add(
                    "lived_over_105",
                    Anomaly::of(vec![me.at()]).with_value(least),
                );
            } else if least > OLD_CENTENARIAN_YEARS && birth.to.year() < CENTENARIAN_BEFORE_YEAR {
                found.add(
                    "centenarian_before_1900",
                    Anomaly::of(vec![me.at()]).with_value(least),
                );
            }
        }
    }
    let death = Span::of(p.death.as_ref());
    let burial = Span::of(p.burial.as_ref());
    if let (Some(death), Some(burial)) = (death, burial)
        && burial.before(death)
    {
        found.add("burial_before_death", Anomaly::of(vec![me.at()]));
    }
    if let (Some(baptism), Some(end)) = (Span::of(p.baptism.as_ref()), me.death)
        && end.before(baptism)
    {
        found.add("baptism_after_death", Anomaly::of(vec![me.at()]));
    }

    // The event standing for the birth is not compared with itself.
    let birth_event = p.birth_or_baptism().map(|e| e.event_id);
    let mut after_death = HashSet::new();
    for event in me.events() {
        let Some(span) = Span::of(Some(event)) else {
            continue;
        };
        if span.from > today {
            found.add(
                "future_date",
                Anomaly::of(vec![me.at()]).with_event(event.event_type),
            );
        }
        let key = matches!(
            event.event_type,
            EventType::Death | EventType::Burial | EventType::Birth
        );
        if let Some(birth) = me.birth
            && !key
            && Some(event.event_id) != birth_event
            && span.before(birth)
        {
            found.add(
                "event_before_birth",
                Anomaly::of(vec![me.at()]).with_event(event.event_type),
            );
        }
        if let Some(death) = death
            && !after_death_allowed(event.event_type)
            && event.event_type != EventType::Baptism
            && death.before(span)
        {
            after_death.insert(event.event_id);
            found.add(
                "event_after_death",
                Anomaly::of(vec![me.at()]).with_event(event.event_type),
            );
        }
        if let Some(burial) = burial
            && !after_death_allowed(event.event_type)
            && event.event_type != EventType::Baptism
            && !after_death.contains(&event.event_id)
            && burial.before(span)
        {
            found.add(
                "burial_not_last",
                Anomaly::of(vec![me.at()]).with_event(event.event_type),
            );
        }
    }
}

fn data_quality(me: &Life<'_>, found: &mut Findings) {
    let p = me.profile;
    let named = p.primary_name.as_ref().is_some_and(|n| {
        n.surname.as_deref().is_some_and(|s| !s.trim().is_empty())
            || n.given_names
                .as_deref()
                .is_some_and(|g| !g.trim().is_empty())
    });
    if !named {
        found.add("no_name", Anomaly::of(vec![me.at()]));
    }
    // Family events are read from the union, once.
    for event in me.events() {
        event_quality(vec![me.at()], None, event, found);
    }
    for link in &p.families_as_spouse {
        if matches!(
            (link.role, p.sex),
            (SpouseRole::Husband, Sex::Female) | (SpouseRole::Wife, Sex::Male)
        ) {
            found.add(
                "spouse_role_sex",
                Anomaly::of(vec![me.at()]).with_family(link.family_id),
            );
        }
    }
}

/// The checks every recorded date gets: that it can be read at all, and that
/// a range does not run backwards.
fn event_quality(
    persons: Vec<PersonRef>,
    family: Option<Uuid>,
    event: &ProfileEvent,
    found: &mut Findings,
) {
    let with = |anomaly: Anomaly| match family {
        Some(family) => anomaly.with_family(family),
        None => anomaly,
    };
    let value = event.date_value.as_deref().map(str::trim).unwrap_or("");
    if !value.is_empty() && event.date_sort.is_none() {
        found.add(
            "unreadable_date",
            with(
                Anomaly::of(persons)
                    .with_event(event.event_type)
                    .with_text(value),
            ),
        );
        return;
    }
    if event.date_qualifier == DateQualifier::Between
        && let (Some(start), Some(end)) = (
            event.date_sort,
            event_date::derive(event.calendar, event.date_value2.as_deref()),
        )
        && end < start
    {
        found.add(
            "reversed_range",
            with(Anomaly::of(persons).with_event(event.event_type)),
        );
    }
}

/// The rules about a child and their parents, read from the child's family.
fn filiation<'a>(
    me: &Life<'a>,
    life: &impl Fn(&Uuid) -> Option<&'a Life<'a>>,
    found: &mut Findings,
) {
    let Some(link) = &me.profile.family_as_child else {
        return;
    };
    // Adoptive, foster and step parents may be of any age and outlive
    // nothing.
    if !matches!(link.child_type, ChildType::Biological | ChildType::Unknown) {
        return;
    }
    let Some(born) = me.birth else {
        return;
    };
    for (parent_id, father) in [(link.father_id, true), (link.mother_id, false)] {
        let Some(parent) = parent_id.as_ref().and_then(life) else {
            continue;
        };
        let pair = || Anomaly::of(vec![me.at(), parent.at()]);
        if let Some(parent_born) = parent.birth {
            if born.before(parent_born) {
                found.add("parent_born_after_child", pair());
            } else {
                let most = parent_born.most_years_to(born);
                let least = parent_born.least_years_to(born);
                if most < MIN_PARENT_AGE_YEARS {
                    found.add("parent_too_young", pair().with_value(most));
                } else if father && least > MAX_FATHER_AGE_YEARS {
                    found.add("father_too_old", pair().with_value(least));
                } else if !father && least > MAX_MOTHER_AGE_YEARS {
                    found.add("mother_too_old", pair().with_value(least));
                }
            }
        }
        if let Some(parent_died) = parent.death {
            if !father && parent_died.before(born) {
                found.add("born_after_mother_death", pair());
            }
            let least = parent_died.least_days_to(born);
            if father && least > MAX_POSTHUMOUS_BIRTH_DAYS {
                found.add(
                    "born_long_after_father_death",
                    pair().with_value(least as f64),
                );
            }
        }
    }
}

/// A union as the rules read it: its spouses with their role, its date and
/// its children.
struct Union<'a> {
    family_id: Uuid,
    spouses: Vec<(Uuid, SpouseRole)>,
    date: Option<Span>,
    children: &'a [Uuid],
}

fn unions<'a>(sorted: &[&'a PersonProfile], lives: &HashMap<Uuid, Life<'a>>, found: &mut Findings) {
    let by_family = group_unions(sorted);
    family_events(sorted, &by_family, lives, found);
    for union in by_family.values() {
        union_rules(union, lives, found);
        siblings(union, lives, found);
    }
    spouse_rules(sorted, lives, found);
}

/// The tree's unions by family, each spouse once with the role of their
/// first link.
fn group_unions<'a>(sorted: &[&'a PersonProfile]) -> BTreeMap<Uuid, Union<'a>> {
    let mut by_family: BTreeMap<Uuid, Union<'a>> = BTreeMap::new();
    for profile in sorted {
        for link in &profile.families_as_spouse {
            let union = by_family.entry(link.family_id).or_insert_with(|| {
                let date = link
                    .events
                    .iter()
                    .chain(link.marriage.as_ref())
                    .filter(|e| dates_union(e.event_type))
                    .filter_map(|e| Span::of(Some(e)))
                    .min_by_key(|s| s.from);
                Union {
                    family_id: link.family_id,
                    spouses: Vec::new(),
                    date,
                    children: &link.children_ids,
                }
            });
            if !union.spouses.iter().any(|(id, _)| *id == profile.person_id) {
                union.spouses.push((profile.person_id, link.role));
            }
        }
    }
    by_family
}

/// The date checks of every family event, once per event.
fn family_events(
    sorted: &[&PersonProfile],
    by_family: &BTreeMap<Uuid, Union<'_>>,
    lives: &HashMap<Uuid, Life<'_>>,
    found: &mut Findings,
) {
    // A family event is on every spouse's link: its dates are checked once,
    // from the first spouse's.
    let mut checked_events = HashSet::new();
    for profile in sorted {
        for link in &profile.families_as_spouse {
            for event in link.events.iter().chain(link.marriage.as_ref()) {
                if checked_events.insert(event.event_id) {
                    let persons = by_family[&link.family_id]
                        .spouses
                        .iter()
                        .filter_map(|(id, _)| lives.get(id))
                        .map(Life::at)
                        .collect();
                    event_quality(persons, Some(link.family_id), event, found);
                }
            }
        }
    }
}

/// The rules on one union's spouses: how many, their roles, their ages at
/// the union and the gap between them.
fn union_rules(union: &Union<'_>, lives: &HashMap<Uuid, Life<'_>>, found: &mut Findings) {
    let spouses: Vec<&Life<'_>> = union
        .spouses
        .iter()
        .filter_map(|(id, _)| lives.get(id))
        .collect();
    let everyone = || spouses.iter().map(|l| l.at()).collect::<Vec<_>>();
    if union.spouses.len() > 2 {
        found.add(
            "union_many_spouses",
            Anomaly::of(everyone()).with_family(union.family_id),
        );
    }
    let husbands = union
        .spouses
        .iter()
        .filter(|(_, r)| *r == SpouseRole::Husband)
        .count();
    let wives = union
        .spouses
        .iter()
        .filter(|(_, r)| *r == SpouseRole::Wife)
        .count();
    if husbands > 1 || wives > 1 {
        found.add(
            "same_role_spouses",
            Anomaly::of(everyone()).with_family(union.family_id),
        );
    }
    if let Some(date) = union.date {
        union_dates(date, &spouses, union.family_id, found);
    }
    if let [a, b] = spouses.as_slice()
        && let (Some(x), Some(y)) = (a.birth, b.birth)
    {
        let gap = x.least_years_to(y).max(y.least_years_to(x));
        if gap > MAX_SPOUSE_GAP_YEARS {
            found.add(
                "spouses_age_gap",
                Anomaly::of(everyone())
                    .with_family(union.family_id)
                    .with_value(gap),
            );
        }
    }
}

/// A union dated before a spouse's birth or after their death, or at an age
/// too young or too old for one.
fn union_dates(date: Span, spouses: &[&Life<'_>], family_id: Uuid, found: &mut Findings) {
    for spouse in spouses {
        let at = || Anomaly::of(vec![spouse.at()]).with_family(family_id);
        if let Some(born) = spouse.birth {
            if date.before(born) {
                found.add("union_before_birth", at());
            } else {
                let most = born.most_years_to(date);
                let least = born.least_years_to(date);
                if most < MIN_UNION_AGE_YEARS {
                    found.add("union_too_young", at().with_value(most));
                } else if least > MAX_UNION_AGE_YEARS {
                    found.add("union_over_100", at().with_value(least));
                }
            }
        }
        if let Some(died) = spouse.death
            && died.before(date)
        {
            found.add("union_after_death", at());
        }
    }
}

/// The rules on each person's unions: the same spouse twice, spouses
/// bearing one name, a parent or a child for a spouse.
fn spouse_rules(sorted: &[&PersonProfile], lives: &HashMap<Uuid, Life<'_>>, found: &mut Findings) {
    let mut repeated = HashSet::new();
    for profile in sorted {
        let me = &lives[&profile.person_id];
        let mut spouses: HashMap<Uuid, usize> = HashMap::new();
        let mut names: HashMap<String, Vec<Uuid>> = HashMap::new();
        for link in &profile.families_as_spouse {
            let Some(spouse) = link.spouse_id else {
                continue;
            };
            *spouses.entry(spouse).or_default() += 1;
            if let Some(name) = lives
                .get(&spouse)
                .map(|l| normalize_for_search(&person_ref(l.profile).name))
                .filter(|n| !n.trim().is_empty())
            {
                let ids = names.entry(name).or_default();
                if !ids.contains(&spouse) {
                    ids.push(spouse);
                }
            }
            // Reported from the child's side only, so once.
            let parents = profile
                .family_as_child
                .as_ref()
                .map(|l| [l.father_id, l.mother_id])
                .unwrap_or_default();
            if parents.contains(&Some(spouse))
                && let Some(parent) = lives.get(&spouse)
            {
                found.add(
                    "union_with_parent_or_child",
                    Anomaly::of(vec![me.at(), parent.at()]).with_family(link.family_id),
                );
            }
        }
        for (spouse, times) in spouses {
            let pair = if profile.person_id < spouse {
                (profile.person_id, spouse)
            } else {
                (spouse, profile.person_id)
            };
            if times > 1
                && repeated.insert(pair)
                && let Some(other) = lives.get(&spouse)
            {
                found.add(
                    "repeated_union",
                    Anomaly::of(vec![me.at(), other.at()]).with_value(times as f64),
                );
            }
        }
        for ids in names.values().filter(|ids| ids.len() > 1) {
            let mut persons = vec![me.at()];
            persons.extend(ids.iter().filter_map(|id| lives.get(id)).map(Life::at));
            found.add("homonymous_spouses", Anomaly::of(persons));
        }
    }
}

/// The children of a union, by birth: births too close to be anything but
/// twins or an error, and gaps too long for one couple.
fn siblings(union: &Union<'_>, lives: &HashMap<Uuid, Life<'_>>, found: &mut Findings) {
    let mut born: Vec<(&Life<'_>, Span)> = union
        .children
        .iter()
        .filter_map(|id| lives.get(id))
        .filter(|child| {
            child.profile.family_as_child.as_ref().is_some_and(|l| {
                l.family_id == union.family_id
                    && matches!(l.child_type, ChildType::Biological | ChildType::Unknown)
            })
        })
        .filter_map(|child| child.birth.map(|span| (child, span)))
        .collect();
    born.sort_by_key(|(child, span)| (span.from, child.profile.person_id));
    for pair in born.windows(2) {
        let [(a, x), (b, y)] = pair else { continue };
        let least = x.least_days_to(*y).max(0);
        let most = x.most_days_to(*y);
        let both = || Anomaly::of(vec![a.at(), b.at()]).with_family(union.family_id);
        if least >= TWIN_DAYS && most < MIN_SIBLING_GAP_DAYS {
            found.add("siblings_too_close", both().with_value(least as f64));
        }
        let years = x.least_years_to(*y);
        if years > MAX_SIBLING_GAP_YEARS {
            found.add("siblings_far_apart", both().with_value(years));
        }
    }
}

/// The rules over whole lines: a person their own ancestor, an ancestor born
/// after a descendant.
fn lineage<'a>(
    sorted: &[&'a PersonProfile],
    lives: &HashMap<Uuid, Life<'a>>,
    found: &mut Findings,
) {
    let parents = |id: Uuid| -> Vec<Uuid> {
        lives
            .get(&id)
            .and_then(|l| l.profile.family_as_child.as_ref())
            .map(|l| {
                [l.father_id, l.mother_id]
                    .into_iter()
                    .flatten()
                    .filter(|p| lives.contains_key(p))
                    .collect()
            })
            .unwrap_or_default()
    };

    // Cycles: a depth-first walk up the parents, each cycle reported once
    // with the persons on it.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    let mut marks: HashMap<Uuid, Mark> = HashMap::new();
    let mut in_cycle: HashSet<Uuid> = HashSet::new();
    let mut cycles: Vec<Vec<Uuid>> = Vec::new();
    for start in sorted.iter().map(|p| p.person_id) {
        if marks.contains_key(&start) {
            continue;
        }
        // (person, parents left to visit)
        let mut stack: Vec<(Uuid, Vec<Uuid>)> = vec![(start, parents(start))];
        marks.insert(start, Mark::Open);
        while let Some((node, pending)) = stack.last_mut() {
            let node = *node;
            match pending.pop() {
                Some(next) => match marks.get(&next) {
                    None => {
                        marks.insert(next, Mark::Open);
                        stack.push((next, parents(next)));
                    }
                    Some(Mark::Open) => {
                        // Back to a person still open: everyone from them up
                        // the walk is on a loop.
                        let at = stack.iter().position(|(id, _)| *id == next).unwrap_or(0);
                        let mut cycle: Vec<Uuid> = stack[at..].iter().map(|(id, _)| *id).collect();
                        cycle.sort();
                        in_cycle.extend(cycle.iter().copied());
                        if !cycles.contains(&cycle) {
                            cycles.push(cycle);
                        }
                    }
                    Some(Mark::Done) => {}
                },
                None => {
                    marks.insert(node, Mark::Done);
                    stack.pop();
                }
            }
        }
    }
    for cycle in cycles {
        found.add(
            "own_ancestor",
            Anomaly::of(
                cycle
                    .iter()
                    .filter_map(|id| lives.get(id))
                    .map(Life::at)
                    .collect(),
            ),
        );
    }

    // The latest-born ancestor of each person: a parent born after their
    // child is its own rule, so a line is compared from the grandparents up,
    // and each ancestor is reported once, with the first descendant found
    // born before them.
    let mut latest: HashMap<Uuid, Option<(NaiveDate, Uuid)>> = HashMap::new();
    fn latest_of(
        id: Uuid,
        lives: &HashMap<Uuid, Life<'_>>,
        parents: &dyn Fn(Uuid) -> Vec<Uuid>,
        in_cycle: &HashSet<Uuid>,
        memo: &mut HashMap<Uuid, Option<(NaiveDate, Uuid)>>,
    ) -> Option<(NaiveDate, Uuid)> {
        // The person themself and every ancestor, the latest born.
        if let Some(known) = memo.get(&id) {
            return *known;
        }
        if in_cycle.contains(&id) {
            return None;
        }
        memo.insert(id, None);
        let own = lives.get(&id).and_then(|l| l.birth).map(|s| (s.from, id));
        let result = parents(id)
            .into_iter()
            .filter_map(|p| latest_of(p, lives, parents, in_cycle, memo))
            .chain(own)
            .max();
        memo.insert(id, result);
        result
    }
    let mut reported = HashSet::new();
    for profile in sorted {
        let id = profile.person_id;
        let Some(born) = lives[&id].birth else {
            continue;
        };
        if in_cycle.contains(&id) {
            continue;
        }
        let above = parents(id)
            .into_iter()
            .flat_map(parents)
            .filter_map(|g| latest_of(g, lives, &parents, &in_cycle, &mut latest))
            .max();
        if let Some((from, ancestor)) = above
            && from > born.to
            && reported.insert(ancestor)
            && let Some(elder) = lives.get(&ancestor)
        {
            found.add(
                "ancestor_born_after_descendant",
                Anomaly::of(vec![lives[&id].at(), elder.at()]),
            );
        }
    }
}

/// Godparent relations, male then female, as a witness link names them
/// once folded: the GEDCOM abbreviations and the words of the interface
/// languages. A relation naming neither (a witness, `GODP`) is not read.
const GODFATHER: &[&str] = &[
    "godfather",
    "godf",
    "parrain",
    "pate",
    "taufpate",
    "patenonkel",
    "padrino",
    "padrinho",
    "peetvader",
    "peter",
    "ojciec chrzestny",
    "chrzestny",
];
const GODMOTHER: &[&str] = &[
    "godmother",
    "godm",
    "marraine",
    "patin",
    "taufpatin",
    "patentante",
    "madrina",
    "madrinha",
    "peetmoeder",
    "meter",
    "matka chrzestna",
    "chrzestna",
];

fn witnessing<'a>(
    sorted: &[&'a PersonProfile],
    lives: &HashMap<Uuid, Life<'a>>,
    witnesses: &[EventWitness],
    found: &mut Findings,
) {
    // Every dated event with whose it is: a person's, or a union's spouses.
    let mut events: HashMap<Uuid, (&ProfileEvent, Vec<Uuid>, Option<Uuid>)> = HashMap::new();
    for profile in sorted {
        let me = &lives[&profile.person_id];
        for event in me.events() {
            events.insert(event.event_id, (event, vec![profile.person_id], None));
        }
        for link in &profile.families_as_spouse {
            for event in link.events.iter().chain(link.marriage.as_ref()) {
                let entry = events
                    .entry(event.event_id)
                    .or_insert_with(|| (event, Vec::new(), Some(link.family_id)));
                if !entry.1.contains(&profile.person_id) {
                    entry.1.push(profile.person_id);
                }
            }
        }
    }
    let mut rows: Vec<&EventWitness> = witnesses.iter().collect();
    rows.sort_by_key(|w| (w.event_id, w.sort_order, w.person_id));
    for row in rows {
        let (Some(witness), Some((event, owners, family))) =
            (lives.get(&row.person_id), events.get(&row.event_id))
        else {
            continue;
        };
        let persons = || {
            let mut persons = vec![witness.at()];
            persons.extend(owners.iter().filter_map(|id| lives.get(id)).map(Life::at));
            persons
        };
        let anomaly = || {
            let anomaly = Anomaly::of(persons()).with_event(event.event_type);
            match family {
                Some(family) => anomaly.with_family(*family),
                None => anomaly,
            }
        };
        if let Some(span) = Span::of(Some(event)) {
            if let Some(born) = witness.birth
                && span.before(born)
            {
                found.add("witness_before_birth", anomaly());
            }
            if let Some(died) = witness.death
                && died.before(span)
            {
                found.add("witness_after_death", anomaly());
            }
        }
        if let Some(relation) = row.relation.as_deref() {
            let folded = normalize_for_search(relation.trim());
            let wrong = match witness.profile.sex {
                Sex::Female => GODFATHER.contains(&folded.as_str()),
                Sex::Male => GODMOTHER.contains(&folded.as_str()),
                Sex::Unknown => false,
            };
            if wrong {
                found.add("godparent_sex", anomaly().with_text(relation));
            }
        }
    }
}

#[cfg(test)]
mod tests;
