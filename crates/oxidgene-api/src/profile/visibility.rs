//! Keeping `person_visibility` current (docs/privacy.md §4.5): the stored
//! inputs of the contemporary rule, refreshed in the transaction of every
//! write that changes them. Web build only: the desktop has no viewer to
//! redact for, and its export applies the rule to the records it reads.
//!
//! A person's row depends on their own records, on those of their parents,
//! spouses and siblings, and on their descendants' recorded dates down to
//! ten generations. The descendants' part is stored by depth
//! ([`Descent`]), so a write refreshes:
//!
//! 1. the persons it touched and their relatives one link away, from their
//!    records;
//! 2. then, generation by generation, the descent of their parents from the
//!    children's stored rows, going up only while a descent changes, and at
//!    most ten generations: one bounded walk of the ancestors, never a walk
//!    down from each of them.
//!
//! A tree whose rows are missing or come from an earlier version of the
//! rule is rebuilt whole by the next read that needs them; writes before
//! that touch nothing.

use std::collections::{HashMap, HashSet};

use oxidgene_core::OxidGeneError;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::privacy::{
    DESCENT_DEPTH, Descent, FamilyMembers, Kinship, PersonVisibility, derive_records, evidence_of,
    person_visibility,
};
use oxidgene_core::types::FamilySpouse;
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, Membership, MembershipsOf,
    PersonRepo, PersonVisibilityRepo,
};
use sea_orm::ConnectionTrait;
use tracing::debug;
use uuid::Uuid;

use super::builder::TreeData;

/// Rebuild every row of `tree_id` from `data`, the records a full
/// projection rebuild read.
pub(crate) async fn rebuild_from(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    data: &TreeData,
) -> Result<usize, OxidGeneError> {
    let rows = derive_records(&data.persons, &data.events, &data.spouses, &data.children);
    let rows: Vec<(Uuid, PersonVisibility)> = rows.into_iter().collect();
    PersonVisibilityRepo::replace_tree(conn, tree_id, &rows).await?;
    Ok(rows.len())
}

