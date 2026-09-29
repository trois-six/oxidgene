//! The records of the Statistics page (`docs/ui-statistics.md` §8): the one
//! person, couple or pair of siblings holding each extreme of the tree.
//!
//! Ages and durations follow the dates rule of the other figures; the
//! earliest and latest births and unions take any dated one. A record
//! nobody qualifies for is left out. Ties go to the first found, profiles
//! being read in the order they are stored and unions by family.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use oxidgene_core::Sex;
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use serde::Serialize;
use uuid::Uuid;

use super::{PersonRef, RecordDate, Tree, person_ref, record_date};

/// One record.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct StatRecord {
    /// What the record is, in snake_case: `longest_life_man`,
    /// `longest_life_woman`, `earliest_birth`, `latest_birth`,
    /// `youngest_at_union`, `oldest_at_union`, `longest_union`,
    /// `most_unions`, `first_union`, `last_union`, `most_children`,
    /// `oldest_first_child`, `youngest_first_child`, `youngest_death`,
    /// `largest_spouse_gap`, `most_places`, `longest_widowhood`,
    /// `largest_sibling_gap` or `most_generations`.
    pub kind: String,
    /// Who holds it: one person, a couple, a survivor and the spouse they
    /// outlived, or the eldest and the youngest sibling.
    pub persons: Vec<PersonRef>,
    /// Days for an age or a duration, a number for a count; absent for a
    /// record that is a date only.
    pub value: Option<f64>,
    /// The spouses the parent with the most children had them with.
    pub value2: Option<i64>,
    /// The event the record is about, when it is one.
    pub date: Option<RecordDate>,
}

fn days(from: NaiveDate, to: NaiveDate) -> f64 {
    (to - from).num_days() as f64
}

/// The item with the largest key, or the smallest; the first on a tie.
fn extreme<T>(items: impl IntoIterator<Item = (f64, T)>, largest: bool) -> Option<(f64, T)> {
    items
        .into_iter()
        .fold(None, |best, (key, item)| match best {
            Some((best_key, _))
                if (largest && key <= best_key) || (!largest && key >= best_key) =>
            {
                best
            }
            _ => Some((key, item)),
        })
}

fn record(
    kind: &str,
    persons: Vec<PersonRef>,
    value: Option<f64>,
    date: Option<&ProfileEvent>,
) -> StatRecord {
    StatRecord {
        kind: kind.to_string(),
        persons,
        value,
        value2: None,
        date: date.map(record_date),
    }
}

pub(super) fn records(
    tree: &Tree<'_>,
    children_of: &HashMap<Uuid, HashSet<Uuid>>,
) -> Vec<StatRecord> {
    let profiles = tree.profiles;
    let mut out = Vec::new();

    // Lives.
    let lives: Vec<(f64, &PersonProfile)> = profiles
        .iter()
        .filter_map(|p| {
            let age = days(tree.birth(p)?.0, tree.death(p)?.0);
            (age >= 0.0).then_some((age, p))
        })
        .collect();
    life_records(tree, &lives, &mut out);
    union_records(tree, &mut out);
    descent_records(tree, children_of, &lives, &mut out);
    place_records(profiles, &mut out);

    out
}

/// The longest lives of a man and of a woman, and the earliest and latest
/// births.
fn life_records(tree: &Tree<'_>, lives: &[(f64, &PersonProfile)], out: &mut Vec<StatRecord>) {
    let profiles = tree.profiles;
    for (kind, sex) in [
        ("longest_life_man", Sex::Male),
        ("longest_life_woman", Sex::Female),
    ] {
        let of_sex = lives.iter().filter(|(_, p)| p.sex == sex).copied();
        if let Some((age, p)) = extreme(of_sex, true) {
            out.push(record(
                kind,
                vec![person_ref(p)],
                Some(age),
                p.death_or_burial(),
            ));
        }
    }
    let born = profiles.iter().filter_map(|p| {
        let date = p.birth_or_baptism()?.date_sort?;
        Some((date.num_days_from_ce() as f64, p))
    });
    for (kind, largest) in [("earliest_birth", false), ("latest_birth", true)] {
        if let Some((_, p)) = extreme(born.clone(), largest) {
            out.push(record(
                kind,
                vec![person_ref(p)],
                None,
                p.birth_or_baptism(),
            ));
        }
    }
}

