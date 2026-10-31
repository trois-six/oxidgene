//! Anomalies: dates, filiations, unions, witnesses and records that are
//! impossible or unlikely (`docs/ui-tools.md` §3, which holds the whole
//! catalogue with the rules still proposed).
//!
//! Computed on each request from the person projections and what they do
//! not carry (see [`Records`]); nothing is stored. A rule fires only when it
//! holds for every day its dates may stand for (see `Span`), so an imprecise
//! date never raises an anomaly its precision could explain.

mod places;

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};
use oxidgene_core::calendar::DAYS_PER_YEAR;
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::search::fold_words;
use oxidgene_core::types::{AnomalyThresholds, EventWitness, FamilyChild, Place};
use oxidgene_core::{ChildType, DateQualifier, EventType, OxidGeneError, Sex, SpouseRole};
use serde::Serialize;
use uuid::Uuid;

use crate::reference::{PlaceLocation, ReferenceLang};
use crate::service::event_date;
use crate::service::statistics::{PersonRef, PlaceUsage, carries, locate_used, person_ref};

// ── Thresholds (`docs/ui-tools.md` §3.3) ──────────────────────────────────

/// The most items a rule lists; its count still says how many there are.
pub const MAX_ITEMS_PER_RULE: usize = 500;

/// A tree's [`AnomalyThresholds`] as the rules compare them: ages and gaps
/// in years, as fractions of years; gaps in days, distances and the slack of
/// an approximate date as they are.
#[derive(Debug, Clone, Copy)]
struct Limits {
    lifespan: f64,
    centenarian: f64,
    centenarian_before: i32,
    min_parent: f64,
    max_father: f64,
    max_mother: f64,
    posthumous_days: i64,
    twin_days: i64,
    sibling_gap_days: i64,
    sibling_far: f64,
    min_union: f64,
    max_union: f64,
    spouse_gap: f64,
    /// A recorded age's tolerance, in days.
    age_tolerance_days: i64,
    /// How far a date about, calculated or estimated may stray, each way, in
    /// days.
    slack_days: i64,
    alive: f64,
    same_day_km: f64,
}

impl From<&AnomalyThresholds> for Limits {
    fn from(t: &AnomalyThresholds) -> Self {
        let days = |years: i32| (f64::from(years) * DAYS_PER_YEAR) as i64;
        Self {
            lifespan: f64::from(t.max_lifespan_years),
            centenarian: f64::from(t.old_centenarian_years),
            centenarian_before: t.centenarian_before_year,
            min_parent: f64::from(t.min_parent_age_years),
            max_father: f64::from(t.max_father_age_years),
            max_mother: f64::from(t.max_mother_age_years),
            posthumous_days: i64::from(t.max_posthumous_birth_days),
            twin_days: i64::from(t.twin_days),
            sibling_gap_days: i64::from(t.min_sibling_gap_days),
            sibling_far: f64::from(t.max_sibling_gap_years),
            min_union: f64::from(t.min_union_age_years),
            max_union: f64::from(t.max_union_age_years),
            spouse_gap: f64::from(t.max_spouse_gap_years),
            age_tolerance_days: days(t.recorded_age_tolerance_years),
            // Whole years of 365 days, as the slack always counted.
            slack_days: i64::from(t.approximate_slack_years) * 365,
            alive: f64::from(t.max_age_alive_years),
            same_day_km: f64::from(t.max_same_day_distance_km),
        }
    }
}

/// The events a person has one of at most.
pub const UNIQUE_EVENTS: [EventType; 4] = [
    EventType::Birth,
    EventType::Baptism,
    EventType::Death,
    EventType::Burial,
];

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
    ("recorded_age_mismatch", "dates", "warning"),
    ("qualifier_contradiction", "dates", "warning"),
    ("alive_too_old", "dates", "warning"),
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
    ("child_of_several_families", "filiation", "warning"),
    ("same_name_living_siblings", "filiation", "warning"),
    ("union_before_birth", "unions", "error"),
    ("union_too_young", "unions", "warning"),
    ("union_over_100", "unions", "warning"),
    ("union_after_death", "unions", "error"),
    ("spouses_age_gap", "unions", "warning"),
    ("repeated_union", "unions", "warning"),
    ("homonymous_spouses", "unions", "warning"),
    ("union_with_parent_or_child", "unions", "error"),
    ("union_many_spouses", "unions", "warning"),
    ("union_with_sibling", "unions", "warning"),
    ("witness_before_birth", "witnesses", "error"),
    ("witness_after_death", "witnesses", "error"),
    ("godparent_sex", "witnesses", "warning"),
    ("spouse_role_sex", "data_quality", "warning"),
    ("same_role_spouses", "data_quality", "warning"),
    ("unreadable_date", "data_quality", "warning"),
    ("reversed_range", "data_quality", "warning"),
    ("no_name", "data_quality", "warning"),
    ("unique_event_twice", "data_quality", "warning"),
    ("given_name_sex", "data_quality", "warning"),
    ("place_spellings", "places", "warning"),
    ("distant_places_same_day", "places", "warning"),
];

