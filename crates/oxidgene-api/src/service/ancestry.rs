//! Ancestry completeness: generation by generation from the tree's SOSA
//! root, which ancestors are known and which of their key facts are
//! recorded (`docs/ui-tools.md` §4).
//!
//! Computed on each request from the person projections; nothing is stored.
//! Only the ancestors found, and the missing parents of those found, are
//! listed: an unknown ancestor's own ancestors are unknown too, and are
//! counted per generation instead of listed one by one, so an empty branch
//! costs nothing however deep the request goes.

use std::collections::HashMap;

use chrono::NaiveDate;
use oxidgene_core::OxidGeneError;
use oxidgene_core::projection::{PersonProfile, ProfileEvent};
use serde::Serialize;
use uuid::Uuid;

use crate::service::statistics::{PersonRef, RecordDate, person_ref, possibly_alive, record_date};

/// Generations shown when the request names none: the root and seven
/// generations of ancestors.
pub const DEFAULT_GENERATIONS: u32 = 8;
/// The most generations one request may ask for: 16,384 ancestors at the
/// last one, enough for any tree and bounded for the server.
pub const MAX_GENERATIONS: u32 = 15;

/// A tree's ancestry completeness.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AncestryCompleteness {
    /// The SOSA root, `None` when the tree has none.
    pub root: Option<PersonRef>,
    /// One per generation, the root's first, up to the generations asked
    /// for; empty without a root.
    pub generations: Vec<AncestryGeneration>,
}

/// One generation of ancestors.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AncestryGeneration {
    /// 1 for the root, 2 for the parents…
    pub generation: i32,
    /// The ancestors a complete tree has there: 2^(generation − 1).
    pub expected: i64,
    /// Those known. An ancestor reached through two lines (pedigree
    /// collapse) counts at each of their SOSA numbers.
    pub found: i64,
    /// Found ancestors with a birth or a baptism, with a death or a burial,
    /// and with a union to the other parent of their child (§4).
    pub with_birth: i64,
    pub with_death: i64,
    pub with_union: i64,
    /// Found ancestors with no death recorded who may be alive.
    pub living: i64,
    /// Ancestors missing because their child is missing too: implied, not
    /// listed in `entries`.
    pub implied_missing: i64,
    /// The found ancestors, and the missing parents of the previous
    /// generation's found ones, by SOSA number.
    pub entries: Vec<AncestryEntry>,
}

/// One SOSA number of a generation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AncestryEntry {
    pub sosa: i64,
    /// `None` for a missing ancestor.
    pub person: Option<AncestorFacts>,
}

/// A found ancestor and which key facts are recorded.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[cfg_attr(feature = "graphql", derive(async_graphql::SimpleObject))]
pub struct AncestorFacts {
    pub person_id: String,
    pub name: String,
    /// `Sex` in its snake_case form.
    pub sex: String,
    /// The birth, or the baptism, when it carries a date.
    pub birth: Option<RecordDate>,
    /// The death, or the burial, when it carries a date.
    pub death: Option<RecordDate>,
    pub has_birth: bool,
    pub has_death: bool,
    pub has_union: bool,
    pub living: bool,
}

/// Validates a requested depth: `None` for the default, else between 1 and
/// [`MAX_GENERATIONS`].
pub fn generations(requested: Option<i64>) -> Result<u32, OxidGeneError> {
    match requested {
        None => Ok(DEFAULT_GENERATIONS),
        Some(n) if (1..=i64::from(MAX_GENERATIONS)).contains(&n) => Ok(n as u32),
        Some(_) => Err(OxidGeneError::Validation(format!(
            "generations must be between 1 and {MAX_GENERATIONS}"
        ))),
    }
}

/// Loads a tree's projections and computes its ancestry completeness from
/// its SOSA root.
#[tracing::instrument(name = "ancestry.load", skip_all, fields(ancestry.generations = generations))]
pub async fn load(
    db: &sea_orm::DatabaseConnection,
    profiles: &crate::profile::ProfileService,
    tree_id: Uuid,
    generations: u32,
) -> Result<AncestryCompleteness, OxidGeneError> {
    let tree = oxidgene_db::repo::TreeRepo::get(db, tree_id).await?;
    let Some(root) = tree.sosa_root_person_id else {
        return Ok(AncestryCompleteness {
            root: None,
            generations: Vec::new(),
        });
    };
    let persons = profiles.get_all_persons(tree_id).await?;
    let today = chrono::Utc::now().date_naive();
    let span = tracing::info_span!("ancestry.compute", person.count = persons.len());
    crate::service::blocking::run(span, move || compute(&persons, root, generations, today)).await
}

