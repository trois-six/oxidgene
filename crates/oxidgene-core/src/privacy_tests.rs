//! The rule tables of `docs/privacy.md` §11: each qualifier, each kind of
//! evidence, descendants at several depths, the 10-generation limit, the
//! smallest bound winning, undated persons, the year boundary with a fixed
//! clock, and the precedence of §3.1.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, Utc};
use uuid::Uuid;

use super::*;

fn event(kind: EventType, year: i32, qualifier: DateQualifier) -> Event {
    let now = Utc::now();
    Event {
        id: Uuid::now_v7(),
        tree_id: Uuid::nil(),
        event_type: kind,
        date_value: Some(year.to_string()),
        date_sort: NaiveDate::from_ymd_opt(year, 1, 1),
        date_qualifier: qualifier,
        date_value2: None,
        calendar: Calendar::Gregorian,
        cause: None,
        age: None,
        agency: None,
        spouse_ages: Vec::new(),
        place_id: None,
        person_id: None,
        family_id: None,
        related_family_id: None,
        description: None,
        classification: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        deleted_at: None,
    }
}

fn exact(kind: EventType, year: i32) -> Event {
    event(kind, year, DateQualifier::Exact)
}

fn undated(kind: EventType) -> Event {
    Event {
        date_value: None,
        date_sort: None,
        ..exact(kind, 2000)
    }
}

fn range(qualifier: DateQualifier, first: i32, second: &str) -> Event {
    Event {
        date_value2: Some(second.to_string()),
        ..event(EventType::Birth, first, qualifier)
    }
}

fn evidence(events: &[Event]) -> LifeEvidence {
    LifeEvidence::from_events(events, std::iter::empty())
}

const NOW: i32 = 2026;
const RULE: ContemporaryRule = ContemporaryRule {
    enabled: true,
    years: 100,
};

#[test]
fn each_qualifier_bounds_the_birth_as_the_table_says() {
    let latest = |event: Event| evidence(&[event]).birth_latest();
    assert_eq!(latest(exact(EventType::Birth, 1900)), Some(1900));
    for qualifier in [
        DateQualifier::About,
        DateQualifier::Estimated,
        DateQualifier::Calculated,
    ] {
        assert_eq!(
            latest(event(EventType::Birth, 1900, qualifier)),
            Some(1905),
            "{qualifier}"
        );
    }
    assert_eq!(
        latest(event(EventType::Birth, 1900, DateQualifier::Before)),
        Some(1899)
    );
    assert_eq!(
        latest(range(DateQualifier::Between, 1900, "1910")),
        Some(1910)
    );
    assert_eq!(
        latest(range(DateQualifier::Between, 1900, "12 MAR 1910")),
        Some(1910)
    );
    // `after Y`, and a range whose far end cannot be read, bound nothing.
    assert_eq!(
        latest(event(EventType::Birth, 1900, DateQualifier::After)),
        None
    );
    assert_eq!(latest(range(DateQualifier::Between, 1900, "soon")), None);
    // A phrase OxidGene could not read has no date to bound with.
    assert_eq!(latest(undated(EventType::Birth)), None);
}

#[test]
fn a_date_of_another_calendar_ends_in_the_gregorian_year_it_reaches() {
    // Republican year VII runs from September 1798 to September 1799.
    let republican = Event {
        calendar: Calendar::FrenchRepublican,
        date_value: Some("7".to_string()),
        date_sort: NaiveDate::from_ymd_opt(1798, 9, 22),
        ..exact(EventType::Birth, 1798)
    };
    assert_eq!(evidence(&[republican]).birth_latest(), Some(1799));
    assert_eq!(
        latest_gregorian_year("15 VEND 7", Calendar::FrenchRepublican),
        Some(1799)
    );
    assert_eq!(latest_gregorian_year("5600", Calendar::Hebrew), Some(1840));
}

#[test]
fn the_birth_comes_before_the_baptism_and_the_other_events() {
    let both = evidence(&[
        exact(EventType::Baptism, 1880),
        exact(EventType::Birth, 1890),
    ]);
    assert_eq!(both.birth_latest(), Some(1890));
    let baptized = evidence(&[exact(EventType::Christening, 1880)]);
    assert_eq!(baptized.birth_latest(), Some(1880));
    // A descendant's records: the birth, else the baptism, else the rest.
    let worker = evidence(&[exact(EventType::Occupation, 1920)]);
    assert_eq!(worker.recorded_latest(), Some(1920));
    assert_eq!(both.recorded_latest(), Some(1890));
}

