//! Potential duplicates on small fictitious trees.

use chrono::{NaiveDate, Utc};
use oxidgene_core::projection::{ProfileChildLink, ProfileFamilyLink, ProfileName};
use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole};

use super::*;

fn birth(value: &str, sort: (i32, u32, u32), place: Option<&str>) -> ProfileEvent {
    ProfileEvent {
        event_id: Uuid::now_v7(),
        event_type: EventType::Birth,
        date_value: Some(value.to_string()),
        date_sort: NaiveDate::from_ymd_opt(sort.0, sort.1, sort.2),
        date_qualifier: DateQualifier::Exact,
        date_value2: None,
        calendar: Calendar::Gregorian,
        place_name: place.map(str::to_string),
        place_id: None,
        description: None,
    }
}

fn person(given: &str, surname: &str, sex: Sex) -> PersonProfile {
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

fn child_of(p: &mut PersonProfile, family_id: Uuid) {
    p.family_as_child = Some(ProfileChildLink {
        family_id,
        child_type: ChildType::Biological,
        father_id: None,
        father_display_name: None,
        father_surname: None,
        father_given_names: None,
        mother_id: None,
        mother_display_name: None,
        mother_surname: None,
        mother_given_names: None,
    });
}

fn pairs(profiles: &[PersonProfile]) -> Vec<(i64, Vec<String>)> {
    potential_duplicates(profiles, &HashSet::new())
        .pairs
        .into_iter()
        .map(|p| (p.score, p.reasons))
        .collect()
}

#[test]
fn names_that_sound_alike_share_a_key() {
    assert_eq!(sound_key("Martins"), sound_key("Martin"));
    assert_eq!(sound_key("DUPOND"), sound_key("Dupont"));
    assert_eq!(sound_key("Philippe"), sound_key("Filipe"));
    assert_eq!(sound_key("Hélène"), sound_key("helene"));
    assert_ne!(sound_key("Branch"), sound_key("Branco"));
}

#[test]
fn a_name_alone_is_not_enough_but_a_shared_birth_is() {
    let a = person("Anna", "BRANCH_A", Sex::Female);
    let b = person("Anna", "BRANCH_A", Sex::Female);
    assert!(pairs(&[a.clone(), b.clone()]).is_empty());

    let a = born(a, birth("1850", (1850, 1, 1), Some("Sample Town")));
    let b = born(b, birth("3 MAR 1850", (1850, 3, 3), Some("Sample Town")));
    assert_eq!(
        pairs(&[a, b]),
        vec![(
            60,
            vec![
                "same_name".to_string(),
                "same_birth_year".to_string(),
                "same_birth_place".to_string()
            ]
        )]
    );
}

#[test]
fn similar_spellings_are_compared_too() {
    let a = born(
        person("Jean", "Dupont", Sex::Male),
        birth("2 FEB 1850", (1850, 2, 2), None),
    );
    let b = born(
        person("Jean", "DUPOND", Sex::Unknown),
        birth("2 FEB 1850", (1850, 2, 2), None),
    );
    assert_eq!(
        pairs(&[a, b]),
        vec![(
            45,
            vec!["similar_name".to_string(), "same_birth_date".to_string()]
        )]
    );
}

#[test]
fn what_rules_a_pair_out() {
    // Another sex.
    let a = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("1850", (1850, 1, 1), None),
    );
    let b = born(
        person("Sam", "BRANCH_A", Sex::Female),
        birth("1850", (1850, 1, 1), None),
    );
    assert!(pairs(&[a, b]).is_empty());
    // Births years apart.
    let a = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("1850", (1850, 1, 1), None),
    );
    let b = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("1860", (1860, 1, 1), None),
    );
    assert!(pairs(&[a, b]).is_empty());
    // Two children of one family born on different known days: the second
    // was named after the first.
    let family = Uuid::now_v7();
    let mut a = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("1 JAN 1850", (1850, 1, 1), None),
    );
    let mut b = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("5 JUN 1851", (1851, 6, 5), None),
    );
    child_of(&mut a, family);
    child_of(&mut b, family);
    assert!(pairs(&[a, b]).is_empty());
    // Spouses.
    let mut a = born(
        person("Sam", "BRANCH_A", Sex::Unknown),
        birth("1850", (1850, 1, 1), None),
    );
    let b = born(
        person("Sam", "BRANCH_A", Sex::Unknown),
        birth("1850", (1850, 1, 1), None),
    );
    a.families_as_spouse.push(ProfileFamilyLink {
        family_id: Uuid::now_v7(),
        role: SpouseRole::Partner,
        spouse_id: Some(b.person_id),
        spouse_display_name: None,
        spouse_surname: None,
        spouse_given_names: None,
        spouse_sex: None,
        marriage: None,
        events: Vec::new(),
        children_ids: Vec::new(),
        children_count: 0,
    });
    assert!(pairs(&[a, b]).is_empty());
}