/// Recorded: the event exists with a date or a place. A dateless,
/// placeless stub says nothing a researcher can use.
fn recorded(event: Option<&ProfileEvent>) -> bool {
    event.is_some_and(|e| e.date_value.is_some() || e.place_id.is_some())
}

/// The ancestry completeness of `root` over `generations` generations.
pub fn compute(
    profiles: &[PersonProfile],
    root: Uuid,
    generations: u32,
    today: NaiveDate,
) -> AncestryCompleteness {
    let by_id: HashMap<Uuid, &PersonProfile> = profiles.iter().map(|p| (p.person_id, p)).collect();
    let Some(root_profile) = by_id.get(&root).copied() else {
        // A root that no longer exists is no root.
        return AncestryCompleteness {
            root: None,
            generations: Vec::new(),
        };
    };

    // Each slot: its SOSA number, the person there, and the family that
    // makes them a parent of the slot below (none for the root).
    let mut slots: Vec<(i64, Option<&PersonProfile>, Option<Uuid>)> =
        vec![(1, Some(root_profile), None)];
    let mut out = Vec::new();
    for generation in 1..=generations as i32 {
        let expected = 1_i64 << (generation - 1);
        let mut row = AncestryGeneration {
            generation,
            expected,
            found: 0,
            with_birth: 0,
            with_death: 0,
            with_union: 0,
            living: 0,
            implied_missing: expected - slots.len() as i64,
            entries: Vec::with_capacity(slots.len()),
        };
        let mut next = Vec::new();
        for (sosa, person, family) in &slots {
            let Some(person) = person else {
                row.entries.push(AncestryEntry {
                    sosa: *sosa,
                    person: None,
                });
                continue;
            };
            let facts = facts(person, *family, today);
            row.found += 1;
            row.with_birth += i64::from(facts.has_birth);
            row.with_death += i64::from(facts.has_death);
            row.with_union += i64::from(facts.has_union);
            row.living += i64::from(facts.living);
            row.entries.push(AncestryEntry {
                sosa: *sosa,
                person: Some(facts),
            });
            let parents = person.family_as_child.as_ref();
            let family = parents.map(|link| link.family_id);
            let parent = |id: Option<Uuid>| id.and_then(|id| by_id.get(&id).copied());
            next.push((
                sosa * 2,
                parent(parents.and_then(|link| link.father_id)),
                family,
            ));
            next.push((
                sosa * 2 + 1,
                parent(parents.and_then(|link| link.mother_id)),
                family,
            ));
        }
        out.push(row);
        slots = next;
    }

    AncestryCompleteness {
        root: Some(person_ref(root_profile)),
        generations: out,
    }
}

