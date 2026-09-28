//! Tree statistics: the figures of the Statistics page (`docs/ui-statistics.md`),
//! computed on each request from the person projections and the place usages.
//!
//! Nothing here is stored. Every age and average is over the records whose
//! dates are good enough for it (§7 of the specification): exact dates, or
//! also approximate ones when the caller asks, precise to the month for
//! month shares and to the day for weekday shares.
//!
//! Time series are filed by year, as sums and counts, so the client groups
//! them into periods of any width over any range of years without asking
//! again.

mod records;

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::types::Place;
use oxidgene_core::{DateQualifier, EventType, Sex, SpouseRole};
use serde::Serialize;
use uuid::Uuid;

use crate::reference::{PlaceLocation, ReferenceLang};

pub use records::StatRecord;

const TOP: usize = 10;
const LIST: usize = 100;
/// A person with no recorded death, born fewer years ago than this, may be
/// alive.
const POSSIBLY_ALIVE_YEARS: i32 = 120;
const PYRAMID_BAND: i64 = 5;

/// Everything the Statistics page draws.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct TreeStatistics {
    pub persons: i64,
    pub men: i64,
    pub women: i64,
    pub unknown_sex: i64,
    pub unions: i64,
    /// Places named by at least one event or media.
    pub places: i64,
    pub sources: i64,
    /// The first and the last year of the tree's dated events.
    pub first_year: Option<i32>,
    pub last_year: Option<i32>,
    /// Persons with a dated birth (or baptism), and with a dated death (or
    /// burial).
    pub dated_births: i64,
    pub dated_deaths: i64,
    /// Persons without a known parent, who are no one's parent, and who
    /// are no one's spouse.
    pub without_parents: i64,
    pub without_children: i64,
    pub without_union: i64,
    /// Distinct family names and first given names.
    pub surnames: i64,
    pub given_names: i64,
    /// Age at death, in years.
    pub lifespan: SexSummary,
    /// Age at the first union, in years.
    pub first_union_age: SexSummary,
    /// Parents' age at the birth of each of their children, in years.
    pub generation_interval: Summary,
    /// Children per union.
    pub family_size: Summary,
    pub top_surnames: Vec<CountEntry>,
    pub top_given_names_men: Vec<CountEntry>,
    pub top_given_names_women: Vec<CountEntry>,
    pub top_occupations: Vec<CountEntry>,
    /// Every event type with how many events it has, most used first; the
    /// label is the `EventType` in its snake_case form.
    pub event_types: Vec<CountEntry>,
    /// How many unions have 0, 1, 2… children, by that number.
    pub children_histogram: Vec<i64>,
    /// Per year, the births, baptisms, unions, deaths and burials dated in
    /// it, however their dates are qualified.
    pub events_by_year: Vec<YearCounts>,
    /// Per year, the births (or baptisms) of men and of women.
    pub births_by_sex: Vec<YearCounts>,
    /// Per year of birth, the births, and the deaths before one and before
    /// five years of age among them.
    pub mortality: Vec<YearCounts>,
    /// Age at death by year of death.
    pub age_at_death: SexSeries,
    /// Age at death by year of birth.
    pub life_expectancy: SexSeries,
    /// Per year, the births in each month.
    pub births_by_month: Vec<YearCounts>,
    pub parents_age: ParentAgeSeries,
    pub age_at_first_union: SexSeries,
    /// Per year, the unions on each weekday, Monday first.
    pub unions_by_weekday: Vec<YearCounts>,
    /// Per year, the unions in each month.
    pub unions_by_month: Vec<YearCounts>,
    /// Union durations, in years.
    pub union_duration: Vec<YearSum>,
    pub children_per_union: Vec<YearSum>,
    /// Time between two births in a family, in months.
    pub birth_spacing: Vec<YearSum>,
    /// Gap between a family's first and last child, in months.
    pub first_last_child_gap: Vec<YearSum>,
    /// Age difference between spouses, in months.
    pub spouse_age_gap: Vec<YearSum>,
    pub pyramid: Vec<PyramidBand>,
    pub records: Vec<StatRecord>,
    pub recent_births: Vec<PersonRecord>,
    pub recent_deaths: Vec<PersonRecord>,
    pub recent_unions: Vec<UnionRecord>,
    pub oldest_possibly_alive: Vec<PersonRecord>,
    pub longest_lives: Vec<PersonRecord>,
    /// The unions with the most children, most first.
    pub largest_families: Vec<FamilyRecord>,
    /// The used places that could be located, for the heat map.
    pub located_places: Vec<PlaceUsage>,
    /// The ten most used places, located or not.
    pub top_places: Vec<PlaceUsage>,
    /// Used places that could not be located.
    pub unlocated_places: i64,
    /// Distinct countries, regions and subdivisions of the used places.
    pub countries: i64,
    pub regions: i64,
    pub subdivisions: i64,
    /// Births (or baptisms) by the country, region and subdivision of their
    /// place, named in the requested language.
    pub births_by_country: Vec<CountEntry>,
    pub births_by_region: Vec<CountEntry>,
    pub births_by_subdivision: Vec<CountEntry>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct CountEntry {
    pub label: String,
    pub count: i64,
}

/// The values one year adds to an average: their sum and how many they
/// are, so a period's average is its years' sums over their counts.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct YearSum {
    pub year: i32,
    pub sum: f64,
    pub count: i64,
}

/// One year's counts per category (a month, a weekday, a kind of event).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct YearCounts {
    pub year: i32,
    pub counts: Vec<i64>,
}

/// An average for men and for women, by year.
#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct SexSeries {
    pub men: Vec<YearSum>,
    pub women: Vec<YearSum>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct ParentAgeSeries {
    pub father_first_child: Vec<YearSum>,
    pub mother_first_child: Vec<YearSum>,
    pub father_last_child: Vec<YearSum>,
    pub mother_last_child: Vec<YearSum>,
    /// At the birth of each child, by year of that birth.
    pub father_every_child: Vec<YearSum>,
    pub mother_every_child: Vec<YearSum>,
}

/// A set of values in brief, to a tenth; every figure is absent when there
/// is no value.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct Summary {
    pub count: i64,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    /// Population standard deviation.
    pub std_dev: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// A summary over everyone, over men and over women.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct SexSummary {
    pub all: Summary,
    pub men: Summary,
    pub women: Summary,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct PyramidBand {
    /// First age of the band, which spans five years.
    pub from: i64,
    pub men: i64,
    pub women: i64,
}

