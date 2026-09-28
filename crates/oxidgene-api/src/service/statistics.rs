//! Tree statistics: the figures of the Statistics page (`docs/ui-statistics.md`),
//! computed on each request from the person projections and the place usages.
//!
//! Nothing here is stored. Every average is over the records whose dates are
//! good enough for it (§5 of the specification): exact dates only, precise
//! to the month for month shares and to the day for weekday shares.
//!
//! Time series are filed by year, as sums and counts, so the client groups
//! them into periods of any width over any range of years without asking
//! again.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use oxidgene_core::types::Place;
use oxidgene_core::{DateQualifier, EventType, Sex, SpouseRole};
use serde::Serialize;

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
    pub unions: i64,
    /// Places named by at least one event or media.
    pub places: i64,
    pub top_surnames: Vec<CountEntry>,
    pub top_given_names: Vec<CountEntry>,
    pub top_occupations: Vec<CountEntry>,
    pub age_at_death: SexSeries,
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
    pub recent_births: Vec<PersonRecord>,
    pub recent_deaths: Vec<PersonRecord>,
    pub recent_unions: Vec<UnionRecord>,
    pub oldest_possibly_alive: Vec<PersonRecord>,
    pub longest_lives: Vec<PersonRecord>,
    /// The used places that could be located, for the heat map.
    pub located_places: Vec<PlaceUsage>,
    /// The ten most used places, located or not.
    pub top_places: Vec<PlaceUsage>,
    /// Used places that could not be located.
    pub unlocated_places: i64,
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

/// One year's counts per category (month or weekday).
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
    pub spouses: Vec<UnionSpouse>,
    pub date: RecordDate,
    pub place: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct UnionSpouse {
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

/// How precise an exact date is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Precision {
    Year,
    Month,
    Day,
}