/// What the rules read beside the person projections: what a projection
/// does not carry, loaded for the rules that need it.
#[derive(Debug, Clone, Default)]
pub struct Records {
    /// The thresholds the tree sets.
    pub thresholds: AnomalyThresholds,
    /// The tree's witness links.
    pub witnesses: Vec<EventWitness>,
    /// The persons holding more than one of an [`UNIQUE_EVENTS`] type: the
    /// person, the type and how many (a projection keeps one of each).
    pub repeated_events: Vec<(Uuid, EventType, i64)>,
    /// Every child link of the persons who are children of several families
    /// (a projection keeps the first); see [`Wanted::children`].
    pub child_links: Vec<FamilyChild>,
    /// Where the places compared by distance lie, `(latitude, longitude)`
    /// by place; see [`Wanted::places`].
    pub coordinates: HashMap<Uuid, (f64, f64)>,
}

/// What the projections show the rules need loaded: the persons who are
/// children of several families, and the places of a person's events on one
/// day, whose distance is measured.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wanted {
    pub children: Vec<Uuid>,
    pub places: Vec<Uuid>,
}

impl Wanted {
    /// What `profiles` need beside themselves.
    pub fn of(profiles: &[PersonProfile]) -> Self {
        let mut families: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
        for profile in profiles {
            if let Some(link) = &profile.family_as_child {
                families
                    .entry(profile.person_id)
                    .or_default()
                    .insert(link.family_id);
            }
            for link in &profile.families_as_spouse {
                for child in &link.children_ids {
                    families.entry(*child).or_default().insert(link.family_id);
                }
            }
        }
        let mut children: Vec<Uuid> = families
            .into_iter()
            .filter(|(_, families)| families.len() > 1)
            .map(|(child, _)| child)
            .collect();
        children.sort_unstable();
        let mut places: Vec<Uuid> = profiles
            .iter()
            .flat_map(|p| places::same_day_pairs(p))
            .flat_map(|(a, b)| [a.place_id, b.place_id])
            .flatten()
            .collect();
        places.sort_unstable();
        places.dedup();
        Self { children, places }
    }
}

/// A place an anomaly names.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AnomalyPlace {
    pub place_id: String,
    pub name: String,
}

/// A tree's anomalies. GraphQL serves it as `GqlTreeAnomalies`, whose
/// thresholds are the GraphQL object of the tree's.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TreeAnomalies {
    /// Persons examined.
    pub persons: i64,
    /// The thresholds the rules applied: the tree's.
    pub thresholds: AnomalyThresholds,
    /// The rules that found something, in catalogue order.
    pub rules: Vec<AnomalyRule>,
}

/// What one rule found.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AnomalyRule {
    pub rule: String,
    /// `dates`, `filiation`, `unions`, `witnesses`, `data_quality` or
    /// `places`.
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
    /// The places concerned, for the rules about places.
    pub places: Vec<AnomalyPlace>,
    /// The figure the rule measured: whole years for ages and gaps in
    /// years, days for gaps between births and deaths, kilometres for
    /// distances, a count for what is recorded too often.
    pub value: Option<i64>,
    /// The event concerned, an `EventType` in its snake_case form.
    pub event_type: Option<String>,
    /// The event `event_type` is compared with, for the rules comparing two.
    pub other_event_type: Option<String>,
    /// Recorded text the rule is about: an unreadable date, a relation, a
    /// given name.
    pub text: Option<String>,
}

impl Anomaly {
    fn of(persons: Vec<PersonRef>) -> Self {
        Self {
            persons,
            family_id: None,
            places: Vec::new(),
            value: None,
            event_type: None,
            other_event_type: None,
            text: None,
        }
    }