/// A date as recorded, for the client to format in its language.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct RecordDate {
    pub value: Option<String>,
    pub value2: Option<String>,
    /// `DateQualifier` in its snake_case form.
    pub qualifier: String,
    /// `Calendar` in its snake_case form.
    pub calendar: String,
    pub sort: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct PersonRecord {
    pub person_id: String,
    pub name: String,
    /// `Sex` in its snake_case form.
    pub sex: String,
    /// The event the list is about (a birth, a death).
    pub date: Option<RecordDate>,
    pub place: Option<String>,
    pub birth: Option<RecordDate>,
    pub death: Option<RecordDate>,
    /// Age in whole years, at death or today.
    pub age: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct UnionRecord {
    pub family_id: String,
    pub spouses: Vec<PersonRef>,
    pub date: RecordDate,
    pub place: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct FamilyRecord {
    pub family_id: String,
    pub spouses: Vec<PersonRef>,
    pub children: i64,
    /// The union's date, when it has one.
    pub date: Option<RecordDate>,
}

/// A person named in a list or a record.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct PersonRef {
    pub person_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct PlaceUsage {
    pub place_id: String,
    pub name: String,
    pub count: i64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// How precise a date is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precision {
    Year,
    Month,
    Day,
}

/// Which dates ages and averages may use: exact ones, and with
/// `approximate` also those about, calculated or estimated. Before, after,
/// perhaps and ranges never count.
#[derive(Debug, Clone, Copy)]
struct Dates {
    approximate: bool,
}

impl Dates {
    /// An event's date when it is good enough to compute with, with its
    /// precision read from how many parts the value has.
    fn of(self, event: Option<&ProfileEvent>) -> Option<(NaiveDate, Precision)> {
        let event = event?;
        let usable = match event.date_qualifier {
            DateQualifier::Exact => true,
            DateQualifier::About | DateQualifier::Calculated | DateQualifier::Estimated => {
                self.approximate
            }
            _ => false,
        };
        if !usable {
            return None;
        }
        let date = event.date_sort?;
        let parts = event
            .date_value
            .as_deref()
            .map_or(0, |v| v.split_whitespace().count());
        let precision = match parts {
            0 | 1 => Precision::Year,
            2 => Precision::Month,
            _ => Precision::Day,
        };
        Some((date, precision))
    }

    fn date(self, event: Option<&ProfileEvent>) -> Option<NaiveDate> {
        self.of(event).map(|(date, _)| date)
    }
}

fn years_between(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64 / 365.2425
}

fn months_between(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64 / 30.436875
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

/// Sums and counts per year, for averages.
#[derive(Default)]
struct Average(BTreeMap<i32, (f64, i64)>);

impl Average {
    fn add(&mut self, date: NaiveDate, value: f64) {
        let slot = self.0.entry(date.year()).or_insert((0.0, 0));
        slot.0 += value;
        slot.1 += 1;
    }

    /// The years with values, oldest first. Sums keep three decimals: far
    /// finer than the tenth the page shows, and a lighter response.
    fn years(self) -> Vec<YearSum> {
        self.0
            .into_iter()
            .map(|(year, (sum, count))| YearSum {
                year,
                sum: (sum * 1000.0).round() / 1000.0,
                count,
            })
            .collect()
    }
}

/// Counts per year and category.
struct Distribution {
    categories: usize,
    counts: BTreeMap<i32, Vec<i64>>,
}

impl Distribution {
    fn new(categories: usize) -> Self {
        Self {
            categories,
            counts: BTreeMap::new(),
        }
    }

    fn add(&mut self, date: NaiveDate, category: usize) {
        self.counts
            .entry(date.year())
            .or_insert_with(|| vec![0; self.categories])[category] += 1;
    }

    fn years(self) -> Vec<YearCounts> {
        self.counts
            .into_iter()
            .map(|(year, counts)| YearCounts { year, counts })
            .collect()
    }
}

fn summary(mut values: Vec<f64>) -> Summary {
    values.retain(|v| v.is_finite());
    values.sort_by(f64::total_cmp);
    let (Some(min), Some(max)) = (values.first().copied(), values.last().copied()) else {
        return Summary::default();
    };
    let count = values.len();
    let mean = values.iter().sum::<f64>() / count as f64;
    let median = if count % 2 == 1 {
        values[count / 2]
    } else {
        (values[count / 2 - 1] + values[count / 2]) / 2.0
    };
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
    Summary {
        count: count as i64,
        mean: Some(round1(mean)),
        median: Some(round1(median)),
        std_dev: Some(round1(variance.sqrt())),
        min: Some(round1(min)),
        max: Some(round1(max)),
    }
}

/// Values for everyone, for men and for women.
#[derive(Default)]
struct BySex {
    all: Vec<f64>,
    men: Vec<f64>,
    women: Vec<f64>,
}

impl BySex {
    fn add(&mut self, sex: Sex, value: f64) {
        self.all.push(value);
        match sex {
            Sex::Male => self.men.push(value),
            Sex::Female => self.women.push(value),
            Sex::Unknown => {}
        }
    }

    fn summary(self) -> SexSummary {
        SexSummary {
            all: summary(self.all),
            men: summary(self.men),
            women: summary(self.women),
        }
    }
}

fn record_date(event: &ProfileEvent) -> RecordDate {
    RecordDate {
        value: event.date_value.clone(),
        value2: event.date_value2.clone(),
        qualifier: event.date_qualifier.to_string(),
        calendar: event.calendar.to_string(),
        sort: event.date_sort,
    }
}

fn display_name(profile: &PersonProfile) -> String {
    profile
        .primary_name
        .as_ref()
        .map(|n| n.display_name.clone())
        .unwrap_or_default()
}

fn person_ref(profile: &PersonProfile) -> PersonRef {
    PersonRef {
        person_id: profile.person_id.to_string(),
        name: display_name(profile),
    }
}

/// A union: a family with at least one spouse, dated by its marriage or its
/// first dated family event.
struct Union<'a> {
    family_id: Uuid,
    spouses: Vec<(Uuid, SpouseRole)>,
    date: Option<&'a ProfileEvent>,
    /// A divorce or an annulment.
    end: Option<NaiveDate>,
    children: Vec<Uuid>,
}

/// The tree as the statistics read it.
struct Tree<'a> {
    profiles: &'a [PersonProfile],
    by_id: HashMap<Uuid, &'a PersonProfile>,
    dates: Dates,
    unions: Vec<Union<'a>>,
}

impl<'a> Tree<'a> {
    fn new(profiles: &'a [PersonProfile], dates: Dates) -> Self {
        let mut by_family: HashMap<Uuid, Union<'a>> = HashMap::new();
        for profile in profiles {
            for link in &profile.families_as_spouse {
                let union = by_family.entry(link.family_id).or_insert_with(|| {
                    let dated = |e: &&ProfileEvent| e.date_sort.is_some();
                    let date =
                        link.marriage.as_ref().filter(dated).or_else(|| {
                            link.events.iter().filter(dated).min_by_key(|e| e.date_sort)
                        });
                    let end = link
                        .events
                        .iter()
                        .filter(|e| {
                            matches!(e.event_type, EventType::Divorce | EventType::Annulment)
                        })
                        .filter_map(|e| dates.date(Some(e)))
                        .min();
                    Union {
                        family_id: link.family_id,
                        spouses: Vec::new(),
                        date,
                        end,
                        children: link.children_ids.clone(),
                    }
                });
                if !union.spouses.iter().any(|(id, _)| *id == profile.person_id) {
                    union.spouses.push((profile.person_id, link.role));
                }
            }
        }
        let mut unions: Vec<_> = by_family.into_values().collect();
        unions.sort_by_key(|u| u.family_id);
        Self {
            profiles,
            by_id: profiles.iter().map(|p| (p.person_id, p)).collect(),
            dates,
            unions,
        }
    }

    fn person(&self, id: &Uuid) -> Option<&'a PersonProfile> {
        self.by_id.get(id).copied()
    }

    fn birth(&self, profile: &PersonProfile) -> Option<(NaiveDate, Precision)> {
        self.dates.of(profile.birth_or_baptism())
    }

    fn death(&self, profile: &PersonProfile) -> Option<(NaiveDate, Precision)> {
        self.dates.of(profile.death_or_burial())
    }

    fn born(&self, id: &Uuid) -> Option<NaiveDate> {
        self.person(id)
            .and_then(|p| self.birth(p))
            .map(|(date, _)| date)
    }

    fn died(&self, id: &Uuid) -> Option<NaiveDate> {
        self.person(id)
            .and_then(|p| self.death(p))
            .map(|(date, _)| date)
    }

    /// When a union ended: the first death of a spouse, or a divorce or an
    /// annulment, whichever came first. A union with one known spouse only
    /// ends by a divorce or an annulment.
    fn union_end(&self, union: &Union<'_>) -> Option<NaiveDate> {
        let deaths: Option<Vec<NaiveDate>> =
            union.spouses.iter().map(|(id, _)| self.died(id)).collect();
        match (deaths.and_then(|d| d.into_iter().min()), union.end) {
            (Some(d), Some(e)) => Some(d.min(e)),
            (Some(d), None) if union.spouses.len() == 2 => Some(d),
            (None, Some(e)) => Some(e),
            _ => None,
        }
    }

    fn spouses(&self, union: &Union<'_>) -> Vec<PersonRef> {
        union
            .spouses
            .iter()
            .filter_map(|(id, _)| self.person(id))
            .map(person_ref)
            .collect()
    }

    /// Every parent with the persons they are a parent of: through a
    /// child's own family, and as a spouse of a union with children.
    fn children_of(&self) -> HashMap<Uuid, HashSet<Uuid>> {
        let mut children: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
        for profile in self.profiles {
            if let Some(link) = &profile.family_as_child {
                for parent in [link.father_id, link.mother_id].into_iter().flatten() {
                    children
                        .entry(parent)
                        .or_default()
                        .insert(profile.person_id);
                }
            }
        }
        for union in &self.unions {
            for (spouse, _) in &union.spouses {
                children
                    .entry(*spouse)
                    .or_default()
                    .extend(union.children.iter().copied());
            }
        }
        children.retain(|_, c| !c.is_empty());
        children
    }
}