/// Rebuild every row of `tree_id` from its records.
pub(crate) async fn rebuild_tree(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<usize, OxidGeneError> {
    let (persons, events, families) = tokio::try_join!(
        PersonRepo::list_all(conn, tree_id),
        EventRepo::list_all(conn, tree_id),
        FamilyRepo::list_all(conn, tree_id),
    )?;
    let family_ids: Vec<Uuid> = families.iter().map(|f| f.id).collect();
    let (spouses, children) = tokio::try_join!(
        FamilySpouseRepo::list_by_families(conn, &family_ids),
        FamilyChildRepo::list_by_families(conn, &family_ids),
    )?;
    let rows: Vec<(Uuid, PersonVisibility)> =
        derive_records(&persons, &events, &spouses, &children)
            .into_iter()
            .collect();
    PersonVisibilityRepo::replace_tree(conn, tree_id, &rows).await?;
    debug!(count = rows.len(), "Rebuilt the person visibility");
    Ok(rows.len())
}

/// The live families `links` name, with their live members.
fn families(links: &[Membership]) -> HashMap<Uuid, FamilyMembers> {
    let mut families: HashMap<Uuid, FamilyMembers> = HashMap::new();
    for link in links {
        let family = families.entry(link.family_id).or_default();
        if link.spouse {
            family.spouses.push(link.person_id);
        } else {
            family.children.push(link.person_id);
        }
    }
    for family in families.values_mut() {
        family.spouses.sort_unstable();
        family.spouses.dedup();
        family.children.sort_unstable();
        family.children.dedup();
    }
    families
}

/// The families `persons` belong to, with every live member of each: two
/// grouped reads.
async fn families_around(
    conn: &impl ConnectionTrait,
    persons: &[Uuid],
) -> Result<HashMap<Uuid, FamilyMembers>, OxidGeneError> {
    let own = PersonVisibilityRepo::memberships(conn, persons, MembershipsOf::Persons).await?;
    let ids = sorted_unique(own.iter().map(|link| link.family_id));
    let members = PersonVisibilityRepo::memberships(conn, &ids, MembershipsOf::Families).await?;
    Ok(families(&members))
}

/// The rows a refresh read and those it changed.
#[derive(Default)]
struct Refresh {
    rows: HashMap<Uuid, PersonVisibility>,
    touched: HashSet<Uuid>,
}

impl Refresh {
    /// Set `person`'s row, remembering it is to be written when it changed.
    fn set(&mut self, person: Uuid, row: PersonVisibility) {
        if self.rows.get(&person) != Some(&row) {
            self.touched.insert(person);
        }
        self.rows.insert(person, row);
    }
}

/// Refresh the rows `seeds` affect, in the caller's transaction: the seeds'
/// and their close relatives' from their records, then their ancestors'
/// descents (see the module documentation). The seeds are the persons a
/// write's projection refresh rewrites; a deleted seed's row goes.
pub(crate) async fn refresh(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    seeds: &[Uuid],
) -> Result<(), OxidGeneError> {
    if seeds.is_empty() || !PersonVisibilityRepo::is_materialized(conn, tree_id).await? {
        return Ok(());
    }
    let mut refresh = Refresh::default();
    let (near, kin, recorded_moved) = refresh_near(conn, seeds, &mut refresh).await?;
    // The first frontier: every person whose children may have changed —
    // the near persons, whose own links a write touches — and the parents
    // of the near persons.
    let frontier = sorted_unique(
        near.iter()
            .copied()
            .chain(near.iter().flat_map(|&p| kin.parents_of(p).iter().copied())),
    );
    propagate(conn, &mut refresh, frontier, recorded_moved).await?;
    let writes: Vec<(Uuid, PersonVisibility)> = refresh
        .touched
        .iter()
        .filter_map(|person| refresh.rows.get(person).map(|row| (*person, *row)))
        .collect();
    debug!(count = writes.len(), "Refreshed the person visibility");
    PersonVisibilityRepo::upsert(conn, tree_id, &writes).await
}

/// Step 1: the seeds and every member of their families, from their records
/// and their relatives' one link away — what bounds them, and what they
/// bound. Deleted seeds lose their row. The live persons refreshed, how
/// their families link them, and those whose recorded date moved.
async fn refresh_near(
    conn: &impl ConnectionTrait,
    seeds: &[Uuid],
    refresh: &mut Refresh,
) -> Result<(Vec<Uuid>, Kinship, Vec<Uuid>), OxidGeneError> {
    let members = |families: &HashMap<Uuid, FamilyMembers>| -> Vec<Uuid> {
        families
            .values()
            .flat_map(|f| f.spouses.iter().chain(&f.children))
            .copied()
            .collect()
    };
    let around_seeds = families_around(conn, seeds).await?;
    let near = sorted_unique(seeds.iter().copied().chain(members(&around_seeds)));
    let privacy = PersonVisibilityRepo::live_privacy(conn, &near).await?;
    let gone: Vec<Uuid> = seeds
        .iter()
        .copied()
        .filter(|id| !privacy.contains_key(id))
        .collect();
    PersonVisibilityRepo::delete_persons(conn, &gone).await?;
    let near: Vec<Uuid> = near
        .into_iter()
        .filter(|id| privacy.contains_key(id))
        .collect();

    let around_near = families_around(conn, &near).await?;
    let kin = Kinship::of_families(around_near.values());
    let involved = sorted_unique(near.iter().copied().chain(members(&around_near)));
    let family_ids: Vec<Uuid> = around_near.keys().copied().collect();
    let events = EventRepo::list_untagged_by_owners(conn, &involved, &family_ids).await?;
    let spouse_links: Vec<FamilySpouse> = around_near
        .iter()
        .flat_map(|(family_id, family)| {
            family.spouses.iter().map(|&person_id| FamilySpouse {
                id: Uuid::nil(),
                family_id: *family_id,
                person_id,
                role: oxidgene_core::enums::SpouseRole::Partner,
                sort_order: 0,
            })
        })
        .collect();
    let evidence = evidence_of(&events, &spouse_links, &involved.iter().copied().collect());

    refresh.rows = PersonVisibilityRepo::get_many(conn, &involved).await?;
    let mut recorded_moved = Vec::new();
    for &person in &near {
        let stored = refresh.rows.get(&person).copied();
        let mut row = person_visibility(person, privacy[&person], &evidence, &kin);
        row.set_descent(stored.map(|row| row.descent).unwrap_or_default());
        if stored.map(|row| row.recorded_birth_year) != Some(row.recorded_birth_year) {
            recorded_moved.push(person);
        }
        refresh.set(person, row);
    }
    Ok((near, kin, recorded_moved))
}

/// Step 2: up the ancestry, a generation per round, the descent of each
/// person of `frontier` from their children's rows; the next frontier is
/// the parents of those whose recorded date (`moved`, the first round) or
/// descent changed. A change reaches ten generations at most, so the walk
/// stops there even through a cycle of corrupt links.
async fn propagate(
    conn: &impl ConnectionTrait,
    refresh: &mut Refresh,
    mut frontier: Vec<Uuid>,
    mut moved: Vec<Uuid>,
) -> Result<(), OxidGeneError> {
    for _ in 0..=DESCENT_DEPTH + 1 {
        if frontier.is_empty() {
            break;
        }
        let around = families_around(conn, &frontier).await?;
        let kin = Kinship::of_families(around.values());
        let missing: Vec<Uuid> = around
            .values()
            .flat_map(|f| f.spouses.iter().chain(&f.children))
            .chain(&frontier)
            .copied()
            .filter(|id| !refresh.rows.contains_key(id))
            .collect();
        refresh
            .rows
            .extend(PersonVisibilityRepo::get_many(conn, &sorted_unique(missing)).await?);
        for &person in &frontier {
            let Some(mut row) = refresh.rows.get(&person).copied() else {
                continue;
            };
            let children: Vec<(Option<i32>, Descent)> = kin
                .children_of(person)
                .iter()
                .filter_map(|child| refresh.rows.get(child))
                .map(|child| (child.recorded_birth_year, child.descent))
                .collect();
            let descent = Descent::of_children(children.iter().map(|(year, d)| (*year, d)));
            if descent != row.descent {
                row.set_descent(descent);
                refresh.set(person, row);
                moved.push(person);
            }
        }
        // The parents of the persons that moved: from the links just read
        // when they were part of the frontier, else read for them.
        let unread: Vec<Uuid> = moved
            .iter()
            .copied()
            .filter(|person| !frontier.contains(person))
            .collect();
        let extra = Kinship::of_families(families_around(conn, &unread).await?.values());
        frontier = sorted_unique(moved.drain(..).flat_map(|person| {
            kin.parents_of(person)
                .iter()
                .chain(extra.parents_of(person))
                .copied()
                .collect::<Vec<_>>()
        }));
    }
    Ok(())
}