#[test]
fn the_same_parents_count_and_confirmed_pairs_are_left_out() {
    let family = Uuid::now_v7();
    let mut a = born(
        person("Sam", "BRANCH_A", Sex::Male),
        birth("1850", (1850, 1, 1), None),
    );
    let mut b = person("Sam", "BRANCH_A", Sex::Male);
    child_of(&mut a, family);
    child_of(&mut b, family);
    let found = potential_duplicates(&[a.clone(), b.clone()], &HashSet::new());
    assert_eq!(found.count, 1);
    assert_eq!(found.pairs[0].score, 50);
    assert_eq!(found.pairs[0].reasons, vec!["same_name", "same_parents"]);

    let pair = if a.person_id < b.person_id {
        (a.person_id, b.person_id)
    } else {
        (b.person_id, a.person_id)
    };
    let confirmed: HashSet<_> = [pair].into_iter().collect();
    assert_eq!(potential_duplicates(&[a, b], &confirmed).count, 0);
}

/// A block is swept by birth year: each record meets the ones born within
/// the gap after it however many lie between, and the undated meet everyone.
#[test]
fn a_block_is_compared_within_the_birth_year_gap() {
    let born_in = |year: i32| {
        born(
            person("Given", "Surname", Sex::Male),
            birth(&year.to_string(), (year, 1, 1), None),
        )
    };
    let profiles = [
        born_in(1800),
        born_in(1803),
        born_in(1805),
        born_in(1811),
        person("Given", "Surname", Sex::Male),
        person("Given", "Surname", Sex::Male),
    ];
    let names = HashMap::new();
    let mut records: Vec<Record<'_>> = profiles
        .iter()
        .filter_map(|p| Record::new(p, &names))
        .collect();
    let mut found: Vec<(Option<i32>, Option<i32>)> = candidate_pairs(&mut records)
        .map(|(a, b)| (Record::year(a.birth), Record::year(b.birth)))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (None, None),
            (Some(1800), None),
            (Some(1800), None),
            (Some(1800), Some(1803)),
            (Some(1800), Some(1805)),
            (Some(1803), None),
            (Some(1803), None),
            (Some(1803), Some(1805)),
            (Some(1805), None),
            (Some(1805), None),
            (Some(1811), None),
            (Some(1811), None),
        ]
    );
}

/// However common a name, a record is compared with the homonyms born within
/// the gap of it, not with the whole block: a thousand born a year apart
/// make five pairs each, not half a million in all.
#[test]
fn homonyms_cost_their_neighbours_in_time_not_the_whole_block() {
    let profiles: Vec<PersonProfile> = (0..1000)
        .map(|n| {
            let year = 1000 + n;
            born(
                person("Given", "Surname", Sex::Male),
                birth(&year.to_string(), (year, 1, 1), None),
            )
        })
        .collect();
    let names = HashMap::new();
    let mut records: Vec<Record<'_>> = profiles
        .iter()
        .filter_map(|p| Record::new(p, &names))
        .collect();
    let gap = MAX_YEAR_GAP as usize;
    assert_eq!(
        candidate_pairs(&mut records).count(),
        gap * records.len() - gap * (gap + 1) / 2
    );
}