/// Every label with how many times it comes, case aside, under its most
/// frequent spelling; most frequent first.
fn tally(values: impl IntoIterator<Item = String>) -> Vec<CountEntry> {
    let mut counts: HashMap<String, (i64, HashMap<String, i64>)> = HashMap::new();
    for value in values {
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        let slot = counts.entry(value.to_lowercase()).or_default();
        slot.0 += 1;
        *slot.1.entry(value).or_insert(0) += 1;
    }
    let mut entries: Vec<CountEntry> = counts
        .into_values()
        .map(|(count, spellings)| {
            let label = spellings
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
                .map(|(s, _)| s)
                .unwrap_or_default();
            CountEntry { label, count }
        })
        .collect();
    entries.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
    entries
}

/// The ten most frequent labels.
fn top(values: impl IntoIterator<Item = String>) -> Vec<CountEntry> {
    let mut entries = tally(values);
    entries.truncate(TOP);
    entries
}

fn first_given_name(profile: &PersonProfile) -> Option<String> {
    profile
        .primary_name
        .as_ref()
        .and_then(|n| n.given_names.as_deref())
        .and_then(|g| g.split_whitespace().next())
        .map(str::to_string)
}

/// The language a statistics request names places in: English when it
/// names none, an error for a language the interface does not offer.
pub fn language(code: Option<&str>) -> Result<ReferenceLang, oxidgene_core::OxidGeneError> {
    code.map_or(Ok(ReferenceLang::En), |code| {
        ReferenceLang::from_code(code).ok_or_else(|| {
            oxidgene_core::OxidGeneError::Validation(format!("unsupported language: {code}"))
        })
    })
}

/// Loads what a tree's statistics are computed from, and computes them off
/// the async workers: the person projections, the place usages, the source
/// count, and the place dictionary to locate places and name their country,
/// region and subdivision in `lang`.
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    profiles: &crate::profile::ProfileService,
    tree_id: Uuid,
    approximate: bool,
    lang: ReferenceLang,
) -> Result<TreeStatistics, oxidgene_core::OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let persons = profiles.get_all_persons(db, tree_id).await?;
    let places = oxidgene_db::repo::DictionaryRepo::places_with_usage(db, tree_id).await?;
    let sources = oxidgene_db::repo::SourceRepo::count_in_tree(db, tree_id).await?;
    let today = chrono::Utc::now().date_naive();
    tokio::task::spawn_blocking(move || {
        compute(
            &persons,
            &places,
            i64::try_from(sources).unwrap_or(i64::MAX),
            today,
            approximate,
            |labels| crate::reference::locate_places(lang, labels),
        )
    })
    .await
    .map_err(|e| oxidgene_core::OxidGeneError::Internal(e.to_string()))
}

