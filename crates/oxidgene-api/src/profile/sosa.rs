//! What a tree's stored SOSA ancestry holds (docs/data-model.md §4.8): its
//! root and every ancestor of the root, with their SOSA-Stradonitz numbers.
//!
//! Built from the family links, one generation per query: only the root's
//! ancestry is read, never the rest of the tree.

use std::collections::{HashMap, HashSet};

use oxidgene_core::OxidGeneError;
use oxidgene_core::enums::SpouseRole;
use oxidgene_db::repo::{
    MAX_GENERATIONS, ParentLink, PersonRepo, SosaAncestor, SosaRepo, TreeRepo,
};
use sea_orm::ConnectionTrait;
use uuid::Uuid;

/// The deepest generation numbered: the signed 64-bit integers SQLite and
/// PostgreSQL share stop at `i64::MAX`, the largest number of generation 62.
const MAX_SOSA_GENERATIONS: u32 = 62;

/// The SOSA ancestry of `tree_id`, from the family links as `conn` sees
/// them: empty when the tree has no root, or a root that is not a live
/// person of the tree.
pub(crate) async fn ancestry(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
) -> Result<Vec<SosaAncestor>, OxidGeneError> {
    let Some(root) = TreeRepo::get(conn, tree_id).await?.sosa_root_person_id else {
        return Ok(Vec::new());
    };
    match PersonRepo::get_in_tree(conn, tree_id, root).await {
        Ok(_) => {}
        Err(OxidGeneError::NotFound { .. }) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }

    // Bounded, like every ancestry walk: the schema does not prevent a cycle.
    let mut links = Vec::new();
    let mut seen = HashSet::from([root]);
    let mut generation = vec![root];
    for _ in 0..MAX_GENERATIONS {
        if generation.is_empty() {
            break;
        }
        let parents = SosaRepo::parent_links(conn, &generation).await?;
        generation = parents
            .iter()
            .map(|link| link.parent_id)
            .filter(|parent| seen.insert(*parent))
            .collect();
        links.extend(parents);
    }
    Ok(number(root, &links))
}

/// The root and every parent `links` reaches from it, each with its SOSA
/// number: the root is 1, a husband `2n` and a wife `2n + 1` of the child
/// numbered `n`. Through pedigree implex a person is reached by several
/// paths; the shortest wins, then the lowest number. A parent who is a
/// family's partner, neither husband nor wife, is an ancestor without a
/// number, and so are the ancestors reached only through them.
fn number(root: Uuid, links: &[ParentLink]) -> Vec<SosaAncestor> {
    let mut parents: HashMap<Uuid, Vec<(Uuid, u64)>> = HashMap::new();
    for link in links {
        let side = match link.role {
            SpouseRole::Husband => 0,
            SpouseRole::Wife => 1,
            SpouseRole::Partner => continue,
        };
        parents
            .entry(link.child_id)
            .or_default()
            .push((link.parent_id, side));
    }

    // One generation at a time, so the first generation that reaches a
    // person is their shortest path; within it, the lowest number comes
    // from the lowest-numbered child, since `2n + side` grows with `n`.
    let mut numbers = HashMap::from([(root, 1_u64)]);
    let mut generation = vec![(root, 1_u64)];
    for _ in 0..MAX_SOSA_GENERATIONS {
        let mut next: HashMap<Uuid, u64> = HashMap::new();
        for (child, n) in &generation {
            for (parent, side) in parents.get(child).into_iter().flatten() {
                if numbers.contains_key(parent) {
                    continue;
                }
                let candidate = 2 * n + side;
                next.entry(*parent)
                    .and_modify(|best| *best = (*best).min(candidate))
                    .or_insert(candidate);
            }
        }
        if next.is_empty() {
            break;
        }
        numbers.extend(&next);
        generation = next.into_iter().collect();
    }

    let mut members: Vec<Uuid> = std::iter::once(root)
        .chain(links.iter().map(|link| link.parent_id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    members.sort_unstable();
    members
        .into_iter()
        .map(|person_id| SosaAncestor {
            person_id,
            sosa_number: numbers.get(&person_id).copied(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn link(child: u128, parent: u128, role: SpouseRole) -> ParentLink {
        ParentLink {
            child_id: id(child),
            parent_id: id(parent),
            role,
        }
    }

    fn numbers(root: u128, links: &[ParentLink]) -> HashMap<Uuid, Option<u64>> {
        number(id(root), links)
            .into_iter()
            .map(|ancestor| (ancestor.person_id, ancestor.sosa_number))
            .collect()
    }

    #[test]
    fn husbands_double_wives_double_plus_one_and_partners_have_no_number() {
        let found = numbers(
            1,
            &[
                link(1, 2, SpouseRole::Husband),
                link(1, 3, SpouseRole::Wife),
                link(1, 4, SpouseRole::Partner),
                link(2, 5, SpouseRole::Husband),
                link(3, 6, SpouseRole::Wife),
                // Reached only through a partner: an ancestor, unnumbered.
                link(4, 7, SpouseRole::Husband),
            ],
        );
        assert_eq!(found[&id(1)], Some(1));
        assert_eq!(found[&id(2)], Some(2));
        assert_eq!(found[&id(3)], Some(3));
        assert_eq!(found[&id(4)], None);
        assert_eq!(found[&id(5)], Some(4));
        assert_eq!(found[&id(6)], Some(7));
        assert_eq!(found[&id(7)], None);
        assert_eq!(found.len(), 7);
    }

    #[test]
    fn implex_keeps_the_shortest_path_then_the_lowest_number() {
        // Person 9 is the root's mother's mother (7) and, through the
        // father's side, a great-grandmother too (2 → 4 → 9 as 9).
        let found = numbers(
            1,
            &[
                link(1, 2, SpouseRole::Husband),
                link(1, 3, SpouseRole::Wife),
                link(2, 4, SpouseRole::Husband),
                link(4, 9, SpouseRole::Wife),
                link(3, 9, SpouseRole::Wife),
            ],
        );
        assert_eq!(found[&id(9)], Some(7));

        // The same person as both father and mother of one generation.
        let found = numbers(
            1,
            &[
                link(1, 2, SpouseRole::Wife),
                link(1, 2, SpouseRole::Husband),
            ],
        );
        assert_eq!(found[&id(2)], Some(2));
    }

    #[test]
    fn a_cycle_in_the_links_ends() {
        let found = numbers(
            1,
            &[
                link(1, 2, SpouseRole::Husband),
                link(2, 1, SpouseRole::Husband),
            ],
        );
        assert_eq!(found[&id(1)], Some(1));
        assert_eq!(found[&id(2)], Some(2));
    }

    #[test]
    fn numbering_stops_at_the_largest_signed_64_bit_generation() {
        // A line of 70 fathers: generation 62 is numbered 2^62, the ones
        // above it are ancestors without a number.
        let links: Vec<_> = (1..=70)
            .map(|n| link(n, n + 1, SpouseRole::Husband))
            .collect();
        let found = numbers(1, &links);
        assert_eq!(found[&id(63)], Some(1 << 62));
        assert_eq!(found[&id(64)], None);
        assert_eq!(found.len(), 71);
    }
}