/// The ages at union, the longest union, the most unions, the first and last
/// unions, the largest spouse gap and the longest widowhood.
fn union_records(tree: &Tree<'_>, out: &mut Vec<StatRecord>) {
    let profiles = tree.profiles;
    let at_union: Vec<(f64, (&PersonProfile, &ProfileEvent))> = tree
        .unions
        .iter()
        .filter_map(|u| Some((tree.dates.date(u.date)?, u.date?, u)))
        .flat_map(|(date, event, union)| {
            union.spouses.iter().filter_map(move |(id, _)| {
                let age = days(tree.born(id)?, date);
                let spouse = tree.person(id)?;
                (age > 0.0).then_some((age, (spouse, event)))
            })
        })
        .collect();
    for (kind, largest) in [("youngest_at_union", false), ("oldest_at_union", true)] {
        if let Some((age, (p, event))) = extreme(at_union.iter().copied(), largest) {
            out.push(record(kind, vec![person_ref(p)], Some(age), Some(event)));
        }
    }
    let lasting = tree.unions.iter().filter_map(|u| {
        let date = tree.dates.date(u.date)?;
        let end = tree.union_end(u)?;
        (end >= date).then(|| (days(date, end), u))
    });
    if let Some((length, u)) = extreme(lasting, true) {
        out.push(record(
            "longest_union",
            tree.spouses(u),
            Some(length),
            u.date,
        ));
    }
    let unions_of = profiles
        .iter()
        .map(|p| (p.families_as_spouse.len() as f64, p));
    if let Some((count, p)) = extreme(unions_of, true).filter(|(count, _)| *count > 1.0) {
        out.push(record(
            "most_unions",
            vec![person_ref(p)],
            Some(count),
            None,
        ));
    }
    let dated = tree.unions.iter().filter_map(|u| {
        let date = u.date?.date_sort?;
        Some((date.num_days_from_ce() as f64, u))
    });
    for (kind, largest) in [("first_union", false), ("last_union", true)] {
        if let Some((_, u)) = extreme(dated.clone(), largest) {
            out.push(record(kind, tree.spouses(u), None, u.date));
        }
    }
    let gaps = tree.unions.iter().filter_map(|u| {
        let [(a, _), (b, _)] = u.spouses.as_slice() else {
            return None;
        };
        Some((days(tree.born(a)?, tree.born(b)?).abs(), u))
    });
    if let Some((gap, u)) = extreme(gaps, true) {
        out.push(record(
            "largest_spouse_gap",
            tree.spouses(u),
            Some(gap),
            u.date,
        ));
    }
    // A spouse outlived by the other: the union is dated and came before
    // the first death (a spouse dead before it is a data error, not a
    // widowhood), and no divorce parted them first.
    let widowhoods = tree.unions.iter().filter_map(|u| {
        let [(a, _), (b, _)] = u.spouses.as_slice() else {
            return None;
        };
        let married = tree.dates.date(u.date)?;
        let (died_a, died_b) = (tree.died(a)?, tree.died(b)?);
        let (first, survivor, deceased) = if died_a <= died_b {
            (died_a, b, a)
        } else {
            (died_b, a, b)
        };
        if first < married || u.end.is_some_and(|end| end < first) {
            return None;
        }
        let last = died_a.max(died_b);
        Some((days(first, last), (survivor, deceased)))
    });
    if let Some((length, (survivor, deceased))) = extreme(widowhoods, true) {
        let persons = [survivor, deceased]
            .into_iter()
            .filter_map(|id| tree.person(id))
            .map(person_ref)
            .collect();
        out.push(record("longest_widowhood", persons, Some(length), None));
    }
}