#[test]
fn a_death_burial_or_cremation_dated_or_not_makes_a_person_deceased() {
    for kind in [EventType::Death, EventType::Burial, EventType::Cremation] {
        let row = evidence(&[undated(kind)]);
        assert!(row.deceased, "{kind:?}");
        assert!(!RULE.applies(row.deceased, None, NOW));
    }
    assert!(!evidence(&[exact(EventType::Birth, 2000)]).deceased);
}

#[test]
fn the_own_other_events_and_marriages_bound_the_birth() {
    let marriage = exact(EventType::Marriage, 1930);
    let own = [exact(EventType::Census, 1925)];
    let row = LifeEvidence::from_events(&own, [&marriage]);
    // Born at the latest at the census, 14 years before the marriage.
    assert_eq!(row.own_latest(), Some(1916));
    let divorce = exact(EventType::Divorce, 1940);
    let row = LifeEvidence::from_events(std::iter::empty(), [&divorce]);
    assert_eq!(row.own_latest(), Some(1940));
}

fn person() -> Uuid {
    Uuid::now_v7()
}

/// A tree of `families`, each person with the events given.
fn tree(
    events: &[(Uuid, Vec<Event>)],
    families: &[FamilyMembers],
) -> HashMap<Uuid, PersonVisibility> {
    let by_person: HashMap<Uuid, LifeEvidence> = events
        .iter()
        .map(|(id, events)| (*id, evidence(events)))
        .collect();
    let persons: Vec<(Uuid, Privacy)> = events
        .iter()
        .map(|(id, _)| (*id, Privacy::Default))
        .collect();
    derive_tree(&persons, &by_person, families)
}

fn family(spouses: &[Uuid], children: &[Uuid]) -> FamilyMembers {
    FamilyMembers {
        spouses: spouses.to_vec(),
        children: children.to_vec(),
    }
}

#[test]
fn each_relative_bounds_the_birth_with_its_margin() {
    let (parent, child, spouse, sibling) = (person(), person(), person(), person());
    let rows = tree(
        &[
            (parent, vec![exact(EventType::Birth, 1850)]),
            (child, vec![]),
            (spouse, vec![]),
            (sibling, vec![]),
        ],
        &[family(&[parent], &[child, sibling])],
    );
    // A parent born in 1850: the child at the latest in 1920.
    assert_eq!(rows[&child].latest_birth_year, Some(1920));
    // And at the earliest in 1865, which shows nobody old.
    assert_eq!(rows[&child].earliest_birth_year, Some(1865));

    let rows = tree(
        &[
            (parent, vec![exact(EventType::Death, 1900)]),
            (child, vec![]),
        ],
        &[family(&[parent], &[child])],
    );
    assert_eq!(rows[&child].latest_birth_year, Some(1901));

    let rows = tree(
        &[
            (spouse, vec![exact(EventType::Birth, 1900)]),
            (child, vec![]),
        ],
        &[family(&[spouse, child], &[])],
    );
    assert_eq!(rows[&child].latest_birth_year, Some(1950));

    let rows = tree(
        &[
            (sibling, vec![exact(EventType::Baptism, 1900)]),
            (child, vec![]),
        ],
        &[family(&[], &[sibling, child])],
    );
    assert_eq!(rows[&child].latest_birth_year, Some(1930));
}

/// A line of `generations` persons below `top`, the last one born in
/// `year`, the others undated: the persons from the top down.
fn line(generations: usize, year: i32) -> (Vec<(Uuid, Vec<Event>)>, Vec<FamilyMembers>) {
    let ids: Vec<Uuid> = (0..=generations).map(|_| person()).collect();
    let events = ids
        .iter()
        .enumerate()
        .map(|(depth, id)| {
            let events = if depth == generations {
                vec![exact(EventType::Baptism, year)]
            } else {
                vec![]
            };
            (*id, events)
        })
        .collect();
    let families = ids
        .windows(2)
        .map(|pair| family(&[pair[0]], &[pair[1]]))
        .collect();
    (events, families)
}

