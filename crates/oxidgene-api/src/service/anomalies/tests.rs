//! The anomaly rules, each on a small fictitious family.

use chrono::Utc;
use oxidgene_core::projection::{ProfileChildLink, ProfileFamilyLink, ProfileName};
use oxidgene_core::{Calendar, NameType};

use super::*;

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];

/// An event on a date given to the year, month or day.
fn ev(event_type: EventType, year: i32, month: Option<u32>, day: Option<u32>) -> ProfileEvent {
    let value = match (month, day) {
        (Some(m), Some(d)) => format!("{d} {} {year}", MONTHS[m as usize - 1]),
        (Some(m), None) => format!("{} {year}", MONTHS[m as usize - 1]),
        _ => year.to_string(),
    };
    ProfileEvent {
        event_id: Uuid::now_v7(),
        event_type,
        date_value: Some(value),
        date_sort: NaiveDate::from_ymd_opt(year, month.unwrap_or(1), day.unwrap_or(1)),
        date_qualifier: DateQualifier::Exact,
        date_value2: None,
        calendar: Calendar::Gregorian,
        place_name: None,
        place_id: None,
        description: None,
        age: None,
    }
}

fn year(event_type: EventType, y: i32) -> ProfileEvent {
    ev(event_type, y, None, None)
}

fn day(event_type: EventType, y: i32, m: u32, d: u32) -> ProfileEvent {
    ev(event_type, y, Some(m), Some(d))
}

fn person(given: &str, sex: Sex) -> PersonProfile {
    PersonProfile {
        person_id: Uuid::now_v7(),
        tree_id: Uuid::nil(),
        sex,
        primary_name: Some(ProfileName {
            name_id: Uuid::now_v7(),
            name_type: NameType::Birth,
            display_name: format!("{given} BRANCH_A"),
            given_names: Some(given.to_string()),
            surname: Some("BRANCH_A".to_string()),
        }),
        other_names: Vec::new(),
        birth: None,
        death: None,
        baptism: None,
        burial: None,
        occupation: None,
        other_events: Vec::new(),
        families_as_spouse: Vec::new(),
        family_as_child: None,
        primary_media: None,
        media_count: 0,
        citation_count: 0,
        note_count: 0,
        updated_at: Utc::now(),
        built_at: Utc::now(),
    }
}

fn born(mut p: PersonProfile, event: ProfileEvent) -> PersonProfile {
    p.birth = Some(event);
    p
}

fn died(mut p: PersonProfile, event: ProfileEvent) -> PersonProfile {
    p.death = Some(event);
    p
}

/// Makes `husband` and `wife` spouses of a new family with `events`, and
/// `children` its biological children. Returns the family.
fn family(
    husband: &mut PersonProfile,
    wife: &mut PersonProfile,
    events: Vec<ProfileEvent>,
    children: &mut [&mut PersonProfile],
) -> Uuid {
    let family_id = Uuid::now_v7();
    let children_ids: Vec<Uuid> = children.iter().map(|c| c.person_id).collect();
    let (husband_id, wife_id) = (husband.person_id, wife.person_id);
    for (me, other, role) in [
        (&mut *husband, wife_id, SpouseRole::Husband),
        (&mut *wife, husband_id, SpouseRole::Wife),
    ] {
        me.families_as_spouse.push(ProfileFamilyLink {
            family_id,
            role,
            spouse_id: Some(other),
            spouse_display_name: None,
            spouse_surname: None,
            spouse_given_names: None,
            spouse_sex: None,
            marriage: None,
            events: events.clone(),
            children_ids: children_ids.clone(),
            children_count: children_ids.len() as u32,
        });
    }
    for child in children.iter_mut() {
        child.family_as_child = Some(ProfileChildLink {
            family_id,
            child_type: ChildType::Biological,
            father_id: Some(husband_id),
            father_display_name: None,
            father_surname: None,
            father_given_names: None,
            mother_id: Some(wife_id),
            mother_display_name: None,
            mother_surname: None,
            mother_given_names: None,
        });
    }
    family_id
}

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
}

fn run(profiles: &[PersonProfile]) -> TreeAnomalies {
    compute(profiles, &[], today())
}

