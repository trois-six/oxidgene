//! Potential duplicates on small fictitious trees.

use chrono::{NaiveDate, Utc};
use oxidgene_core::projection::{ProfileChildLink, ProfileFamilyLink, ProfileName};
use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole};

use super::*;

fn birth(value: &str, sort: (i32, u32, u32), place: Option<&str>) -> ProfileEvent {
    ProfileEvent {
        tags: Vec::new(),
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
        age: None,
    }
}

fn person(given: &str, surname: &str, sex: Sex) -> PersonProfile {
    PersonProfile {
        placeholder: None,
        tags: Vec::new(),
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
        father_placeholder: None,
        mother_placeholder: None,
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
        spouse_placeholder: None,
        family_id: Uuid::now_v7(),
        role: SpouseRole::Partner,
        spouse_id: Some(b.person_id),
        spouse_display_name: None,
        spouse_surname: None,
        spouse_given_names: None,
        spouse_sex: None,
        events: Vec::new(),
        children_ids: Vec::new(),
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

fn died(mut p: PersonProfile, year: i32) -> PersonProfile {
    let mut event = birth(&year.to_string(), (year, 1, 1), None);
    event.event_type = EventType::Death;
    p.death = Some(event);
    p
}

fn records<'a>(profiles: &'a [PersonProfile], names: &'a HashMap<Uuid, String>) -> Vec<Record<'a>> {
    profiles
        .iter()
        .filter_map(|p| Record::new(p, names))
        .collect()
}

/// A block is swept by birth year: each record meets the ones born within
/// the gap after it however many lie between. A record without a birth
/// year meets the records it shares a clue with — here a death year — and
/// nobody else: a name alone lists no pair.
#[test]
fn a_block_is_compared_within_the_birth_year_gap_and_by_shared_clues() {
    let born_in = |year: i32| {
        born(
            person("Given", "Surname", Sex::Male),
            birth(&year.to_string(), (year, 1, 1), None),
        )
    };
    let profiles = [
        born_in(1800),
        died(born_in(1803), 1870),
        born_in(1805),
        born_in(1811),
        // Undated: one that shares nothing, two that died in 1870.
        person("Given", "Surname", Sex::Male),
        died(person("Given", "Surname", Sex::Male), 1870),
        died(person("Given", "Surname", Sex::Male), 1870),
    ];
    let names = HashMap::new();
    let mut records = records(&profiles, &names);
    let mut found: Vec<(Option<i32>, Option<i32>)> = candidate_pairs(&mut records, &names)
        .map(|(a, b)| (Record::year(a.birth), Record::year(b.birth)))
        .collect();
    found.sort();
    assert_eq!(
        found,
        vec![
            (None, None),
            (Some(1800), Some(1803)),
            (Some(1800), Some(1805)),
            (Some(1803), None),
            (Some(1803), None),
            (Some(1803), Some(1805)),
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
    let mut records = records(&profiles, &names);
    let gap = MAX_YEAR_GAP as usize;
    assert_eq!(
        candidate_pairs(&mut records, &names).count(),
        gap * records.len() - gap * (gap + 1) / 2
    );
}

/// A block of five thousand homonyms, half of them without a birth year:
/// the undated ones are not compared with the whole block either. Here
/// every tenth undated record died in one of five years, so each of those
/// meets the records that died the same year; the others meet nobody.
/// Compared one by one they made more than nine million pairs.
#[test]
fn undated_homonyms_cost_what_they_share_not_the_whole_block() {
    const BLOCK: usize = 5_000;
    let profiles: Vec<PersonProfile> = (0..BLOCK)
        .map(|n| {
            let record = person("Given", "Surname", Sex::Male);
            if n % 2 == 0 {
                let year = 1000 + (n / 2) as i32;
                return born(record, birth(&year.to_string(), (year, 1, 1), None));
            }
            if n % 20 == 1 {
                died(record, 1500 + (n / 20 % 5) as i32)
            } else {
                record
            }
        })
        .collect();
    let names = HashMap::new();
    let mut records = records(&profiles, &names);
    let started = std::time::Instant::now();
    let candidates = candidate_pairs(&mut records, &names).count();
    let elapsed = started.elapsed();

    let dated = BLOCK / 2;
    let gap = MAX_YEAR_GAP as usize;
    let within_the_gap = gap * dated - gap * (gap + 1) / 2;
    // 250 undated records with a death year, 50 for each of the five years.
    let sharing_a_death_year = 5 * (50 * 49 / 2);
    assert_eq!(candidates, within_the_gap + sharing_a_death_year);
    eprintln!("{BLOCK} homonyms, half undated: {candidates} candidate pairs in {elapsed:?}");

    let started = std::time::Instant::now();
    let found = potential_duplicates(&profiles, &HashSet::new());
    eprintln!(
        "{BLOCK} homonyms, half undated: {} pairs found in {:?}",
        found.count,
        started.elapsed()
    );
    // The same name and the same death year: 45 points each.
    assert_eq!(found.count, sharing_a_death_year as i64);
    assert_eq!(found.pairs.len(), MAX_DUPLICATE_PAIRS);
}

/// Every pair of `profiles`, compared one by one: what the candidates must
/// lose nothing of.
fn compared_one_by_one(profiles: &[PersonProfile]) -> Vec<(Uuid, Uuid, i64)> {
    let names: HashMap<Uuid, String> = profiles
        .iter()
        .map(|p| {
            let name = p.primary_name.as_ref().map(|n| fold_words(&n.display_name));
            (p.person_id, name.unwrap_or_default())
        })
        .collect();
    let records = records(profiles, &names);
    let mut found = Vec::new();
    for (at, a) in records.iter().enumerate() {
        for b in &records[at + 1..] {
            if a.key != b.key {
                continue;
            }
            let (a, b) = if a.profile.person_id < b.profile.person_id {
                (a, b)
            } else {
                (b, a)
            };
            if let Some((score, _)) = compare(a, b, &names)
                && score >= MIN_DUPLICATE_SCORE
            {
                found.push((a.profile.person_id, b.profile.person_id, score));
            }
        }
    }
    found.sort();
    found
}

/// The candidates lose no pair: on a block mixing dated and undated
/// records with every kind of clue, the pairs found are those a comparison
/// of every record with every other lists.
#[test]
fn the_candidates_lose_no_pair_a_full_comparison_lists() {
    // A deterministic generator (xorshift64*).
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut below = move |bound: u64| {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % bound
    };
    let families: Vec<Uuid> = (0..6).map(|_| Uuid::now_v7()).collect();
    let parents: Vec<PersonProfile> = ["Parent One", "Parent Two", "Parent Three"]
        .into_iter()
        .map(|given| person(given, "Elder", Sex::Unknown))
        .collect();
    let places = ["Sample Town", "Other Town", "Third Town"];
    let mut profiles: Vec<PersonProfile> = (0..400)
        .map(|_| {
            let given = if below(4) == 0 { "Givenn" } else { "Given" };
            let mut record = person(given, "Surname", Sex::Unknown);
            let place = (below(3) == 0).then(|| places[below(3) as usize]);
            match below(3) {
                // Dated, undated with a birthplace, or no birth at all.
                0 => {
                    let year = 1800 + below(12) as i32;
                    let (value, day) = if below(2) == 0 {
                        (format!("3 MAR {year}"), (year, 3, 3))
                    } else {
                        (year.to_string(), (year, 1, 1))
                    };
                    record = born(record, birth(&value, day, place));
                }
                1 => {
                    let mut event = birth("", (1, 1, 1), place);
                    event.date_value = None;
                    event.date_sort = None;
                    record = born(record, event);
                }
                _ => {}
            }
            if below(4) == 0 {
                record = died(record, 1860 + below(4) as i32);
            }
            match below(5) {
                0 => child_of(&mut record, families[below(6) as usize]),
                1 => {
                    child_of(&mut record, Uuid::now_v7());
                    let link = record.family_as_child.as_mut().unwrap();
                    link.father_id = Some(parents[below(3) as usize].person_id);
                    link.mother_id = (below(2) == 0).then(|| parents[below(3) as usize].person_id);
                }
                _ => {}
            }
            if below(5) == 0 {
                record.families_as_spouse.push(ProfileFamilyLink {
                    spouse_placeholder: None,
                    family_id: Uuid::now_v7(),
                    role: SpouseRole::Partner,
                    spouse_id: Some(parents[below(3) as usize].person_id),
                    spouse_display_name: None,
                    spouse_surname: None,
                    spouse_given_names: None,
                    spouse_sex: None,
                    events: Vec::new(),
                    children_ids: Vec::new(),
                });
            }
            record
        })
        .collect();
    profiles.extend(parents);

    let expected = compared_one_by_one(&profiles);
    assert!(
        expected.len() > MAX_DUPLICATE_PAIRS,
        "the block lists more pairs than one answer keeps: {}",
        expected.len()
    );
    let found = potential_duplicates(&profiles, &HashSet::new());
    assert_eq!(found.count as usize, expected.len(), "a pair was lost");

    // The pairs kept are the best of them all, in order.
    let mut best = expected;
    best.sort_by_key(|&(a, b, score)| (std::cmp::Reverse(score), a, b));
    best.truncate(MAX_DUPLICATE_PAIRS);
    let kept: Vec<(Uuid, Uuid, i64)> = found
        .pairs
        .iter()
        .map(|pair| (pair.first.person_id, pair.second.person_id, pair.score))
        .collect();
    assert_eq!(kept, best);
}