#[test]
fn a_descendant_at_depth_d_bounds_the_birth_d_generation_gaps_earlier() {
    // A grandchild baptized in 1890: born at the latest in 1860.
    let (events, families) = line(2, 1890);
    let rows = tree(&events, &families);
    let top = events[0].0;
    assert_eq!(rows[&top].latest_birth_year, Some(1860));
    assert_eq!(rows[&events[1].0].latest_birth_year, Some(1875));

    for depth in [1, 3, 5, DESCENT_DEPTH] {
        let (events, families) = line(depth, 1990);
        let rows = tree(&events, &families);
        assert_eq!(
            rows[&events[0].0].latest_birth_year,
            Some(1990 - depth as i32 * GENERATION_GAP),
            "depth {depth}"
        );
    }
}

#[test]
fn descendants_beyond_ten_generations_are_not_read() {
    let (events, families) = line(DESCENT_DEPTH + 1, 1990);
    let rows = tree(&events, &families);
    assert_eq!(rows[&events[0].0].latest_birth_year, None);
    assert!(rows[&events[0].0].is_contemporary(RULE, NOW));
    // One generation lower, the same descendant is within reach.
    assert_eq!(
        rows[&events[1].0].latest_birth_year,
        Some(1990 - DESCENT_DEPTH as i32 * GENERATION_GAP)
    );
}

#[test]
fn the_smallest_bound_wins() {
    let (parent, child, grandchild) = (person(), person(), person());
    let rows = tree(
        &[
            (parent, vec![exact(EventType::Birth, 1950)]),
            (
                child,
                vec![event(EventType::Birth, 1990, DateQualifier::About)],
            ),
            (grandchild, vec![exact(EventType::Birth, 1960)]),
        ],
        &[family(&[parent], &[child]), family(&[child], &[grandchild])],
    );
    // Own birth 1995, parent 1950 + 70 = 2020, grandchild 1960 − 15 = 1945.
    assert_eq!(rows[&child].latest_birth_year, Some(1945));
}

#[test]
fn an_undated_person_is_contemporary_unless_a_relative_dates_them() {
    let (alone, parent, child) = (person(), person(), person());
    let rows = tree(&[(alone, vec![])], &[]);
    assert_eq!(rows[&alone].latest_birth_year, None);
    assert!(rows[&alone].is_contemporary(RULE, NOW));

    // Undated, but their child was born in 1880: born by 1865, long ago.
    let rows = tree(
        &[
            (parent, vec![]),
            (child, vec![exact(EventType::Birth, 1880)]),
        ],
        &[family(&[parent], &[child])],
    );
    assert!(!rows[&parent].is_contemporary(RULE, NOW));
    // The child, born in 1880 with no death recorded, is not either.
    assert!(!rows[&child].is_contemporary(RULE, NOW));
}

#[test]
fn a_person_turns_non_contemporary_on_the_first_of_january() {
    let row = tree(&[(Uuid::nil(), vec![exact(EventType::Birth, 1926)])], &[])[&Uuid::nil()];
    assert!(row.is_contemporary(RULE, 2025));
    assert!(!row.is_contemporary(RULE, 2026));
    // The threshold is applied at read time.
    let eighty = ContemporaryRule {
        enabled: true,
        years: 80,
    };
    assert!(!row.is_contemporary(eighty, 2025));
    let off = ContemporaryRule {
        enabled: false,
        years: 100,
    };
    assert!(!tree(&[(Uuid::nil(), vec![])], &[])[&Uuid::nil()].is_contemporary(off, NOW));
}

#[test]
fn the_descent_is_stored_and_read_back() {
    let mut descent = Descent::default();
    descent.0[0] = Some(1900);
    descent.0[9] = Some(-20);
    let text = descent.encode();
    assert_eq!(text, "1900,,,,,,,,,-20");
    assert_eq!(Descent::decode(&text), Some(descent));
    assert_eq!(Descent::decode(""), None);
    assert_eq!(Descent::decode("1,2"), None);
    assert_eq!(Descent::decode(&format!("{text},1")), None);
}

#[test]
fn a_cycle_in_the_links_ends() {
    let (a, b) = (person(), person());
    let rows = tree(
        &[(a, vec![exact(EventType::Birth, 1990)]), (b, vec![])],
        &[family(&[a], &[b]), family(&[b], &[a])],
    );
    // a is b's child and b's parent, so a is b's descendant at every odd
    // depth: the walk stops at ten generations, the ninth reading a last.
    assert_eq!(rows[&b].latest_birth_year, Some(1990 - 9 * GENERATION_GAP));
}