    fn with_other_event(mut self, event_type: EventType) -> Self {
        self.other_event_type = Some(event_type.to_string());
        self
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

/// Loads a tree's projections and the [`Records`] they do not carry, and
/// finds its anomalies, or with `tag` only those that name a person carrying
/// it (see [`compute_for_tag`]).
///
/// Beside the projections, it reads the witness links, the repeated key
/// events (counted by the database), and only when the projections show a
/// need for them the child links of the persons who are children of several
/// families and the places of a person's events on one day.
#[tracing::instrument(name = "anomalies.load", skip_all)]
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    profiles: &crate::profile::ProfileService,
    tree_id: Uuid,
    tag: Option<Uuid>,
) -> Result<TreeAnomalies, OxidGeneError> {
    use oxidgene_db::repo::{EventRepo, EventWitnessRepo, FamilyChildRepo};
    let tree = oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    if let Some(tag) = tag {
        oxidgene_db::repo::TagRepo::get(db, tree_id, tag).await?;
    }
    let persons = profiles.get_all_persons(tree_id).await?;
    let witnesses = EventWitnessRepo::list_by_tree(db, tree_id).await?;
    let repeated_events = EventRepo::repeated_person_events(db, tree_id, &UNIQUE_EVENTS).await?;
    let span = tracing::info_span!("anomalies.wanted", person.count = persons.len());
    let (persons, wanted) = crate::service::blocking::run(span, move || {
        let wanted = Wanted::of(&persons);
        (persons, wanted)
    })
    .await?;
    let child_links = if wanted.children.is_empty() {
        Vec::new()
    } else {
        FamilyChildRepo::list_by_persons(db, &wanted.children).await?
    };
    let coordinates = coordinates(db, &wanted.places).await?;
    let records = Records {
        thresholds: tree.anomaly_thresholds,
        witnesses,
        repeated_events,
        child_links,
        coordinates,
    };
    let today = chrono::Utc::now().date_naive();
    let span = tracing::info_span!(
        "anomalies.compute",
        person.count = persons.len(),
        witness.count = records.witnesses.len(),
    );
    crate::service::blocking::run(span, move || {
        compute_for_tag(&persons, &records, today, tag)
    })
    .await
}

/// Where the places `ids` lie: their own coordinates, else their label's in
/// the place dictionary, read once for them all; a place found in neither is
/// left out.
async fn coordinates(
    db: &sea_orm::DatabaseConnection,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, (f64, f64)>, OxidGeneError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let places = oxidgene_db::repo::PlaceRepo::get_many_untagged(db, ids).await?;
    let span = tracing::info_span!("anomalies.locate_compared", place.count = places.len());
    crate::service::blocking::run(span, move || {
        let mut found: HashMap<Uuid, (f64, f64)> = HashMap::new();
        let mut unplaced: Vec<&Place> = Vec::new();
        for place in &places {
            match place.latitude.zip(place.longitude) {
                Some(spot) => {
                    found.insert(place.id, spot);
                }
                None => unplaced.push(place),
            }
        }
        if !unplaced.is_empty() {
            let labels: Vec<(&str, i64)> = unplaced.iter().map(|p| (p.name.as_str(), 1)).collect();
            let located = crate::reference::locate_places(ReferenceLang::En, &labels);
            for (place, location) in unplaced.iter().zip(located) {
                if let Some(spot) = location.spot {
                    found.insert(place.id, spot);
                }
            }
        }
        found
    })
    .await
}

/// Loads a tree's used places and returns those that cannot be located, most
/// used first: the places the statistics count as not located, by the same
/// rule (`crate::service::statistics::locate_used`).
#[tracing::instrument(name = "anomalies.unlocated_places", skip_all)]
pub async fn load_unlocated_places(
    db: &sea_orm::DatabaseConnection,
    tree_id: Uuid,
) -> Result<Vec<PlaceUsage>, OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let places = oxidgene_db::repo::DictionaryRepo::places_with_usage(db, tree_id).await?;
    let span = tracing::info_span!("anomalies.locate", place.count = places.len());
    crate::service::blocking::run(span, move || {
        // The language only names countries and regions, which this list
        // does not show.
        unlocated_places(&places, |labels| {
            crate::reference::locate_places(ReferenceLang::En, labels)
        })
    })
    .await
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
/// month its whole month, widened by `slack_days` each way for a date about,
/// calculated or estimated. Dates before, after, perhaps, or ranges, say too
/// little to compare and have no span.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    from: NaiveDate,
    to: NaiveDate,
}