/// An event's date when it is good enough to compute with: exact, with a
/// sort key, and its precision read from how many parts the value has.
fn exact(event: Option<&ProfileEvent>) -> Option<(NaiveDate, Precision)> {
    let event = event?;
    if event.date_qualifier != DateQualifier::Exact {
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

fn years_between(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64 / 365.2425
}

fn months_between(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64 / 30.436875
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

/// Counts per year and category, for shares.
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

/// A union: a family with at least one spouse, dated by its marriage or its
/// first dated family event.
struct Union<'a> {
    family_id: uuid::Uuid,
    spouses: Vec<(uuid::Uuid, SpouseRole)>,
    date: Option<&'a ProfileEvent>,
    end: Option<NaiveDate>,
    children: Vec<uuid::Uuid>,
}

fn unions(profiles: &[PersonProfile]) -> Vec<Union<'_>> {
    let mut by_family: HashMap<uuid::Uuid, Union<'_>> = HashMap::new();
    for profile in profiles {
        for link in &profile.families_as_spouse {
            let union = by_family.entry(link.family_id).or_insert_with(|| {
                let dated = |e: &&ProfileEvent| e.date_sort.is_some();
                let date = link
                    .marriage
                    .as_ref()
                    .filter(dated)
                    .or_else(|| link.events.iter().filter(dated).min_by_key(|e| e.date_sort));
                // A divorce or an annulment ends it before any death does.
                let end = link
                    .events
                    .iter()
                    .filter(|e| matches!(e.event_type, EventType::Divorce | EventType::Annulment))
                    .filter_map(|e| exact(Some(e)).map(|(d, _)| d))
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
    unions
}

/// Ranks labels by how many persons carry them, keeping the most frequent
/// spelling of each case-insensitive label.
fn top(values: impl IntoIterator<Item = String>) -> Vec<CountEntry> {
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
    entries.truncate(TOP);
    entries
}

/// Loads what a tree's statistics are computed from, and computes them off
/// the async workers: the person projections, the place usages, and the
/// place dictionary to locate places without coordinates.
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    profiles: &crate::profile::ProfileService,
    tree_id: uuid::Uuid,
) -> Result<TreeStatistics, oxidgene_core::OxidGeneError> {
    oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let persons = profiles.get_all_persons(db, tree_id).await?;
    let places = oxidgene_db::repo::DictionaryRepo::places_with_usage(db, tree_id).await?;
    let today = chrono::Utc::now().date_naive();
    tokio::task::spawn_blocking(move || {
        compute(&persons, &places, today, crate::reference::locate_places)
    })
    .await
    .map_err(|e| oxidgene_core::OxidGeneError::Internal(e.to_string()))
}

/// Computes the statistics of a tree.
///
/// `locate` gives coordinates for the tree's place names, with how often each
/// is used, for the places without their own.
pub fn compute(
    profiles: &[PersonProfile],
    places: &[(Place, i64)],
    today: NaiveDate,
    locate: impl FnOnce(&[(&str, i64)]) -> Vec<Option<(f64, f64)>>,
) -> TreeStatistics {
    let by_id: HashMap<uuid::Uuid, &PersonProfile> =
        profiles.iter().map(|p| (p.person_id, p)).collect();
    let birth = |p: &PersonProfile| exact(p.birth_or_baptism());
    let death = |p: &PersonProfile| exact(p.death_or_burial());

    let mut age_at_death = (Average::default(), Average::default());
    let mut births_by_month = Distribution::new(12);
    let mut pyramid: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    for profile in profiles {
        if let Some((born, precision)) = birth(profile)
            && precision >= Precision::Month
        {
            births_by_month.add(born, born.month0() as usize);
        }
        let (Some((born, _)), Some((died, _))) = (birth(profile), death(profile)) else {
            continue;
        };
        let age = years_between(born, died);
        if age < 0.0 {
            continue;
        }
        match profile.sex {
            Sex::Male => age_at_death.0.add(died, age),
            Sex::Female => age_at_death.1.add(died, age),
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

    // Parents' ages at their first and last dated child.
    let mut parents = [
        Average::default(),
        Average::default(),
        Average::default(),
        Average::default(),
    ];
    let mut children_of: HashMap<(uuid::Uuid, bool), Vec<NaiveDate>> = HashMap::new();
    for profile in profiles {
        let (Some(link), Some((born, _))) = (&profile.family_as_child, birth(profile)) else {
            continue;
        };
        for (parent, is_father) in [(link.father_id, true), (link.mother_id, false)] {
            if let Some(parent) = parent {
                children_of
                    .entry((parent, is_father))
                    .or_default()
                    .push(born);
            }
        }
    }
    for ((parent, is_father), births) in &children_of {
        let Some((parent_born, _)) = by_id.get(parent).and_then(|p| birth(p)) else {
            continue;
        };
        let (Some(first), Some(last)) = (births.iter().min(), births.iter().max()) else {
            continue;
        };
        let (first_slot, last_slot) = if *is_father { (0, 2) } else { (1, 3) };
        parents[first_slot].add(*first, years_between(parent_born, *first));
        parents[last_slot].add(*last, years_between(parent_born, *last));
    }

    let unions = unions(profiles);
    let mut first_union = (Average::default(), Average::default());
    let mut earliest: HashMap<uuid::Uuid, NaiveDate> = HashMap::new();
    let mut unions_by_weekday = Distribution::new(7);
    let mut unions_by_month = Distribution::new(12);
    let mut duration = Average::default();
    let mut children_per_union = Average::default();
    let mut spacing = Average::default();
    let mut first_last = Average::default();
    let mut spouse_gap = Average::default();
    for union in &unions {
        let dated = exact(union.date);
        if let Some((date, precision)) = dated {
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
            children_per_union.add(date, union.children.len() as f64);
            // Duration: to the first death of a spouse or the divorce.
            let deaths: Option<Vec<NaiveDate>> = union
                .spouses
                .iter()
                .map(|(id, _)| by_id.get(id).and_then(|p| death(p)).map(|(d, _)| d))
                .collect();
            let end = match (deaths.and_then(|d| d.into_iter().min()), union.end) {
                (Some(d), Some(e)) => Some(d.min(e)),
                (Some(d), None) if union.spouses.len() == 2 => Some(d),
                (None, Some(e)) => Some(e),
                _ => None,
            };
            if let Some(end) = end.filter(|end| *end >= date) {
                duration.add(date, years_between(date, end));
            }
            if let [(a, _), (b, _)] = union.spouses.as_slice()
                && let (Some((born_a, _)), Some((born_b, _))) = (
                    by_id.get(a).and_then(|x| birth(x)),
                    by_id.get(b).and_then(|x| birth(x)),
                )
            {
                spouse_gap.add(date, months_between(born_a, born_b).abs());
            }
        }
        // The children's births, in order.
        let mut born: Vec<NaiveDate> = union
            .children
            .iter()
            .filter_map(|id| by_id.get(id).and_then(|c| birth(c)))
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
    for (person, date) in &earliest {
        let Some(profile) = by_id.get(person) else {
            continue;
        };
        let Some((born, _)) = birth(profile) else {
            continue;
        };
        let age = years_between(born, *date);
        if age < 0.0 {
            continue;
        }
        match profile.sex {
            Sex::Male => first_union.0.add(*date, age),
            Sex::Female => first_union.1.add(*date, age),
            Sex::Unknown => {}
        }
    }

    // Names and occupations.
    let top_surnames = top(profiles
        .iter()
        .filter_map(|p| p.primary_name.as_ref().and_then(|n| n.surname.clone())));
    let top_given_names = top(profiles.iter().filter_map(|p| {
        p.primary_name
            .as_ref()
            .and_then(|n| n.given_names.as_deref())
            .and_then(|g| g.split_whitespace().next())
            .map(str::to_string)
    }));
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
            let (Some((born, _)), Some((died, _))) = (birth(p), death(p)) else {
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
                spouses: u
                    .spouses
                    .iter()
                    .filter_map(|(id, _)| by_id.get(id))
                    .map(|p| UnionSpouse {
                        person_id: p.person_id.to_string(),
                        name: display_name(p),
                    })
                    .collect(),
                date: record_date(date),
                place: date.place_name.clone(),
            })
        })
        .collect();

    // Places.
    let used: Vec<&(Place, i64)> = places.iter().filter(|(_, count)| *count > 0).collect();
    let labels: Vec<(&str, i64)> = used
        .iter()
        .map(|(place, count)| (place.name.as_str(), *count))
        .collect();
    let mut used_places: Vec<PlaceUsage> = used
        .iter()
        .zip(locate(&labels))
        .map(|((place, count), found)| {
            let coordinates = place.latitude.zip(place.longitude).or(found);
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

    let count_sex = |sex: Sex| profiles.iter().filter(|p| p.sex == sex).count() as i64;
    let [father_first, mother_first, father_last, mother_last] = parents;
    TreeStatistics {
        persons: profiles.len() as i64,
        men: count_sex(Sex::Male),
        women: count_sex(Sex::Female),
        unions: unions.len() as i64,
        places: places_count,
        top_surnames,
        top_given_names,
        top_occupations,
        age_at_death: SexSeries {
            men: age_at_death.0.years(),
            women: age_at_death.1.years(),
        },
        births_by_month: births_by_month.years(),
        parents_age: ParentAgeSeries {
            father_first_child: father_first.years(),
            mother_first_child: mother_first.years(),
            father_last_child: father_last.years(),
            mother_last_child: mother_last.years(),
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
        recent_births,
        recent_deaths,
        recent_unions,
        oldest_possibly_alive,
        longest_lives,
        located_places,
        top_places,
        unlocated_places,
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
        mother.families_as_spouse = vec![spouse_link(
            family,
            SpouseRole::Wife,
            "4 FEB 1850",
            &children,
        )];
        vec![father, mother, first, second]
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
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
        let stats = compute(&family(), &[], today(), |labels| vec![None; labels.len()]);
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
        let stats = compute(&people, &[], today(), |labels| vec![None; labels.len()]);
        assert_eq!(counts(&stats.births_by_month, 1853), None);
        assert_eq!(counts(&stats.births_by_month, 1852).unwrap()[0], 1);
        assert_eq!(counts(&stats.unions_by_weekday, 1850).unwrap()[0], 1);
        assert_eq!(counts(&stats.unions_by_month, 1850).unwrap()[1], 1);
    }

    #[test]
    fn names_are_ranked_by_how_many_persons_carry_them() {
        let stats = compute(&family(), &[], today(), |labels| vec![None; labels.len()]);
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
                .top_given_names
                .iter()
                .any(|e| e.label == "Jean" && e.count == 2)
        );
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
        let stats = compute(&people, &places, today(), |labels| {
            labels
                .iter()
                .map(|(name, _)| (*name == "Place B").then_some((48.0, 2.0)))
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
}