/// Computes the statistics of a tree.
///
/// `locate` places the tree's place names, with how often each is used, in
/// the place dictionary; `approximate` lets ages and averages use dates
/// about, calculated or estimated.
pub fn compute(
    profiles: &[PersonProfile],
    places: &[(Place, i64)],
    sources: i64,
    today: NaiveDate,
    approximate: bool,
    locate: impl FnOnce(&[(&str, i64)]) -> Vec<PlaceLocation>,
) -> TreeStatistics {
    let tree = Tree::new(profiles, Dates { approximate });
    let unions = &tree.unions;

    // Lives: ages at death by year of death and of birth, the pyramid, and
    // the deaths of children among the births.
    let mut age_at_death = (Average::default(), Average::default());
    let mut life_expectancy = (Average::default(), Average::default());
    let mut births_by_month = Distribution::new(12);
    let mut mortality = Distribution::new(3);
    let mut pyramid: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    let mut lifespan = BySex::default();
    for profile in profiles {
        let Some((born, precision)) = tree.birth(profile) else {
            continue;
        };
        if precision >= Precision::Month {
            births_by_month.add(born, born.month0() as usize);
        }
        mortality.add(born, 0);
        let Some((died, _)) = tree.death(profile) else {
            continue;
        };
        let age = years_between(born, died);
        if age < 0.0 {
            continue;
        }
        if age < 1.0 {
            mortality.add(born, 1);
        }
        if age < 5.0 {
            mortality.add(born, 2);
        }
        lifespan.add(profile.sex, age);
        match profile.sex {
            Sex::Male => {
                age_at_death.0.add(died, age);
                life_expectancy.0.add(born, age);
            }
            Sex::Female => {
                age_at_death.1.add(died, age);
                life_expectancy.1.add(born, age);
            }
            Sex::Unknown => {}
        }
        let band = (age as i64 / PYRAMID_BAND) * PYRAMID_BAND;
        let slot = pyramid.entry(band).or_insert((0, 0));
        match profile.sex {
            Sex::Male => slot.0 += 1,
            Sex::Female => slot.1 += 1,
            Sex::Unknown => {}
        }
    }

    // Events counted by year, whatever their qualifier.
    let sorted = |event: &Option<ProfileEvent>| event.as_ref().and_then(|e| e.date_sort);
    let mut events_by_year = Distribution::new(5);
    let mut births_by_sex = Distribution::new(2);
    for profile in profiles {
        for (category, event) in [
            (0, &profile.birth),
            (1, &profile.baptism),
            (3, &profile.death),
            (4, &profile.burial),
        ] {
            if let Some(date) = sorted(event) {
                events_by_year.add(date, category);
            }
        }
        if let Some(date) = profile.birth_or_baptism().and_then(|e| e.date_sort) {
            match profile.sex {
                Sex::Male => births_by_sex.add(date, 0),
                Sex::Female => births_by_sex.add(date, 1),
                Sex::Unknown => {}
            }
        }
    }
    for union in unions {
        if let Some(date) = union.date.and_then(|e| e.date_sort) {
            events_by_year.add(date, 2);
        }
    }

    // Parents' ages at their first, last and every dated child.
    let mut parents: [Average; 6] = std::array::from_fn(|_| Average::default());
    let mut generation = Vec::new();
    let mut children_born: HashMap<(Uuid, bool), Vec<NaiveDate>> = HashMap::new();
    for profile in profiles {
        let (Some(link), Some((born, _))) = (&profile.family_as_child, tree.birth(profile)) else {
            continue;
        };
        for (parent, is_father) in [(link.father_id, true), (link.mother_id, false)] {
            if let Some(parent) = parent {
                children_born
                    .entry((parent, is_father))
                    .or_default()
                    .push(born);
            }
        }
    }
    let mut parent_keys: Vec<&(Uuid, bool)> = children_born.keys().collect();
    parent_keys.sort();
    for key @ (parent, is_father) in parent_keys {
        let births = &children_born[key];
        let Some(parent_born) = tree.born(parent) else {
            continue;
        };
        let (Some(first), Some(last)) = (births.iter().min(), births.iter().max()) else {
            continue;
        };
        let (first_slot, last_slot, every_slot) = if *is_father { (0, 2, 4) } else { (1, 3, 5) };
        parents[first_slot].add(*first, years_between(parent_born, *first));
        parents[last_slot].add(*last, years_between(parent_born, *last));
        for born in births {
            let age = years_between(parent_born, *born);
            if age > 0.0 {
                parents[every_slot].add(*born, age);
                generation.push(age);
            }
        }
    }

    // Unions.
    let mut first_union = (Average::default(), Average::default());
    let mut earliest: HashMap<Uuid, NaiveDate> = HashMap::new();
    let mut unions_by_weekday = Distribution::new(7);
    let mut unions_by_month = Distribution::new(12);
    let mut duration = Average::default();
    let mut children_per_union = Average::default();
    let mut spacing = Average::default();
    let mut first_last = Average::default();
    let mut spouse_gap = Average::default();
    let mut children_histogram: Vec<i64> = Vec::new();
    let mut family_sizes = Vec::new();
    for union in unions {
        let size = union.children.len();
        if children_histogram.len() <= size {
            children_histogram.resize(size + 1, 0);
        }
        children_histogram[size] += 1;
        family_sizes.push(size as f64);
        if let Some((date, precision)) = tree.dates.of(union.date) {
            if precision >= Precision::Month {
                unions_by_month.add(date, date.month0() as usize);
            }
            if precision == Precision::Day {
                unions_by_weekday.add(date, date.weekday().num_days_from_monday() as usize);
            }
            for (spouse, _) in &union.spouses {
                let slot = earliest.entry(*spouse).or_insert(date);
                *slot = (*slot).min(date);
            }
            children_per_union.add(date, size as f64);
            if let Some(end) = tree.union_end(union).filter(|end| *end >= date) {
                duration.add(date, years_between(date, end));
            }
            if let [(a, _), (b, _)] = union.spouses.as_slice()
                && let (Some(born_a), Some(born_b)) = (tree.born(a), tree.born(b))
            {
                spouse_gap.add(date, months_between(born_a, born_b).abs());
            }
        }
        // The children's births, in order.
        let mut born: Vec<NaiveDate> = union
            .children
            .iter()
            .filter_map(|id| tree.person(id).and_then(|c| tree.birth(c)))
            .filter(|(_, precision)| *precision >= Precision::Month)
            .map(|(d, _)| d)
            .collect();
        born.sort();
        for pair in born.windows(2) {
            spacing.add(pair[1], months_between(pair[0], pair[1]));
        }
        if let (Some(first), Some(last)) = (born.first(), born.last())
            && born.len() > 1
        {
            first_last.add(*first, months_between(*first, *last));
        }
    }
    let mut first_union_ages = BySex::default();
    let mut married: Vec<(&Uuid, &NaiveDate)> = earliest.iter().collect();
    married.sort();
    for (person, date) in married {
        let (Some(profile), Some(born)) = (tree.person(person), tree.born(person)) else {
            continue;
        };
        let age = years_between(born, *date);
        if age < 0.0 {
            continue;
        }
        first_union_ages.add(profile.sex, age);
        match profile.sex {
            Sex::Male => first_union.0.add(*date, age),
            Sex::Female => first_union.1.add(*date, age),
            Sex::Unknown => {}
        }
    }

    // Names and occupations.
    let surnames = || {
        profiles
            .iter()
            .filter_map(|p| p.primary_name.as_ref().and_then(|n| n.surname.clone()))
    };
    let given = |sex: Option<Sex>| {
        profiles
            .iter()
            .filter(move |p| sex.is_none_or(|s| p.sex == s))
            .filter_map(first_given_name)
    };
    let top_occupations = top(profiles.iter().flat_map(|p| {
        let mut seen = HashSet::new();
        p.other_events
            .iter()
            .filter(|e| e.event_type == EventType::Occupation)
            .filter_map(|e| e.description.clone())
            .chain(p.occupation.clone())
            .filter(move |o| seen.insert(o.trim().to_lowercase()))
            .collect::<Vec<_>>()
    }));

    // Event types, each family event once however many spouses carry it.
    let mut types: HashMap<EventType, i64> = HashMap::new();
    let mut family_events = HashSet::new();
    let mut years = (i32::MAX, i32::MIN);
    for profile in profiles {
        let own = [
            &profile.birth,
            &profile.baptism,
            &profile.death,
            &profile.burial,
        ]
        .into_iter()
        .flatten()
        .chain(&profile.other_events);
        let shared = profile
            .families_as_spouse
            .iter()
            .flat_map(|link| link.marriage.iter().chain(&link.events))
            .filter(|e| family_events.insert(e.event_id));
        for event in own.chain(shared.collect::<Vec<_>>()) {
            *types.entry(event.event_type).or_default() += 1;
            if let Some(date) = event.date_sort {
                years = (years.0.min(date.year()), years.1.max(date.year()));
            }
        }
    }
    let mut event_types: Vec<CountEntry> = types
        .into_iter()
        .map(|(kind, count)| CountEntry {
            label: kind.to_string(),
            count,
        })
        .collect();
    event_types.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));

    // Who has parents, children and a spouse.
    let children_of = tree.children_of();
    let count =
        |keep: &dyn Fn(&PersonProfile) -> bool| profiles.iter().filter(|p| keep(p)).count() as i64;

    // Lists.
    let person_record =
        |p: &PersonProfile, event: Option<&ProfileEvent>, age: Option<i64>| PersonRecord {
            person_id: p.person_id.to_string(),
            name: display_name(p),
            sex: p.sex.to_string(),
            date: event.map(record_date),
            place: event.and_then(|e| e.place_name.clone()),
            birth: p.birth_or_baptism().map(record_date),
            death: p.death_or_burial().map(record_date),
            age,
        };
    let latest = |event: fn(&PersonProfile) -> Option<&ProfileEvent>| {
        let mut dated: Vec<&PersonProfile> = profiles
            .iter()
            .filter(|p| event(p).and_then(|e| e.date_sort).is_some())
            .collect();
        dated.sort_by_key(|p| std::cmp::Reverse(event(p).and_then(|e| e.date_sort)));
        dated
            .into_iter()
            .take(LIST)
            .map(|p| person_record(p, event(p), None))
            .collect::<Vec<_>>()
    };
    let recent_births = latest(PersonProfile::birth_or_baptism);
    let recent_deaths = latest(PersonProfile::death_or_burial);

    let horizon = today.year() - POSSIBLY_ALIVE_YEARS;
    let mut alive: Vec<(&PersonProfile, NaiveDate)> = profiles
        .iter()
        .filter(|p| p.death.is_none() && p.burial.is_none())
        .filter_map(|p| {
            p.birth_or_baptism()
                .and_then(|e| e.date_sort)
                .map(|d| (p, d))
        })
        .filter(|(_, born)| born.year() > horizon && *born <= today)
        .collect();
    alive.sort_by_key(|(p, born)| (*born, p.person_id));
    let oldest_possibly_alive = alive
        .into_iter()
        .take(LIST)
        .map(|(p, born)| {
            person_record(
                p,
                p.birth_or_baptism(),
                Some(years_between(born, today) as i64),
            )
        })
        .collect();

    let mut lives: Vec<(&PersonProfile, f64)> = profiles
        .iter()
        .filter_map(|p| {
            let (Some((born, _)), Some((died, _))) = (tree.birth(p), tree.death(p)) else {
                return None;
            };
            let age = years_between(born, died);
            (age >= 0.0).then_some((p, age))
        })
        .collect();
    lives.sort_by(|a, b| {
        b.1.total_cmp(&a.1)
            .then_with(|| a.0.person_id.cmp(&b.0.person_id))
    });
    let longest_lives = lives
        .into_iter()
        .take(LIST)
        .map(|(p, age)| person_record(p, p.death_or_burial(), Some(age as i64)))
        .collect();

    let mut dated_unions: Vec<&Union<'_>> = unions
        .iter()
        .filter(|u| u.date.and_then(|d| d.date_sort).is_some())
        .collect();
    dated_unions.sort_by_key(|u| std::cmp::Reverse(u.date.and_then(|d| d.date_sort)));
    let recent_unions = dated_unions
        .into_iter()
        .take(LIST)
        .filter_map(|u| {
            let date = u.date?;
            Some(UnionRecord {
                family_id: u.family_id.to_string(),
                spouses: tree.spouses(u),
                date: record_date(date),
                place: date.place_name.clone(),
            })
        })
        .collect();

    let mut large: Vec<&Union<'_>> = unions.iter().filter(|u| !u.children.is_empty()).collect();
    large.sort_by_key(|u| std::cmp::Reverse(u.children.len()));
    let largest_families = large
        .into_iter()
        .take(LIST)
        .map(|u| FamilyRecord {
            family_id: u.family_id.to_string(),
            spouses: tree.spouses(u),
            children: u.children.len() as i64,
            date: u.date.map(record_date),
        })
        .collect();

    // Places.
    let used: Vec<&(Place, i64)> = places.iter().filter(|(_, count)| *count > 0).collect();
    let labels: Vec<(&str, i64)> = used
        .iter()
        .map(|(place, count)| (place.name.as_str(), *count))
        .collect();
    let locations = locate(&labels);
    let location_of: HashMap<Uuid, &PlaceLocation> = used
        .iter()
        .zip(&locations)
        .map(|((place, _), location)| (place.id, location))
        .collect();
    let mut used_places: Vec<PlaceUsage> = used
        .iter()
        .zip(&locations)
        .map(|((place, count), found)| {
            let coordinates = place.latitude.zip(place.longitude).or(found.spot);
            PlaceUsage {
                place_id: place.id.to_string(),
                name: place.name.clone(),
                count: *count,
                latitude: coordinates.map(|c| c.0),
                longitude: coordinates.map(|c| c.1),
            }
        })
        .collect();
    used_places.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    let top_places = used_places.iter().take(TOP).cloned().collect();
    let unlocated_places = used_places.iter().filter(|p| p.latitude.is_none()).count() as i64;
    let places_count = used_places.len() as i64;
    let located_places = used_places
        .into_iter()
        .filter(|p| p.latitude.is_some())
        .collect();
    // A region is told apart by its country, a subdivision by its region.
    let distinct = |part: fn(&PlaceLocation) -> Option<(&str, &str, &str)>| {
        locations
            .iter()
            .filter_map(part)
            .collect::<HashSet<_>>()
            .len() as i64
    };
    let birth_places = |part: fn(&PlaceLocation) -> Option<&String>| {
        top(profiles
            .iter()
            .filter_map(|p| p.birth_or_baptism()?.place_id)
            .filter_map(|id| part(location_of.get(&id)?).cloned()))
    };

    let count_sex = |sex: Sex| profiles.iter().filter(|p| p.sex == sex).count() as i64;
    let [
        father_first,
        mother_first,
        father_last,
        mother_last,
        father_every,
        mother_every,
    ] = parents;
    TreeStatistics {
        persons: profiles.len() as i64,
        men: count_sex(Sex::Male),
        women: count_sex(Sex::Female),
        unknown_sex: count_sex(Sex::Unknown),
        unions: unions.len() as i64,
        places: places_count,
        sources,
        first_year: (years.0 <= years.1).then_some(years.0),
        last_year: (years.0 <= years.1).then_some(years.1),
        dated_births: count(&|p| p.birth_or_baptism().and_then(|e| e.date_sort).is_some()),
        dated_deaths: count(&|p| p.death_or_burial().and_then(|e| e.date_sort).is_some()),
        without_parents: count(&|p| {
            p.family_as_child
                .as_ref()
                .is_none_or(|l| l.father_id.is_none() && l.mother_id.is_none())
        }),
        without_children: count(&|p| !children_of.contains_key(&p.person_id)),
        without_union: count(&|p| p.families_as_spouse.is_empty()),
        surnames: tally(surnames()).len() as i64,
        given_names: tally(given(None)).len() as i64,
        lifespan: lifespan.summary(),
        first_union_age: first_union_ages.summary(),
        generation_interval: summary(generation),
        family_size: summary(family_sizes),
        top_surnames: top(surnames()),
        top_given_names_men: top(given(Some(Sex::Male))),
        top_given_names_women: top(given(Some(Sex::Female))),
        top_occupations,
        event_types,
        children_histogram,
        events_by_year: events_by_year.years(),
        births_by_sex: births_by_sex.years(),
        mortality: mortality.years(),
        age_at_death: SexSeries {
            men: age_at_death.0.years(),
            women: age_at_death.1.years(),
        },
        life_expectancy: SexSeries {
            men: life_expectancy.0.years(),
            women: life_expectancy.1.years(),
        },
        births_by_month: births_by_month.years(),
        parents_age: ParentAgeSeries {
            father_first_child: father_first.years(),
            mother_first_child: mother_first.years(),
            father_last_child: father_last.years(),
            mother_last_child: mother_last.years(),
            father_every_child: father_every.years(),
            mother_every_child: mother_every.years(),
        },
        age_at_first_union: SexSeries {
            men: first_union.0.years(),
            women: first_union.1.years(),
        },
        unions_by_weekday: unions_by_weekday.years(),
        unions_by_month: unions_by_month.years(),
        union_duration: duration.years(),
        children_per_union: children_per_union.years(),
        birth_spacing: spacing.years(),
        first_last_child_gap: first_last.years(),
        spouse_age_gap: spouse_gap.years(),
        pyramid: pyramid
            .into_iter()
            .map(|(from, (men, women))| PyramidBand { from, men, women })
            .collect(),
        records: records::records(&tree, &children_of),
        recent_births,
        recent_deaths,
        recent_unions,
        oldest_possibly_alive,
        longest_lives,
        largest_families,
        located_places,
        top_places,
        unlocated_places,
        countries: distinct(|l| Some((l.country.as_deref()?, "", ""))),
        regions: distinct(|l| Some((l.country.as_deref()?, l.region.as_deref()?, ""))),
        subdivisions: distinct(|l| {
            Some((
                l.country.as_deref()?,
                l.region.as_deref().unwrap_or_default(),
                l.subdivision.as_deref()?,
            ))
        }),
        births_by_country: birth_places(|l| l.country.as_ref()),
        births_by_region: birth_places(|l| l.region.as_ref()),
        births_by_subdivision: birth_places(|l| l.subdivision.as_ref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use oxidgene_core::projection::{ProfileChildLink, ProfileFamilyLink, ProfileName};
    use oxidgene_core::{Calendar, ChildType, NameType};
    use uuid::Uuid;

    fn event(kind: EventType, value: &str) -> ProfileEvent {
        let date_sort = oxidgene_gedcom::date::sort_key(Calendar::Gregorian, Some(value));
        ProfileEvent {
            event_id: Uuid::now_v7(),
            event_type: kind,
            date_value: Some(value.to_string()),
            date_sort,
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            place_name: Some("Place A".to_string()),
            place_id: None,
            description: None,
        }
    }

    fn person(
        sex: Sex,
        given: &str,
        surname: &str,
        born: Option<&str>,
        died: Option<&str>,
    ) -> PersonProfile {
        PersonProfile {
            person_id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            sex,
            primary_name: Some(ProfileName {
                name_id: Uuid::now_v7(),
                name_type: NameType::Birth,
                display_name: format!("{given} {surname}"),
                given_names: Some(given.to_string()),
                surname: Some(surname.to_string()),
            }),
            other_names: vec![],
            birth: born.map(|d| event(EventType::Birth, d)),
            death: died.map(|d| event(EventType::Death, d)),
            baptism: None,
            burial: None,
            occupation: None,
            other_events: vec![],
            families_as_spouse: vec![],
            family_as_child: None,
            primary_media: None,
            media_count: 0,
            citation_count: 0,
            note_count: 0,
            updated_at: Utc::now(),
            built_at: Utc::now(),
        }
    }

    fn spouse_link(
        family: Uuid,
        role: SpouseRole,
        marriage: &str,
        children: &[Uuid],
    ) -> ProfileFamilyLink {
        ProfileFamilyLink {
            family_id: family,
            role,
            spouse_id: None,
            spouse_display_name: None,
            spouse_surname: None,
            spouse_given_names: None,
            spouse_sex: None,
            marriage: Some(event(EventType::Marriage, marriage)),
            events: vec![],
            children_ids: children.to_vec(),
            children_count: children.len() as u32,
        }
    }

    /// A fictitious family: a couple married on a Monday in 1850, two
    /// children two years apart, both parents dead.
    fn family() -> Vec<PersonProfile> {
        let family = Uuid::now_v7();
        let mut father = person(
            Sex::Male,
            "Jean Paul",
            "BRANCH_A",
            Some("3 MAR 1820"),
            Some("3 MAR 1890"),
        );
        let mut mother = person(
            Sex::Female,
            "Anne",
            "BRANCH_B",
            Some("1 JUN 1825"),
            Some("1 JUN 1885"),
        );
        let mut first = person(Sex::Male, "Jean", "BRANCH_A", Some("15 JAN 1852"), None);
        let mut second = person(Sex::Female, "Marie", "BRANCH_A", Some("15 JAN 1854"), None);
        for child in [&mut first, &mut second] {
            child.family_as_child = Some(ProfileChildLink {
                family_id: family,
                child_type: ChildType::Biological,
                father_id: Some(father.person_id),
                father_display_name: None,
                father_surname: None,
                father_given_names: None,
                mother_id: Some(mother.person_id),
                mother_display_name: None,
                mother_surname: None,
                mother_given_names: None,
            });
        }
        let children = [first.person_id, second.person_id];
        // 4 February 1850 was a Monday.
        father.families_as_spouse = vec![spouse_link(
            family,
            SpouseRole::Husband,
            "4 FEB 1850",
            &children,
        )];
        // Both spouses carry the same marriage event.
        let mut link = father.families_as_spouse[0].clone();
        link.role = SpouseRole::Wife;
        mother.families_as_spouse = vec![link];
        vec![father, mother, first, second]
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
    }

    fn nowhere(labels: &[(&str, i64)]) -> Vec<PlaceLocation> {
        vec![PlaceLocation::default(); labels.len()]
    }

    fn stats(people: &[PersonProfile]) -> TreeStatistics {
        compute(people, &[], 0, today(), false, nowhere)
    }

    fn record<'a>(stats: &'a TreeStatistics, kind: &str) -> &'a StatRecord {
        stats
            .records
            .iter()
            .find(|r| r.kind == kind)
            .unwrap_or_else(|| panic!("no {kind} record"))
    }

    /// A year's average, to the tenth the page shows.
    fn average(series: &[YearSum], year: i32) -> Option<f64> {
        series
            .iter()
            .find(|y| y.year == year)
            .map(|y| (y.sum / y.count as f64 * 10.0).round() / 10.0)
    }

    fn counts(series: &[YearCounts], year: i32) -> Option<&[i64]> {
        series
            .iter()
            .find(|y| y.year == year)
            .map(|y| y.counts.as_slice())
    }

    #[test]
    fn averages_are_filed_by_the_year_they_depend_on() {
        let stats = stats(&family());
        assert_eq!(average(&stats.age_at_death.men, 1890), Some(70.0));
        assert_eq!(average(&stats.age_at_death.women, 1885), Some(60.0));
        assert_eq!(average(&stats.age_at_first_union.men, 1850), Some(29.9));
        assert_eq!(average(&stats.children_per_union, 1850), Some(2.0));
        // Spacing is filed by the later of the two births.
        assert_eq!(average(&stats.birth_spacing, 1854), Some(24.0));
        assert_eq!(average(&stats.birth_spacing, 1852), None);
        assert_eq!(average(&stats.union_duration, 1850), Some(35.3));
        assert_eq!(
            average(&stats.parents_age.father_first_child, 1852),
            Some(31.9)
        );
        assert_eq!(
            average(&stats.parents_age.mother_last_child, 1854),
            Some(28.6)
        );
        // Years come oldest first, each once.
        let years: Vec<i32> = stats.age_at_death.men.iter().map(|y| y.year).collect();
        assert_eq!(years, vec![1890]);
    }

    #[test]
    fn shares_need_the_precision_they_count() {
        let mut people = family();
        // A birth known to the year only says nothing of its month.
        people.push(person(Sex::Male, "Paul", "BRANCH_C", Some("1853"), None));
        let stats = stats(&people);
        assert_eq!(counts(&stats.births_by_month, 1853), None);
        assert_eq!(counts(&stats.births_by_month, 1852).unwrap()[0], 1);
        assert_eq!(counts(&stats.unions_by_weekday, 1850).unwrap()[0], 1);
        assert_eq!(counts(&stats.unions_by_month, 1850).unwrap()[1], 1);
    }

    #[test]
    fn names_are_ranked_by_how_many_persons_carry_them() {
        let stats = stats(&family());
        assert_eq!(
            stats.top_surnames[0],
            CountEntry {
                label: "BRANCH_A".to_string(),
                count: 3
            }
        );
        // The first given name only.
        assert!(
            stats
                .top_given_names_men
                .iter()
                .any(|e| e.label == "Jean" && e.count == 2)
        );
        assert_eq!(stats.top_given_names_women.len(), 2);
        assert_eq!((stats.surnames, stats.given_names), (2, 3));
    }

    #[test]
    fn lists_and_places_follow_the_rules() {
        let mut people = family();
        people.push(person(
            Sex::Female,
            "Rose",
            "BRANCH_D",
            Some("2 MAY 1910"),
            None,
        ));
        people.push(person(
            Sex::Female,
            "Lise",
            "BRANCH_D",
            Some("2 MAY 1890"),
            None,
        ));
        let place = |name: &str, lat: Option<f64>| Place {
            id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            name: name.to_string(),
            latitude: lat,
            longitude: lat,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let places = vec![
            (place("Place A", Some(45.0)), 3),
            (place("Place B", None), 5),
            (place("Place C", None), 1),
            (place("Place D", None), 0),
        ];
        let stats = compute(&people, &places, 0, today(), false, |labels| {
            labels
                .iter()
                .map(|(name, _)| PlaceLocation {
                    spot: (*name == "Place B").then_some((48.0, 2.0)),
                    ..PlaceLocation::default()
                })
                .collect()
        });
        // Born 115 years ago: possibly alive; born 135 years ago: not.
        assert_eq!(stats.oldest_possibly_alive.len(), 1);
        assert_eq!(stats.oldest_possibly_alive[0].age, Some(115));
        assert_eq!(stats.longest_lives[0].age, Some(70));
        assert_eq!(stats.recent_unions.len(), 1);
        assert_eq!(stats.recent_unions[0].spouses.len(), 2);
        assert_eq!(stats.places, 3);
        assert_eq!(stats.top_places[0].name, "Place B");
        assert_eq!(stats.located_places.len(), 2);
        assert_eq!(stats.unlocated_places, 1);
    }

    #[test]
    fn dates_travel_in_their_serialized_forms() {
        let date = record_date(&event(EventType::Birth, "1 JAN 1900"));
        assert_eq!(
            date.qualifier,
            serde_json::to_value(DateQualifier::Exact).unwrap()
        );
        assert_eq!(
            date.calendar,
            serde_json::to_value(Calendar::Gregorian).unwrap()
        );
    }

    #[test]
    fn the_overview_counts_the_tree() {
        let stats = stats(&family());
        assert_eq!((stats.persons, stats.men, stats.women), (4, 2, 2));
        assert_eq!(stats.unknown_sex, 0);
        assert_eq!(
            (stats.first_year, stats.last_year),
            (Some(1820), Some(1890))
        );
        assert_eq!((stats.dated_births, stats.dated_deaths), (4, 2));
        // The parents have no parents, the children no children and no spouse.
        assert_eq!(stats.without_parents, 2);
        assert_eq!(stats.without_children, 2);
        assert_eq!(stats.without_union, 2);
        assert_eq!(stats.lifespan.all.mean, Some(65.0));
        assert_eq!(stats.lifespan.men.max, Some(70.0));
        assert_eq!(stats.lifespan.all.median, Some(65.0));
        assert_eq!(stats.lifespan.all.std_dev, Some(5.0));
        assert_eq!(stats.first_union_age.women.count, 1);
        // Father at 31.9 and 33.9, mother at 26.6 and 28.6.
        assert_eq!(stats.generation_interval.count, 4);
        assert_eq!(stats.generation_interval.min, Some(26.6));
        assert_eq!(stats.family_size.mean, Some(2.0));
        assert_eq!(stats.children_histogram, vec![0, 0, 1]);
    }

    #[test]
    fn events_are_counted_by_type_and_by_year() {
        let stats = stats(&family());
        let count = |label: &str| {
            stats
                .event_types
                .iter()
                .find(|e| e.label == label)
                .map(|e| e.count)
        };
        assert_eq!(count("birth"), Some(4));
        assert_eq!(count("death"), Some(2));
        // Both spouses carry the marriage: it counts once.
        assert_eq!(count("marriage"), Some(1));
        assert_eq!(
            counts(&stats.events_by_year, 1850).unwrap(),
            &[0, 0, 1, 0, 0]
        );
        assert_eq!(
            counts(&stats.events_by_year, 1890).unwrap(),
            &[0, 0, 0, 1, 0]
        );
        assert_eq!(counts(&stats.births_by_sex, 1852).unwrap(), &[1, 0]);
        assert_eq!(
            average(&stats.parents_age.father_every_child, 1854),
            Some(33.9)
        );
        assert_eq!(average(&stats.life_expectancy.men, 1820), Some(70.0));
    }

    #[test]
    fn children_dying_young_are_counted_by_their_birth_year() {
        let mut people = family();
        people.push(person(
            Sex::Female,
            "Rose",
            "BRANCH_C",
            Some("1 JAN 1860"),
            Some("1 JUN 1860"),
        ));
        people.push(person(
            Sex::Male,
            "Paul",
            "BRANCH_C",
            Some("1 MAR 1860"),
            Some("1 MAR 1863"),
        ));
        people.push(person(
            Sex::Male,
            "Luc",
            "BRANCH_C",
            Some("1 MAY 1860"),
            None,
        ));
        let stats = stats(&people);
        // Three births; one death before one year, two before five.
        assert_eq!(counts(&stats.mortality, 1860).unwrap(), &[3, 1, 2]);
    }

    #[test]
    fn approximate_dates_count_only_when_asked() {
        let mut people = family();
        let mut guessed = person(Sex::Male, "Paul", "BRANCH_C", Some("1800"), Some("1850"));
        for event in [&mut guessed.birth, &mut guessed.death]
            .into_iter()
            .flatten()
        {
            event.date_qualifier = DateQualifier::About;
        }
        people.push(guessed);
        let exact = compute(&people, &[], 0, today(), false, nowhere);
        let approximate = compute(&people, &[], 0, today(), true, nowhere);
        assert_eq!(exact.lifespan.men.count, 1);
        assert_eq!(approximate.lifespan.men.count, 2);
        // Counting events by year takes every dated one either way.
        assert_eq!(counts(&exact.events_by_year, 1800).unwrap()[0], 1);
    }

    #[test]
    fn births_are_placed_in_their_country_region_and_subdivision() {
        let mut people = family();
        let place = Place {
            id: Uuid::now_v7(),
            tree_id: Uuid::nil(),
            name: "Place A".to_string(),
            latitude: None,
            longitude: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        for p in &mut people {
            if let Some(birth) = &mut p.birth {
                birth.place_id = Some(place.id);
            }
        }
        let stats = compute(&people, &[(place, 4)], 0, today(), false, |labels| {
            vec![
                PlaceLocation {
                    spot: Some((47.0, -1.0)),
                    country: Some("Country A".to_string()),
                    region: Some("Region A".to_string()),
                    subdivision: Some("Subdivision A".to_string()),
                };
                labels.len()
            ]
        });
        assert_eq!(
            stats.births_by_country,
            vec![CountEntry {
                label: "Country A".to_string(),
                count: 4
            }]
        );
        assert_eq!(stats.births_by_subdivision[0].count, 4);
        assert_eq!(
            (stats.countries, stats.regions, stats.subdivisions),
            (1, 1, 1)
        );
    }

    #[test]
    fn records_name_who_holds_them() {
        let stats = stats(&family());
        let father = record(&stats, "longest_life_man");
        assert_eq!(father.persons[0].name, "Jean Paul BRANCH_A");
        assert_eq!(father.value, Some(25567.0));
        assert_eq!(
            record(&stats, "longest_life_woman").persons[0].name,
            "Anne BRANCH_B"
        );
        assert_eq!(
            record(&stats, "earliest_birth").persons[0].name,
            "Jean Paul BRANCH_A"
        );
        assert_eq!(
            record(&stats, "latest_birth").persons[0].name,
            "Marie BRANCH_A"
        );
        assert_eq!(record(&stats, "first_union").persons.len(), 2);
        // Married 4 February 1850, parted by the mother's death in 1885.
        assert_eq!(record(&stats, "longest_union").value, Some(12901.0));
        let widowhood = record(&stats, "longest_widowhood");
        assert_eq!(widowhood.persons[0].name, "Jean Paul BRANCH_A");
        assert_eq!(widowhood.value, Some(1736.0));
        assert_eq!(record(&stats, "largest_sibling_gap").value, Some(731.0));
        let most = record(&stats, "most_children");
        assert_eq!((most.value, most.value2), (Some(2.0), Some(1)));
        assert_eq!(record(&stats, "most_generations").value, Some(1.0));
        assert_eq!(
            record(&stats, "youngest_first_child").persons[0].name,
            "Anne BRANCH_B"
        );
        // Nobody had two unions.
        assert!(stats.records.iter().all(|r| r.kind != "most_unions"));
    }

    #[test]
    fn a_spouse_dead_before_the_union_is_no_widowhood() {
        let mut people = family();
        // The mother's death moved before the marriage of 1850.
        people[1].death = Some(event(EventType::Death, "1 JUN 1845"));
        let stats = stats(&people);
        assert!(stats.records.iter().all(|r| r.kind != "longest_widowhood"));
    }

    #[test]
    fn descendants_are_counted_in_generations() {
        let mut people = family();
        let family_id = Uuid::now_v7();
        let parent = people[2].person_id;
        let mut grandchild = person(Sex::Female, "Lise", "BRANCH_A", Some("2 MAY 1880"), None);
        grandchild.family_as_child = Some(ProfileChildLink {
            family_id,
            child_type: ChildType::Biological,
            father_id: Some(parent),
            father_display_name: None,
            father_surname: None,
            father_given_names: None,
            mother_id: None,
            mother_display_name: None,
            mother_surname: None,
            mother_given_names: None,
        });
        people.push(grandchild);
        let stats = stats(&people);
        assert_eq!(record(&stats, "most_generations").value, Some(2.0));
    }
}