impl Span {
    fn of(event: Option<&ProfileEvent>, slack_days: i64) -> Option<Self> {
        let event = event?;
        let slack = match event.date_qualifier {
            DateQualifier::Exact => 0,
            DateQualifier::About | DateQualifier::Calculated | DateQualifier::Estimated => {
                slack_days
            }
            _ => return None,
        };
        let (from, to) = period(event.date_sort?, event.date_value.as_deref());
        Some(Self {
            from: from - Duration::days(slack),
            to: to + Duration::days(slack),
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
    /// The thresholds the rules apply.
    limits: &'a Limits,
    /// The birth, or the baptism when the birth carries no date.
    birth: Option<Span>,
    /// The death, or the burial when the death carries no date.
    death: Option<Span>,
}

impl<'a> Life<'a> {
    fn new(profile: &'a PersonProfile, limits: &'a Limits) -> Self {
        Self {
            profile,
            limits,
            birth: Span::of(profile.birth_or_baptism(), limits.slack_days),
            death: Span::of(profile.death_or_burial(), limits.slack_days),
        }
    }

    /// The span of `event`, with the thresholds' slack.
    fn span(&self, event: Option<&ProfileEvent>) -> Option<Span> {
        Span::of(event, self.limits.slack_days)
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

    /// Keeps only the anomalies `keep` accepts; a rule left with none is
    /// dropped.
    fn retain(&mut self, keep: impl Fn(&Anomaly) -> bool) {
        for items in self.0.values_mut() {
            items.retain(&keep);
        }
        self.0.retain(|_, items| !items.is_empty());
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
            | EventType::LdsInitiatory
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
pub fn compute(profiles: &[PersonProfile], records: &Records, today: NaiveDate) -> TreeAnomalies {
    compute_for_tag(profiles, records, today, None)
}

/// Finds a tree's anomalies, or with `tag` only those that name a person
/// carrying it (a union names its spouses) or a place the events of such a
/// person use. Every person is examined first, so a rule about a couple or a
/// witness sees the whole family.
pub fn compute_for_tag(
    profiles: &[PersonProfile],
    records: &Records,
    today: NaiveDate,
    tag: Option<Uuid>,
) -> TreeAnomalies {
    let limits = Limits::from(&records.thresholds);
    let mut sorted: Vec<&PersonProfile> = profiles.iter().collect();
    sorted.sort_by_key(|p| p.person_id);
    let lives: HashMap<Uuid, Life<'_>> = sorted
        .iter()
        .map(|p| (p.person_id, Life::new(p, &limits)))
        .collect();
    let life = |id: &Uuid| lives.get(id);
    let mut found = Findings::default();

    for profile in &sorted {
        let me = &lives[&profile.person_id];
        dates(me, today, &mut found);
        data_quality(me, &mut found);
        filiation(me, &life, &mut found);
        places::distant_places(me, &records.coordinates, &mut found);
    }
    repeated_events(&records.repeated_events, &lives, &mut found);
    unions(&sorted, &lives, &limits, &mut found);
    several_families(&sorted, &lives, &records.child_links, &mut found);
    lineage(&sorted, &lives, &mut found);
    witnessing(&sorted, &lives, &records.witnesses, &mut found);
    places::place_spellings(&sorted, &mut found);
    if let Some(tag) = tag {
        let carriers: Vec<&PersonProfile> = profiles.iter().filter(|p| carries(p, tag)).collect();
        let tagged: HashSet<String> = carriers.iter().map(|p| p.person_id.to_string()).collect();
        let used: HashSet<String> = carriers
            .iter()
            .flat_map(|p| places::events_of(p))
            .filter_map(|e| e.place_id)
            .map(|id| id.to_string())
            .collect();
        found.retain(|anomaly| {
            anomaly
                .persons
                .iter()
                .any(|person| tagged.contains(&person.person_id))
                || anomaly.places.iter().any(|p| used.contains(&p.place_id))
        });
    }

    TreeAnomalies {
        persons: profiles.len() as i64,
        thresholds: records.thresholds,
        rules: found.into_rules(),
    }
}

/// The key events recorded more than once for one person, as the database
/// counted them.
fn repeated_events(
    repeated: &[(Uuid, EventType, i64)],
    lives: &HashMap<Uuid, Life<'_>>,
    found: &mut Findings,
) {
    let mut sorted: Vec<&(Uuid, EventType, i64)> = repeated.iter().collect();
    sorted.sort_by_key(|(person, event_type, _)| (*person, event_type.to_string()));
    for (person, event_type, count) in sorted {
        if let Some(me) = lives.get(person) {
            found.add(
                "unique_event_twice",
                Anomaly::of(vec![me.at()])
                    .with_event(*event_type)
                    .with_value(*count as f64),
            );
        }
    }
}

/// The biological children (or of unknown kind) of more than one family: the
/// child, then the parents of each of those families.
fn several_families(
    sorted: &[&PersonProfile],
    lives: &HashMap<Uuid, Life<'_>>,
    links: &[FamilyChild],
    found: &mut Findings,
) {
    if links.is_empty() {
        return;
    }
    // The families the projections know, with their spouses: a link to a
    // family deleted since is not one.
    let mut spouses: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for profile in sorted {
        for link in &profile.families_as_spouse {
            spouses
                .entry(link.family_id)
                .or_default()
                .push(profile.person_id);
        }
        if let Some(link) = &profile.family_as_child {
            spouses.entry(link.family_id).or_default();
        }
    }
    let mut families: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for link in links {
        if matches!(link.child_type, ChildType::Biological | ChildType::Unknown)
            && spouses.contains_key(&link.family_id)
        {
            let of = families.entry(link.person_id).or_default();
            if !of.contains(&link.family_id) {
                of.push(link.family_id);
            }
        }
    }
    for (child, mut of) in families {
        let Some(me) = lives.get(&child).filter(|_| of.len() > 1) else {
            continue;
        };
        of.sort_unstable();
        let mut persons = vec![me.at()];
        let mut named = HashSet::from([child]);
        for parent in of.iter().flat_map(|family| &spouses[family]) {
            if named.insert(*parent)
                && let Some(parent) = lives.get(parent)
            {
                persons.push(parent.at());
            }
        }
        found.add(
            "child_of_several_families",
            Anomaly::of(persons).with_value(of.len() as f64),
        );
    }
}

fn dates(me: &Life<'_>, today: NaiveDate, found: &mut Findings) {
    lifespan(me, found);
    let p = me.profile;
    let death = me.span(p.death.as_ref());
    let burial = me.span(p.burial.as_ref());
    if let (Some(death), Some(burial)) = (death, burial)
        && burial.before(death)
    {
        found.add("burial_before_death", Anomaly::of(vec![me.at()]));
    }
    if let (Some(baptism), Some(end)) = (me.span(p.baptism.as_ref()), me.death)
        && end.before(baptism)
    {
        found.add("baptism_after_death", Anomaly::of(vec![me.at()]));
    }
    events_in_life(me, today, death, burial, found);
    recorded_ages(me, found);
    qualified_dates(me, found);
    alive_too_old(me, today, found);
}

/// A person with no death, burial or cremation recorded, born so long ago
/// that they cannot be alive.
fn alive_too_old(me: &Life<'_>, today: NaiveDate, found: &mut Findings) {
    let p = me.profile;
    let ended = p.death.is_some()
        || p.burial.is_some()
        || p.other_events
            .iter()
            .any(|e| e.event_type == EventType::Cremation);
    let Some(birth) = me.birth.filter(|_| !ended) else {
        return;
    };
    let age = birth.least_years_to(Span {
        from: today,
        to: today,
    });
    if age > me.limits.alive {
        found.add("alive_too_old", Anomaly::of(vec![me.at()]).with_value(age));
    }
}

/// The days a date before, after, between two dates or one of two allows:
/// either end may be open. Other dates are their [`Span`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    earliest: Option<NaiveDate>,
    latest: Option<NaiveDate>,
    /// Whether the qualifier opens or widens the date: before, after,
    /// between or or.
    qualified: bool,
}

impl Bounds {
    fn of(event: &ProfileEvent, slack_days: i64) -> Option<Self> {
        let first = period(event.date_sort?, event.date_value.as_deref());
        let second = || {
            let value = event.date_value2.as_deref();
            event_date::derive(event.calendar, value).map(|start| period(start, value))
        };
        let (earliest, latest) = match event.date_qualifier {
            DateQualifier::Before => (None, Some(first.1)),
            DateQualifier::After => (Some(first.0), None),
            DateQualifier::Between | DateQualifier::Or => {
                let second = second()?;
                (Some(first.0.min(second.0)), Some(first.1.max(second.1)))
            }
            _ => {
                let span = Span::of(Some(event), slack_days)?;
                return Some(Self {
                    earliest: Some(span.from),
                    latest: Some(span.to),
                    qualified: false,
                });
            }
        };
        Some(Self {
            earliest,
            latest,
            qualified: true,
        })
    }

    /// Wholly after `other`, whatever days the two allow.
    fn after(self, other: Self) -> bool {
        matches!((self.earliest, other.latest), (Some(from), Some(to)) if from > to)
    }
}

/// The first and last day of the period a date names from its first day
/// `start`: its year, its month or its day. `start` is the date's sort key,
/// the first day of the period a partial date names in any calendar; a
/// Hebrew or Republican year straddles two Gregorian ones, so the period
/// runs from there.
fn period(start: NaiveDate, value: Option<&str>) -> (NaiveDate, NaiveDate) {
    let length = match value.map_or(0, |v| v.split_whitespace().count()) {
        0 | 1 => 365,
        2 => 30,
        _ => 0,
    };
    (start, start + Duration::days(length))
}

/// The key events of a life in the order they must keep: each pair whose
/// dates contradict each other, one of them before, after or between dates,
/// for every day they allow. Dates without such a qualifier are the other
/// rules' (`death_before_birth`, `event_before_birth`…).
fn qualified_dates(me: &Life<'_>, found: &mut Findings) {
    let p = me.profile;
    let order = [&p.birth, &p.baptism, &p.death, &p.burial];
    let bounds: Vec<Option<(&ProfileEvent, Bounds)>> = order
        .iter()
        .map(|e| {
            e.as_ref()
                .and_then(|e| Bounds::of(e, me.limits.slack_days).map(|b| (e, b)))
        })
        .collect();
    for (i, first) in bounds.iter().enumerate() {
        for second in &bounds[i + 1..] {
            if let (Some((a, x)), Some((b, y))) = (first, second)
                && (x.qualified || y.qualified)
                && x.after(*y)
            {
                found.add(
                    "qualifier_contradiction",
                    Anomaly::of(vec![me.at()])
                        .with_event(a.event_type)
                        .with_other_event(b.event_type),
                );
            }
        }
    }
}

/// The ages records give that the dates contradict: the recorded age, widened
/// by the thresholds' tolerance each way, shares no day with the age
/// computed between the birth and the event — whatever days their dates
/// stand for.
fn recorded_ages(me: &Life<'_>, found: &mut Findings) {
    let Some(birth) = me.birth else {
        return;
    };
    // The event standing for the birth is not compared with itself.
    let birth_event = me.profile.birth_or_baptism().map(|e| e.event_id);
    let family_events = me
        .profile
        .families_as_spouse
        .iter()
        .flat_map(|link| link.events.iter());
    for event in me.events().chain(family_events) {
        let Some(recorded) = event
            .age
            .as_deref()
            .and_then(|age| age.parse::<oxidgene_core::types::AgeAtEvent>().ok())
        else {
            continue;
        };
        let Some(span) = me
            .span(Some(event))
            .filter(|_| Some(event.event_id) != birth_event)
        else {
            continue;
        };
        let tolerance = me.limits.age_tolerance_days;
        let (min, max) = recorded.bounds();
        let too_old = i64::from(min) - tolerance > birth.most_days_to(span);
        let too_young =
            max.is_some_and(|max| i64::from(max) + tolerance < birth.least_days_to(span));
        if too_old || too_young {
            found.add(
                "recorded_age_mismatch",
                Anomaly::of(vec![me.at()])
                    .with_event(event.event_type)
                    .with_value(birth.least_years_to(span).max(0.0))
                    .with_text(&recorded.to_string()),
            );
        }
    }
}

/// A death before the birth, or a life too long.
fn lifespan(me: &Life<'_>, found: &mut Findings) {
    let (Some(birth), Some(death)) = (me.birth, me.death) else {
        return;
    };
    if death.before(birth) {
        found.add("death_before_birth", Anomaly::of(vec![me.at()]));
        return;
    }
    let least = birth.least_years_to(death);
    let limits = me.limits;
    if least > limits.lifespan {
        found.add(
            "lived_over_105",
            Anomaly::of(vec![me.at()]).with_value(least),
        );
    } else if least > limits.centenarian && birth.to.year() < limits.centenarian_before {
        found.add(
            "centenarian_before_1900",
            Anomaly::of(vec![me.at()]).with_value(least),
        );
    }
}

/// A person's events against today, their birth, their `death` and their
/// `burial`.
fn events_in_life(
    me: &Life<'_>,
    today: NaiveDate,
    death: Option<Span>,
    burial: Option<Span>,
    found: &mut Findings,
) {
    // The event standing for the birth is not compared with itself.
    let birth_event = me.profile.birth_or_baptism().map(|e| e.event_id);
    for event in me.events() {
        let Some(span) = me.span(Some(event)) else {
            continue;
        };
        let anomaly = || Anomaly::of(vec![me.at()]).with_event(event.event_type);
        if span.from > today {
            found.add("future_date", anomaly());
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
            found.add("event_before_birth", anomaly());
        }
        if after_death_allowed(event.event_type) || event.event_type == EventType::Baptism {
            continue;
        }
        // An event after the death is not also reported after the burial.
        if death.is_some_and(|death| death.before(span)) {
            found.add("event_after_death", anomaly());
        } else if burial.is_some_and(|burial| burial.before(span)) {
            found.add("burial_not_last", anomaly());
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
    given_name_of_other_sex(me, found);
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

/// A first given name the reference sheets give to the other sex alone. The
/// first given name is the first word of the given names, a hyphenated
/// compound (`Jean-Baptiste`) whole; it is read only when a sheet names it
/// itself, so a compound no sheet names says nothing about its parts.
fn given_name_of_other_sex(me: &Life<'_>, found: &mut Findings) {
    let p = me.profile;
    if p.sex == Sex::Unknown {
        return;
    }
    let Some(first) = p
        .primary_name
        .as_ref()
        .and_then(|n| n.given_names.as_deref())
        .and_then(|g| g.split_whitespace().next())
    else {
        return;
    };
    if crate::reference::given_name_sex(first).is_some_and(|sex| sex != p.sex) {
        found.add(
            "given_name_sex",
            Anomaly::of(vec![me.at()]).with_text(first),
        );
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
        for (rule, value) in parent_rules(born, parent, father) {
            let pair = Anomaly::of(vec![me.at(), parent.at()]);
            let anomaly = match value {
                Some(value) => pair.with_value(value),
                None => pair,
            };
            found.add(rule, anomaly);
        }
    }
}

/// The filiation rules a child born at `born` breaks with a parent, the
/// `father` or else the mother: each rule with its value, when it has one.
fn parent_rules(born: Span, parent: &Life<'_>, father: bool) -> Vec<(&'static str, Option<f64>)> {
    let mut broken: Vec<(&'static str, Option<f64>)> = Vec::new();
    if let Some(parent_born) = parent.birth {
        broken.extend(parent_age_rule(born, parent_born, father, parent.limits));
    }
    if let Some(parent_died) = parent.death {
        if !father && parent_died.before(born) {
            broken.push(("born_after_mother_death", None));
        }
        let least = parent_died.least_days_to(born);
        if father && least > parent.limits.posthumous_days {
            broken.push(("born_long_after_father_death", Some(least as f64)));
        }
    }
    broken
}

/// The rule a parent's age at the birth breaks, if any.
fn parent_age_rule(
    born: Span,
    parent_born: Span,
    father: bool,
    limits: &Limits,
) -> Option<(&'static str, Option<f64>)> {
    if born.before(parent_born) {
        return Some(("parent_born_after_child", None));
    }
    let most = parent_born.most_years_to(born);
    let least = parent_born.least_years_to(born);
    let (oldest, too_old) = if father {
        (limits.max_father, "father_too_old")
    } else {
        (limits.max_mother, "mother_too_old")
    };
    if most < limits.min_parent {
        Some(("parent_too_young", Some(most)))
    } else if least > oldest {
        Some((too_old, Some(least)))
    } else {
        None
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

fn unions<'a>(
    sorted: &[&'a PersonProfile],
    lives: &HashMap<Uuid, Life<'a>>,
    limits: &Limits,
    found: &mut Findings,
) {
    let by_family = group_unions(sorted, limits);
    family_events(sorted, &by_family, lives, found);
    for union in by_family.values() {
        union_rules(union, lives, limits, found);
        siblings(union, lives, limits, found);
    }
    spouse_rules(sorted, lives, found);
}

/// The tree's unions by family, each spouse once with the role of their
/// first link.
fn group_unions<'a>(sorted: &[&'a PersonProfile], limits: &Limits) -> BTreeMap<Uuid, Union<'a>> {
    let mut by_family: BTreeMap<Uuid, Union<'a>> = BTreeMap::new();
    for profile in sorted {
        for link in &profile.families_as_spouse {
            let union = by_family.entry(link.family_id).or_insert_with(|| {
                let date = link
                    .events
                    .iter()
                    .filter(|e| dates_union(e.event_type))
                    .filter_map(|e| Span::of(Some(e), limits.slack_days))
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
            for event in &link.events {
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
fn union_rules(
    union: &Union<'_>,
    lives: &HashMap<Uuid, Life<'_>>,
    limits: &Limits,
    found: &mut Findings,
) {
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
        union_dates(date, &spouses, union.family_id, limits, found);
    }
    if let [a, b] = spouses.as_slice()
        && let (Some(x), Some(y)) = (a.birth, b.birth)
    {
        let gap = x.least_years_to(y).max(y.least_years_to(x));
        if gap > limits.spouse_gap {
            found.add(
                "spouses_age_gap",
                Anomaly::of(everyone())
                    .with_family(union.family_id)
                    .with_value(gap),
            );
        }
    }
    if let [a, b] = spouses.as_slice()
        && siblings_by_birth(a.profile, b.profile)
    {
        found.add(
            "union_with_sibling",
            Anomaly::of(everyone()).with_family(union.family_id),
        );
    }
}

/// Whether two persons are children of one family, or share a parent, as
/// biological children (or of unknown kind).
fn siblings_by_birth(a: &PersonProfile, b: &PersonProfile) -> bool {
    let birth = |p: &PersonProfile| {
        p.family_as_child
            .as_ref()
            .filter(|l| matches!(l.child_type, ChildType::Biological | ChildType::Unknown))
            .map(|l| (l.family_id, [l.father_id, l.mother_id]))
    };
    let (Some((x, x_parents)), Some((y, y_parents))) = (birth(a), birth(b)) else {
        return false;
    };
    x == y
        || x_parents
            .iter()
            .flatten()
            .any(|parent| y_parents.contains(&Some(*parent)))
}

/// A union dated before a spouse's birth or after their death, or at an age
/// too young or too old for one.
fn union_dates(
    date: Span,
    spouses: &[&Life<'_>],
    family_id: Uuid,
    limits: &Limits,
    found: &mut Findings,
) {
    for spouse in spouses {
        let at = || Anomaly::of(vec![spouse.at()]).with_family(family_id);
        if let Some(born) = spouse.birth {
            if date.before(born) {
                found.add("union_before_birth", at());
            } else {
                let most = born.most_years_to(date);
                let least = born.least_years_to(date);
                if most < limits.min_union {
                    found.add("union_too_young", at().with_value(most));
                } else if least > limits.max_union {
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
                .map(|l| fold_words(&person_ref(l.profile).name))
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
fn siblings(
    union: &Union<'_>,
    lives: &HashMap<Uuid, Life<'_>>,
    limits: &Limits,
    found: &mut Findings,
) {
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
        if least >= limits.twin_days && most < limits.sibling_gap_days {
            found.add("siblings_too_close", both().with_value(least as f64));
        }
        let years = x.least_years_to(*y);
        if years > limits.sibling_far {
            found.add("siblings_far_apart", both().with_value(years));
        }
    }
    same_name_alive(union.family_id, &born, found);
}

/// Two children of a union bearing the same given names (folded), the elder
/// known to be alive at the younger's birth: a child was often named after
/// an elder sibling who had died, rarely after a living one. Known alive
/// means the elder's death, or an event of their life other than one that
/// may follow a death, wholly after the younger's birth; an elder of whom
/// nothing later is known may have died before.
fn same_name_alive(family_id: Uuid, born: &[(&Life<'_>, Span)], found: &mut Findings) {
    let name = |life: &Life<'_>| {
        life.profile
            .primary_name
            .as_ref()
            .and_then(|n| n.given_names.as_deref())
            .map(fold_words)
            .filter(|n| !n.trim().is_empty())
    };
    for (i, (elder, _)) in born.iter().enumerate() {
        let Some(elder_name) = name(elder) else {
            continue;
        };
        for (younger, younger_born) in &born[i + 1..] {
            if name(younger).as_ref() == Some(&elder_name) && alive_after(elder, younger_born.to) {
                found.add(
                    "same_name_living_siblings",
                    Anomaly::of(vec![younger.at(), elder.at()])
                        .with_family(family_id)
                        .with_text(
                            younger
                                .profile
                                .primary_name
                                .as_ref()
                                .and_then(|n| n.given_names.as_deref())
                                .unwrap_or_default(),
                        ),
                );
            }
        }
    }
}

/// Whether `p` is known to be alive after `day`: their death, or an event of
/// their life that does not follow a death, comes wholly after it.
fn alive_after(life: &Life<'_>, day: NaiveDate) -> bool {
    let p = life.profile;
    let later = |event: &ProfileEvent| life.span(Some(event)).is_some_and(|s| s.from > day);
    p.death.as_ref().is_some_and(later)
        || [&p.birth, &p.baptism]
            .into_iter()
            .flatten()
            .chain(&p.other_events)
            .chain(p.families_as_spouse.iter().flat_map(|l| &l.events))
            .filter(|e| !after_death_allowed(e.event_type))
            .any(later)
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
    let events = owned_events(sorted, lives);
    let mut rows: Vec<&EventWitness> = witnesses.iter().collect();
    rows.sort_by_key(|w| (w.event_id, w.sort_order, w.person_id));
    for row in rows {
        let (Some(witness), Some((event, owners, family))) =
            (lives.get(&row.person_id), events.get(&row.event_id))
        else {
            continue;
        };
        let anomaly = || {
            let mut persons = vec![witness.at()];
            persons.extend(owners.iter().filter_map(|id| lives.get(id)).map(Life::at));
            let anomaly = Anomaly::of(persons).with_event(event.event_type);
            match family {
                Some(family) => anomaly.with_family(*family),
                None => anomaly,
            }
        };
        if let Some(span) = witness.span(Some(event)) {
            if witness.birth.is_some_and(|born| span.before(born)) {
                found.add("witness_before_birth", anomaly());
            }
            if witness.death.is_some_and(|died| died.before(span)) {
                found.add("witness_after_death", anomaly());
            }
        }
        if let Some(relation) = row.relation.as_deref()
            && godparent_of_other_sex(witness.profile.sex, relation)
        {
            found.add("godparent_sex", anomaly().with_text(relation));
        }
    }
}

/// Every dated event with whose it is: a person's, or a union's spouses and
/// family.
fn owned_events<'a>(
    sorted: &[&'a PersonProfile],
    lives: &HashMap<Uuid, Life<'a>>,
) -> HashMap<Uuid, (&'a ProfileEvent, Vec<Uuid>, Option<Uuid>)> {
    let mut events: HashMap<Uuid, (&ProfileEvent, Vec<Uuid>, Option<Uuid>)> = HashMap::new();
    for profile in sorted {
        let me = &lives[&profile.person_id];
        for event in me.events() {
            events.insert(event.event_id, (event, vec![profile.person_id], None));
        }
        for link in &profile.families_as_spouse {
            for event in &link.events {
                let entry = events
                    .entry(event.event_id)
                    .or_insert_with(|| (event, Vec::new(), Some(link.family_id)));
                if !entry.1.contains(&profile.person_id) {
                    entry.1.push(profile.person_id);
                }
            }
        }
    }
    events
}

/// Whether a witness of sex `sex` is recorded as a godparent of the other.
fn godparent_of_other_sex(sex: Sex, relation: &str) -> bool {
    let folded = fold_words(relation.trim());
    match sex {
        Sex::Female => GODFATHER.contains(&folded.as_str()),
        Sex::Male => GODMOTHER.contains(&folded.as_str()),
        Sex::Unknown => false,
    }
}

#[cfg(test)]
mod tests;