#[test]
fn the_first_rule_that_applies_decides() {
    use TreeDefaultPrivacy::{Private, Public};
    assert_eq!(
        withheld(true, Privacy::Public, Public, false),
        Some(Withheld::Excluded)
    );
    assert_eq!(
        withheld(false, Privacy::Private, Public, true),
        Some(Withheld::Private)
    );
    assert_eq!(
        withheld(false, Privacy::Default, Private, false),
        Some(Withheld::Private)
    );
    // A person's own `public` does not turn the contemporary rule off.
    assert_eq!(
        withheld(false, Privacy::Public, Private, true),
        Some(Withheld::Contemporary)
    );
    assert_eq!(withheld(false, Privacy::Public, Private, false), None);
    assert_eq!(withheld(false, Privacy::Default, Public, false), None);
}

#[test]
fn each_exclusion_kind_covers_its_branch() {
    // grandparent → parent (+ spouse) → child → grandchild (+ spouse)
    let [grand, parent, spouse, child, grandchild, partner] = [(); 6].map(|()| person());
    let kin = Kinship::of_families(&[
        family(&[grand], &[parent]),
        family(&[parent, spouse], &[child]),
        family(&[child], &[grandchild]),
        family(&[grandchild, partner], &[]),
    ]);
    let live: HashSet<Uuid> = [grand, parent, spouse, child, grandchild, partner].into();
    let scope = |kind, depth, include_root, include_spouses| {
        let mut covered: Vec<Uuid> = ExclusionScope {
            kind,
            root: parent,
            depth,
            include_root,
            include_spouses,
        }
        .persons(&kin, &live)
        .into_iter()
        .collect();
        covered.sort();
        covered
    };
    let sorted = |mut ids: Vec<Uuid>| {
        ids.sort();
        ids
    };
    assert_eq!(
        scope(ExclusionKind::Person, None, false, false),
        vec![parent]
    );
    assert_eq!(
        scope(ExclusionKind::Ancestors, None, false, false),
        vec![grand]
    );
    assert_eq!(
        scope(ExclusionKind::Ancestors, None, true, false),
        sorted(vec![grand, parent])
    );
    assert_eq!(
        scope(ExclusionKind::Descendants, None, true, true),
        sorted(vec![parent, child, grandchild, partner])
    );
    assert_eq!(
        scope(ExclusionKind::Descendants, Some(1), false, false),
        vec![child]
    );
    // A deleted root covers nobody until it is restored.
    let without_root: HashSet<Uuid> = live.iter().copied().filter(|id| *id != parent).collect();
    assert!(
        ExclusionScope {
            kind: ExclusionKind::Descendants,
            root: parent,
            depth: None,
            include_root: true,
            include_spouses: true,
        }
        .persons(&kin, &without_root)
        .is_empty()
    );
}

#[test]
fn a_range_end_no_date_can_hold_bounds_nothing_and_never_overflows() {
    // A free-text far end at the edge of `i32` must not wrap a relative's
    // bound into the distant past: the child stays contemporary.
    let parent = evidence(&[range(DateQualifier::Between, 1990, "2147483647")]);
    assert_eq!(parent.birth_latest(), None);
    let child = LifeEvidence::default();
    let bound = kin_latest(&child, [&parent], [&parent], [&parent]);
    let visibility = PersonVisibility {
        latest_birth_year: bound,
        ..PersonVisibility::default()
    };
    assert!(visibility.is_contemporary(RULE, NOW));
    assert_eq!(
        latest_gregorian_year("2147483647", Calendar::Gregorian),
        None
    );
    assert_eq!(latest_gregorian_year("2147483647", Calendar::Julian), None);
    assert_eq!(latest_gregorian_year("-2147483648", Calendar::Hebrew), None);

    // Stored bounds at the edge saturate rather than wrap.
    let extreme = LifeEvidence {
        birth: Some(YearSpan {
            earliest: Some(i32::MAX),
            latest: Some(i32::MAX),
        }),
        death_latest: Some(i32::MAX),
        ..LifeEvidence::default()
    };
    assert_eq!(
        kin_latest(&child, [&extreme], [&extreme], [&extreme]),
        Some(i32::MAX)
    );
    assert_eq!(kin_earliest(&child, [&extreme]), Some(i32::MAX));
    let deep = Descent([Some(i32::MIN); DESCENT_DEPTH]);
    assert_eq!(
        Descent::of_children([(Some(i32::MIN), &deep)]).bound(),
        Some(i32::MIN)
    );
}