/// The persons each finding of `rule` names, by given name.
fn found(result: &TreeAnomalies, rule: &str) -> Vec<Vec<String>> {
    result
        .rules
        .iter()
        .find(|r| r.rule == rule)
        .map(|r| {
            r.items
                .iter()
                .map(|a| {
                    a.persons
                        .iter()
                        .map(|p| p.name.trim_end_matches(" BRANCH_A").to_string())
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn every_rule_is_catalogued_once() {
    let mut ids: Vec<_> = RULES.iter().map(|(id, _, _)| *id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), RULES.len());
    for (_, category, severity) in RULES {
        assert!(["dates", "filiation", "unions", "witnesses", "data_quality"].contains(category));
        assert!(["error", "warning"].contains(severity));
    }
}

#[test]
fn a_life_must_run_forwards_and_not_too_long() {
    let backwards = died(
        born(person("Backwards", Sex::Male), year(EventType::Birth, 1850)),
        year(EventType::Death, 1840),
    );
    // A year against the same year: the dates may be in either order.
    let same_year = died(
        born(
            person("SameYear", Sex::Male),
            day(EventType::Birth, 1850, 6, 1),
        ),
        year(EventType::Death, 1850),
    );
    let very_old = died(
        born(person("VeryOld", Sex::Female), year(EventType::Birth, 1900)),
        year(EventType::Death, 2008),
    );
    let old_centenarian = died(
        born(
            person("OldCentenarian", Sex::Female),
            year(EventType::Birth, 1780),
        ),
        year(EventType::Death, 1882),
    );
    let modern_centenarian = died(
        born(
            person("ModernCentenarian", Sex::Female),
            year(EventType::Birth, 1910),
        ),
        year(EventType::Death, 2012),
    );
    let result = run(&[
        backwards,
        same_year,
        very_old,
        old_centenarian,
        modern_centenarian,
    ]);
    assert_eq!(
        found(&result, "death_before_birth"),
        vec![vec!["Backwards"]]
    );
    assert_eq!(found(&result, "lived_over_105"), vec![vec!["VeryOld"]]);
    assert_eq!(
        found(&result, "centenarian_before_1900"),
        vec![vec!["OldCentenarian"]]
    );
}

#[test]
fn events_keep_their_place_in_a_life() {
    let mut p = born(
        person("Events", Sex::Male),
        day(EventType::Birth, 1850, 3, 2),
    );
    p.baptism = Some(day(EventType::Baptism, 1850, 2, 20));
    p.death = Some(day(EventType::Death, 1900, 5, 1));
    p.burial = Some(day(EventType::Burial, 1900, 4, 28));
    p.other_events = vec![
        year(EventType::Residence, 1902),
        day(EventType::Probate, 1901, 1, 1),
        // A succession, entered as a free-form event, is expected after a
        // death too, and so is a proxy ordinance.
        year(EventType::Other, 1901),
        year(EventType::Endowment, 1950),
        year(EventType::Census, 2090),
    ];
    let result = run(&[p]);
    assert_eq!(found(&result, "event_before_birth"), vec![vec!["Events"]]);
    assert_eq!(found(&result, "burial_before_death"), vec![vec!["Events"]]);
    // The residence and the census after death, once each and not again as
    // following the burial; the probate, the succession and the ordinance
    // are expected there.
    assert_eq!(found(&result, "event_after_death").len(), 2);
    assert!(found(&result, "burial_not_last").is_empty());
    assert_eq!(found(&result, "future_date"), vec![vec!["Events"]]);

    let mut buried = born(person("Buried", Sex::Female), year(EventType::Birth, 1800));
    buried.burial = Some(year(EventType::Burial, 1860));
    buried.baptism = Some(year(EventType::Baptism, 1870));
    buried.other_events = vec![year(EventType::Occupation, 1865)];
    let result = run(&[buried]);
    assert_eq!(found(&result, "burial_not_last"), vec![vec!["Buried"]]);
    assert_eq!(found(&result, "baptism_after_death"), vec![vec!["Buried"]]);
}

#[test]
fn parents_have_an_age_to_have_children() {
    let mut father = born(person("Father", Sex::Male), year(EventType::Birth, 1800));
    let mut mother = born(person("Mother", Sex::Female), year(EventType::Birth, 1840));
    father.death = Some(day(EventType::Death, 1890, 1, 1));
    mother.death = Some(day(EventType::Death, 1894, 6, 1));
    // Father 71 or more, mother over 55 at the birth.
    let mut late = born(person("Late", Sex::Male), day(EventType::Birth, 1896, 5, 1));
    family(&mut father, &mut mother, vec![], &mut [&mut late]);
    let result = run(&[father, mother, late]);
    assert_eq!(
        found(&result, "father_too_old"),
        vec![vec!["Late", "Father"]]
    );
    assert_eq!(
        found(&result, "mother_too_old"),
        vec![vec!["Late", "Mother"]]
    );
    assert_eq!(
        found(&result, "born_after_mother_death"),
        vec![vec!["Late", "Mother"]]
    );
    assert_eq!(
        found(&result, "born_long_after_father_death"),
        vec![vec!["Late", "Father"]]
    );
}

#[test]
fn a_posthumous_child_and_a_young_parent() {
    let mut father = born(person("Father", Sex::Male), year(EventType::Birth, 1800));
    father.death = Some(day(EventType::Death, 1830, 1, 1));
    let mut mother = born(
        person("Mother", Sex::Female),
        day(EventType::Birth, 1820, 1, 1),
    );
    // Eight months after the father's death, the mother aged ten.
    let mut posthumous = born(
        person("Posthumous", Sex::Male),
        day(EventType::Birth, 1830, 9, 1),
    );
    family(&mut father, &mut mother, vec![], &mut [&mut posthumous]);
    let result = run(&[father, mother, posthumous]);
    assert!(found(&result, "born_long_after_father_death").is_empty());
    assert_eq!(
        found(&result, "parent_too_young"),
        vec![vec!["Posthumous", "Mother"]]
    );
}

#[test]
fn an_adopted_child_is_not_measured_against_their_parents() {
    let mut father = born(person("Father", Sex::Male), year(EventType::Birth, 1900));
    let mut mother = born(person("Mother", Sex::Female), year(EventType::Birth, 1900));
    let mut adopted = born(person("Adopted", Sex::Female), year(EventType::Birth, 1890));
    family(&mut father, &mut mother, vec![], &mut [&mut adopted]);
    if let Some(link) = adopted.family_as_child.as_mut() {
        link.child_type = ChildType::Adopted;
    }
    let result = run(&[father, mother, adopted]);
    assert!(found(&result, "parent_born_after_child").is_empty());
}

#[test]
fn siblings_are_neither_too_close_nor_too_far() {
    let mut father = born(person("Father", Sex::Male), year(EventType::Birth, 1800));
    let mut mother = born(person("Mother", Sex::Female), year(EventType::Birth, 1805));
    let mut first = born(
        person("First", Sex::Male),
        day(EventType::Birth, 1830, 1, 10),
    );
    let mut twin = born(
        person("Twin", Sex::Male),
        day(EventType::Birth, 1830, 1, 12),
    );
    let mut close = born(
        person("Close", Sex::Female),
        day(EventType::Birth, 1830, 6, 1),
    );
    // Only the year: no way to tell how close.
    let mut vague = born(person("Vague", Sex::Female), year(EventType::Birth, 1831));
    let mut far = born(person("Far", Sex::Female), year(EventType::Birth, 1890));
    family(
        &mut father,
        &mut mother,
        vec![],
        &mut [&mut first, &mut twin, &mut close, &mut vague, &mut far],
    );
    let result = run(&[father, mother, first, twin, close, vague, far]);
    assert_eq!(
        found(&result, "siblings_too_close"),
        vec![vec!["Twin", "Close"]]
    );
    assert_eq!(
        found(&result, "siblings_far_apart"),
        vec![vec!["Vague", "Far"]]
    );
}

#[test]
fn unions_follow_births_and_precede_deaths() {
    let mut young = born(
        person("Young", Sex::Male),
        day(EventType::Birth, 1850, 1, 1),
    );
    let mut old = born(person("Old", Sex::Female), year(EventType::Birth, 1790));
    old.death = Some(year(EventType::Death, 1860));
    let family_id = family(
        &mut young,
        &mut old,
        vec![day(EventType::Marriage, 1861, 5, 1)],
        &mut [],
    );
    let result = run(&[young, old]);
    assert_eq!(found(&result, "union_too_young"), vec![vec!["Young"]]);
    assert_eq!(found(&result, "union_after_death"), vec![vec!["Old"]]);
    assert_eq!(
        found(&result, "spouses_age_gap"),
        vec![vec!["Young", "Old"]]
    );
    let rule = result
        .rules
        .iter()
        .find(|r| r.rule == "union_after_death")
        .unwrap();
    assert_eq!(rule.items[0].family_id, Some(family_id.to_string()));
    assert_eq!(rule.category, "unions");
    assert_eq!(rule.severity, "error");
}

#[test]
fn spouses_repeated_homonymous_or_related() {
    let mut a = person("Spouse", Sex::Male);
    let mut b = person("Wife", Sex::Female);
    family(&mut a, &mut b, vec![], &mut []);
    family(&mut a, &mut b, vec![], &mut []);
    // Two different women of one name.
    let mut c = person("Wife", Sex::Female);
    family(&mut a, &mut c, vec![], &mut []);
    // A father married to his daughter.
    let mut father = person("Father", Sex::Male);
    let mut mother = person("Mother", Sex::Female);
    let mut daughter = person("Daughter", Sex::Female);
    family(&mut father, &mut mother, vec![], &mut [&mut daughter]);
    family(&mut father, &mut daughter, vec![], &mut []);
    let result = run(&[a, b, c, father, mother, daughter]);
    assert_eq!(
        found(&result, "repeated_union"),
        vec![vec!["Spouse", "Wife"]]
    );
    assert_eq!(
        found(&result, "homonymous_spouses"),
        vec![vec!["Spouse", "Wife", "Wife"]]
    );
    assert_eq!(
        found(&result, "union_with_parent_or_child"),
        vec![vec!["Daughter", "Father"]]
    );
}

#[test]
fn lines_have_no_loop_and_run_down_the_years() {
    let mut grandfather = born(
        person("Grandfather", Sex::Male),
        year(EventType::Birth, 1950),
    );
    let mut grandmother = person("Grandmother", Sex::Female);
    let mut father = born(person("Father", Sex::Male), year(EventType::Birth, 1880));
    let mut mother = person("Mother", Sex::Female);
    let mut child = born(person("Child", Sex::Male), year(EventType::Birth, 1910));
    family(
        &mut grandfather,
        &mut grandmother,
        vec![],
        &mut [&mut father],
    );
    family(&mut father, &mut mother, vec![], &mut [&mut child]);
    let result = run(&[
        grandfather.clone(),
        grandmother,
        father.clone(),
        mother,
        child,
    ]);
    // The father is born before his father: that is the parent rule. The
    // child is born before their grandfather: the ancestor rule.
    assert_eq!(
        found(&result, "parent_born_after_child"),
        vec![vec!["Father", "Grandfather"]]
    );
    assert_eq!(
        found(&result, "ancestor_born_after_descendant"),
        vec![vec!["Child", "Grandfather"]]
    );

    // A loop: each the other's father.
    let mut a = person("LoopA", Sex::Male);
    let mut b = person("LoopB", Sex::Male);
    let mut wife = person("Wife", Sex::Female);
    let mut other_wife = person("OtherWife", Sex::Female);
    family(&mut a, &mut wife, vec![], &mut [&mut b]);
    family(&mut b, &mut other_wife, vec![], &mut [&mut a]);
    let result = run(&[a, b, wife, other_wife]);
    let cycles = found(&result, "own_ancestor");
    assert_eq!(cycles.len(), 1);
    let mut names = cycles[0].clone();
    names.sort();
    assert_eq!(names, vec!["LoopA", "LoopB"]);
}

#[test]
fn witnesses_live_at_the_event_and_godparents_fit_their_sex() {
    let child = born(
        person("Child", Sex::Male),
        day(EventType::Birth, 1850, 1, 1),
    );
    let dead = died(
        born(person("Dead", Sex::Male), year(EventType::Birth, 1800)),
        year(EventType::Death, 1840),
    );
    let unborn = born(person("Unborn", Sex::Female), year(EventType::Birth, 1860));
    let godfather = born(
        person("Godfather", Sex::Female),
        year(EventType::Birth, 1820),
    );
    let birth = child.birth.as_ref().unwrap().event_id;
    let link = |person: &PersonProfile, relation: &str, sort_order| EventWitness {
        id: Uuid::now_v7(),
        event_id: birth,
        person_id: person.person_id,
        relation: Some(relation.to_string()),
        sort_order,
    };
    let witnesses = vec![
        link(&dead, "Witness", 0),
        link(&unborn, "Witness", 1),
        link(&godfather, "Parrain", 2),
    ];
    let result = compute(&[child, dead, unborn, godfather], &witnesses, today());
    assert_eq!(
        found(&result, "witness_after_death"),
        vec![vec!["Dead", "Child"]]
    );
    assert_eq!(
        found(&result, "witness_before_birth"),
        vec![vec!["Unborn", "Child"]]
    );
    assert_eq!(
        found(&result, "godparent_sex"),
        vec![vec!["Godfather", "Child"]]
    );
    let rule = result
        .rules
        .iter()
        .find(|r| r.rule == "godparent_sex")
        .unwrap();
    assert_eq!(rule.items[0].text.as_deref(), Some("Parrain"));
    assert_eq!(rule.items[0].event_type.as_deref(), Some("birth"));
}

#[test]
fn records_are_named_readable_and_in_their_roles() {
    let mut unnamed = person("", Sex::Unknown);
    unnamed.primary_name = None;
    let mut unreadable = person("Unreadable", Sex::Male);
    unreadable.birth = Some(ProfileEvent {
        date_sort: None,
        date_value: Some("the spring thaw".to_string()),
        ..year(EventType::Birth, 1800)
    });
    let mut reversed = person("Reversed", Sex::Male);
    reversed.death = Some(ProfileEvent {
        date_qualifier: DateQualifier::Between,
        date_value2: Some("1800".to_string()),
        ..year(EventType::Death, 1850)
    });
    let mut husband = person("Husband", Sex::Female);
    let mut wife = person("WifeToo", Sex::Female);
    family(&mut husband, &mut wife, vec![], &mut []);
    let result = run(&[unnamed, unreadable, reversed, husband, wife]);
    assert_eq!(found(&result, "no_name").len(), 1);
    assert_eq!(found(&result, "unreadable_date"), vec![vec!["Unreadable"]]);
    let rule = result
        .rules
        .iter()
        .find(|r| r.rule == "unreadable_date")
        .unwrap();
    assert_eq!(rule.items[0].text.as_deref(), Some("the spring thaw"));
    assert_eq!(found(&result, "reversed_range"), vec![vec!["Reversed"]]);
    assert_eq!(found(&result, "spouse_role_sex"), vec![vec!["Husband"]]);
}

#[test]
fn approximate_dates_widen_by_their_slack() {
    // About 1850 may be 1848: a death in 1849 may follow it.
    let mut about = person("About", Sex::Male);
    about.birth = Some(ProfileEvent {
        date_qualifier: DateQualifier::About,
        ..year(EventType::Birth, 1850)
    });
    about.death = Some(year(EventType::Death, 1849));
    // Before 1850 says too little to compare with anything.
    let mut before = person("Before", Sex::Male);
    before.birth = Some(ProfileEvent {
        date_qualifier: DateQualifier::Before,
        ..year(EventType::Birth, 1900)
    });
    before.death = Some(year(EventType::Death, 1800));
    let result = run(&[about, before]);
    assert!(found(&result, "death_before_birth").is_empty());
}

#[test]
fn a_rule_lists_a_bounded_number_of_items_but_counts_them_all() {
    let persons: Vec<_> = (0..MAX_ITEMS_PER_RULE + 3)
        .map(|_| {
            let mut p = person("", Sex::Unknown);
            p.primary_name = None;
            p
        })
        .collect();
    let result = run(&persons);
    let rule = result.rules.iter().find(|r| r.rule == "no_name").unwrap();
    assert_eq!(rule.count, (MAX_ITEMS_PER_RULE + 3) as i64);
    assert_eq!(rule.items.len(), MAX_ITEMS_PER_RULE);
}

#[test]
fn places_without_coordinates_the_dictionary_cannot_read_are_listed() {
    let place = |name: &str, coordinates: Option<(f64, f64)>| Place {
        id: Uuid::now_v7(),
        tree_id: Uuid::nil(),
        name: name.to_string(),
        latitude: coordinates.map(|c| c.0),
        longitude: coordinates.map(|c| c.1),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let places = vec![
        (place("Own Coordinates", Some((45.0, 5.0))), 3),
        (place("Known Village", None), 5),
        (place("Nowhere Hamlet", None), 2),
        (place("Unused Hamlet", None), 0),
    ];
    let unlocated = unlocated_places(&places, |labels| {
        labels
            .iter()
            .map(|(name, _)| PlaceLocation {
                spot: (*name == "Known Village").then_some((46.0, 4.0)),
                ..PlaceLocation::default()
            })
            .collect()
    });
    let names: Vec<_> = unlocated
        .iter()
        .map(|p| (p.name.as_str(), p.count))
        .collect();
    assert_eq!(names, vec![("Nowhere Hamlet", 2)]);
}

/// `event` stating `age` for its person.
fn aged(mut event: ProfileEvent, age: &str) -> ProfileEvent {
    event.age = Some(age.to_string());
    event
}

#[test]
fn a_recorded_age_must_fit_the_dates_within_two_years() {
    // Born 1800: dies in 1850 aged 49 — a year's rounding, not reported.
    let fits = died(
        born(person("Fits", Sex::Male), year(EventType::Birth, 1800)),
        aged(year(EventType::Death, 1850), "49y"),
    );
    // Dies in 1850 "aged 30": twenty years off.
    let off = died(
        born(
            person("Off", Sex::Female),
            day(EventType::Birth, 1800, 3, 1),
        ),
        aged(day(EventType::Death, 1850, 6, 1), "30y"),
    );
    // "Under one year" at 5: off; "over 80" at 40: off; "child" at 6: fits.
    let mut infant = born(person("Infant", Sex::Male), year(EventType::Birth, 1800));
    infant.burial = Some(aged(year(EventType::Burial, 1805), "< 1y"));
    let mut elder = born(person("Elder", Sex::Female), year(EventType::Birth, 1800));
    elder.other_events = vec![aged(year(EventType::Residence, 1840), "> 80y")];
    let child = died(
        born(person("Child", Sex::Male), year(EventType::Birth, 1800)),
        aged(year(EventType::Death, 1806), "CHILD"),
    );
    // A date about a year widens by its slack: aged 51 about 1847 fits.
    let mut about = born(person("About", Sex::Female), year(EventType::Birth, 1800));
    let mut approximate = aged(year(EventType::Death, 1847), "51y");
    approximate.date_qualifier = DateQualifier::About;
    about.death = Some(approximate);
    // A spouse's age at the marriage, read from the union.
    let mut groom = born(person("Groom", Sex::Male), year(EventType::Birth, 1800));
    let mut bride = born(person("Bride", Sex::Female), year(EventType::Birth, 1802));
    family(
        &mut groom,
        &mut bride,
        vec![year(EventType::Marriage, 1825)],
        &mut [],
    );
    groom.families_as_spouse[0].events[0].age = Some("40y".to_string());
    bride.families_as_spouse[0].events[0].age = Some("23y".to_string());

    let result = run(&[fits, off, infant, elder, child, about, groom, bride]);
    let mut names = found(&result, "recorded_age_mismatch");
    names.sort();
    assert_eq!(
        names,
        vec![vec!["Elder"], vec!["Groom"], vec!["Infant"], vec!["Off"]]
    );
    let rule = result
        .rules
        .iter()
        .find(|r| r.rule == "recorded_age_mismatch")
        .unwrap();
    assert_eq!(
        (rule.category.as_str(), rule.severity.as_str()),
        ("dates", "warning")
    );
    let off = rule
        .items
        .iter()
        .find(|a| a.text.as_deref() == Some("30y"))
        .unwrap();
    assert_eq!(off.value, Some(50));
    assert_eq!(off.event_type.as_deref(), Some("death"));
}