/// The most children, the ages at a first child, the youngest death, the
/// largest sibling gap and the most generations of descendants.
fn descent_records(
    tree: &Tree<'_>,
    children_of: &HashMap<Uuid, HashSet<Uuid>>,
    lives: &[(f64, &PersonProfile)],
    out: &mut Vec<StatRecord>,
) {
    let profiles = tree.profiles;
    let parents_of_most = profiles.iter().filter_map(|p| {
        let children = children_of.get(&p.person_id)?;
        Some((children.len() as f64, p))
    });
    if let Some((count, p)) = extreme(parents_of_most, true) {
        let spouses = tree
            .unions
            .iter()
            .filter(|u| !u.children.is_empty())
            .filter(|u| u.spouses.iter().any(|(id, _)| *id == p.person_id))
            .count();
        let mut most = record("most_children", vec![person_ref(p)], Some(count), None);
        most.value2 = Some(spouses as i64);
        out.push(most);
    }
    let first_children: Vec<(f64, (&PersonProfile, &PersonProfile))> = profiles
        .iter()
        .filter_map(|parent| {
            let parent_born = tree.birth(parent)?.0;
            let (child_born, child) = children_of
                .get(&parent.person_id)?
                .iter()
                .filter_map(|id| Some((tree.born(id)?, tree.person(id)?)))
                .min_by_key(|(date, child)| (*date, child.person_id))?;
            let age = days(parent_born, child_born);
            (age > 0.0).then_some((age, (parent, child)))
        })
        .collect();
    for (kind, largest) in [
        ("oldest_first_child", true),
        ("youngest_first_child", false),
    ] {
        if let Some((age, (parent, child))) = extreme(first_children.iter().copied(), largest) {
            out.push(record(
                kind,
                vec![person_ref(parent)],
                Some(age),
                child.birth_or_baptism(),
            ));
        }
    }
    if let Some((age, p)) = extreme(lives.iter().copied(), false) {
        out.push(record(
            "youngest_death",
            vec![person_ref(p)],
            Some(age),
            p.death_or_burial(),
        ));
    }
    let mut siblings: HashMap<Uuid, Vec<(NaiveDate, &PersonProfile)>> = HashMap::new();
    for profile in profiles {
        if let (Some(link), Some((born, _))) = (&profile.family_as_child, tree.birth(profile)) {
            siblings
                .entry(link.family_id)
                .or_default()
                .push((born, profile));
        }
    }
    let mut families: Vec<_> = siblings.into_iter().collect();
    families.sort_by_key(|(family, _)| *family);
    let spreads = families.iter().filter_map(|(_, children)| {
        let eldest = children.iter().min_by_key(|(d, p)| (*d, p.person_id))?;
        let youngest = children.iter().max_by_key(|(d, p)| (*d, p.person_id))?;
        (children.len() > 1).then(|| (days(eldest.0, youngest.0), (eldest.1, youngest.1)))
    });
    if let Some((gap, (eldest, youngest))) = extreme(spreads, true) {
        out.push(record(
            "largest_sibling_gap",
            vec![person_ref(eldest), person_ref(youngest)],
            Some(gap),
            None,
        ));
    }
    let depths = generations(tree, children_of);
    let deepest = profiles
        .iter()
        .filter_map(|p| Some((f64::from(*depths.get(&p.person_id)?), p)));
    if let Some((depth, p)) = extreme(deepest, true) {
        out.push(record(
            "most_generations",
            vec![person_ref(p)],
            Some(depth),
            None,
        ));
    }
}

/// The person whose events took place in the most places.
fn place_records(profiles: &[PersonProfile], out: &mut Vec<StatRecord>) {
    let travelled = profiles.iter().map(|p| {
        let places: HashSet<Uuid> = [&p.birth, &p.baptism, &p.death, &p.burial]
            .into_iter()
            .flatten()
            .chain(&p.other_events)
            .chain(
                p.families_as_spouse
                    .iter()
                    .flat_map(|link| link.marriage.iter().chain(&link.events)),
            )
            .filter_map(|e| e.place_id)
            .collect();
        (places.len() as f64, p)
    });
    if let Some((count, p)) = extreme(travelled, true).filter(|(count, _)| *count > 0.0) {
        out.push(record(
            "most_places",
            vec![person_ref(p)],
            Some(count),
            None,
        ));
    }
}

/// How many generations of descendants each parent has, from their
/// children (one) down. A loop in the data counts each person once.
fn generations(tree: &Tree<'_>, children_of: &HashMap<Uuid, HashSet<Uuid>>) -> HashMap<Uuid, u32> {
    fn depth(
        person: Uuid,
        children_of: &HashMap<Uuid, HashSet<Uuid>>,
        known: &mut HashMap<Uuid, u32>,
        open: &mut HashSet<Uuid>,
    ) -> u32 {
        if let Some(depth) = known.get(&person) {
            return *depth;
        }
        let Some(children) = children_of.get(&person) else {
            return 0;
        };
        if !open.insert(person) {
            return 0;
        }
        let deepest = children
            .iter()
            .map(|child| depth(*child, children_of, known, open))
            .max()
            .unwrap_or(0);
        open.remove(&person);
        known.insert(person, deepest + 1);
        deepest + 1
    }
    let mut known = HashMap::new();
    let mut open = HashSet::new();
    for profile in tree.profiles {
        depth(profile.person_id, children_of, &mut known, &mut open);
    }
    known
}