/// What is recorded of an ancestor. `family` is the union that makes them
/// a parent of the ancestor below; for the root, any of their unions counts.
fn facts(person: &PersonProfile, family: Option<Uuid>, today: NaiveDate) -> AncestorFacts {
    let dated =
        |event: Option<&ProfileEvent>| event.filter(|e| e.date_value.is_some()).map(record_date);
    let has_union = person
        .families_as_spouse
        .iter()
        .filter(|link| family.is_none_or(|family| link.family_id == family))
        .flat_map(|link| link.events.iter().chain(link.marriage.as_ref()))
        .any(|event| event.event_type.attests_union() && recorded(Some(event)));
    let has_death = recorded(person.death.as_ref()) || recorded(person.burial.as_ref());
    AncestorFacts {
        person_id: person.person_id.to_string(),
        name: person_ref(person).name,
        sex: person.sex.to_string(),
        birth: dated(person.birth_or_baptism()),
        death: dated(person.death_or_burial()),
        has_birth: recorded(person.birth.as_ref()) || recorded(person.baptism.as_ref()),
        has_death,
        has_union,
        living: !has_death && possibly_alive(person, today),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use oxidgene_core::projection::{ProfileChildLink, ProfileFamilyLink, ProfileName};
    use oxidgene_core::{Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole};

    use super::*;

    fn event(event_type: EventType, date: Option<&str>) -> ProfileEvent {
        ProfileEvent {
            event_id: Uuid::now_v7(),
            event_type,
            date_value: date.map(str::to_string),
            date_sort: date
                .and_then(|d| NaiveDate::parse_from_str(&format!("{d}-01-01"), "%Y-%m-%d").ok()),
            date_qualifier: DateQualifier::Exact,
            date_value2: None,
            calendar: Calendar::Gregorian,
            place_name: None,
            place_id: None,
            description: None,
            age: None,
        }
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

    fn child_of(
        child: &mut PersonProfile,
        family_id: Uuid,
        father: Option<Uuid>,
        mother: Option<Uuid>,
    ) {
        child.family_as_child = Some(ProfileChildLink {
            family_id,
            child_type: ChildType::Biological,
            father_id: father,
            father_display_name: None,
            father_surname: None,
            father_given_names: None,
            mother_id: mother,
            mother_display_name: None,
            mother_surname: None,
            mother_given_names: None,
        });
    }

    fn spouse_in(
        person: &mut PersonProfile,
        family_id: Uuid,
        role: SpouseRole,
        events: Vec<ProfileEvent>,
    ) {
        person.families_as_spouse.push(ProfileFamilyLink {
            family_id,
            role,
            spouse_id: None,
            spouse_display_name: None,
            spouse_surname: None,
            spouse_given_names: None,
            spouse_sex: None,
            marriage: None,
            events,
            children_ids: Vec::new(),
            children_count: 0,
        });
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }

    /// Root, a father with his birth and a union with the root's mother,
    /// no mother, and a paternal grandfather: the mother's own parents are
    /// implied, not listed.
    #[test]
    fn missing_ancestors_are_listed_once_and_their_branch_counted() {
        let family = Uuid::now_v7();
        let grand_family = Uuid::now_v7();
        let mut root = person("Child", Sex::Female);
        root.birth = Some(event(EventType::Birth, Some("1990")));
        let mut father = person("Father", Sex::Male);
        father.birth = Some(event(EventType::Birth, Some("1960")));
        spouse_in(
            &mut father,
            family,
            SpouseRole::Husband,
            vec![event(EventType::Marriage, Some("1985"))],
        );
        let grandfather = person("Grandfather", Sex::Male);
        child_of(&mut root, family, Some(father.person_id), None);
        child_of(&mut father, grand_family, Some(grandfather.person_id), None);
        let root_id = root.person_id;
        let profiles = vec![root, father, grandfather];

        let result = compute(&profiles, root_id, 4, today());
        assert_eq!(
            result.root.as_ref().map(|r| r.name.as_str()),
            Some("Child BRANCH_A")
        );
        let g = &result.generations;
        assert_eq!(g.len(), 4);
        // The root is alive: born 36 years ago with no death.
        assert_eq!((g[0].found, g[0].living, g[0].with_birth), (1, 1, 1));
        // Parents: the father with his birth and the union, the mother missing.
        assert_eq!((g[1].expected, g[1].found, g[1].implied_missing), (2, 1, 0));
        assert_eq!(g[1].with_birth, 1);
        assert_eq!(g[1].with_union, 1);
        let sosas: Vec<_> = g[1]
            .entries
            .iter()
            .map(|e| (e.sosa, e.person.is_some()))
            .collect();
        assert_eq!(sosas, vec![(2, true), (3, false)]);
        // Grandparents: 4 found, 5 missing and listed, 6 and 7 implied.
        let sosas: Vec<_> = g[2]
            .entries
            .iter()
            .map(|e| (e.sosa, e.person.is_some()))
            .collect();
        assert_eq!(sosas, vec![(4, true), (5, false)]);
        assert_eq!((g[2].found, g[2].implied_missing), (1, 2));
        // The grandfather's union with no one recorded is missing.
        assert_eq!(g[2].with_union, 0);
        // Great-grandparents: only the grandfather's two parents are listed.
        assert_eq!(g[3].entries.len(), 2);
        assert_eq!((g[3].found, g[3].implied_missing), (0, 6));
    }

    #[test]
    fn a_tree_whose_root_is_gone_has_no_ancestry() {
        let result = compute(&[], Uuid::now_v7(), 8, today());
        assert_eq!(result.root, None);
        assert!(result.generations.is_empty());
    }

    #[test]
    fn the_depth_is_bounded() {
        assert_eq!(generations(None).unwrap(), DEFAULT_GENERATIONS);
        assert_eq!(generations(Some(15)).unwrap(), 15);
        assert!(generations(Some(0)).is_err());
        assert!(generations(Some(16)).is_err());
    }
}
