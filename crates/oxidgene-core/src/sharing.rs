//! Sharing a tree (docs/authorization.md §6): who the tree's visibility
//! makes a Visitor, and the access scopes that limit a share to a part of
//! the tree.
//!
//! - A [`TreeVisibility`] says who is a Visitor without a membership or a
//!   share link (§6.4).
//! - A [`ScopeDefinition`] names the persons a membership, an invitation, a
//!   share link or the visitor scope is limited to (§6.5): a root person and
//!   their ancestors, their descendants or both, with the spouses and the
//!   collateral relatives of the persons reached. [`ScopeDefinition::persons`]
//!   computes them from a tree's family links, the way an exclusion's
//!   persons are ([`crate::privacy::ExclusionScope`]).
//!
//! The types are shared by every build; only the web build stores and
//! applies them.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::privacy::Kinship;

/// The deepest a scope walks when it names no depth: the bound every
/// ancestry walk has, the schema not preventing a cycle (docs/data-model.md
/// §1, "Ancestry traversal").
pub const MAX_SCOPE_GENERATIONS: u32 = 64;
/// The shortest and the longest depth a scope may name, in generations: the
/// bounds of the API's bulk targets.
pub const MIN_SCOPE_DEPTH: u32 = 1;
pub const MAX_SCOPE_DEPTH: u32 = 30;

/// Who a tree's visibility makes a Visitor (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeVisibility {
    /// Nobody but the holders of a share link.
    #[default]
    Private,
    /// Every signed-in user of the instance.
    Site,
    /// Everyone on the web, signed in or not.
    Public,
}

impl TreeVisibility {
    /// The stored and serialized value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Site => "site",
            Self::Public => "public",
        }
    }

    /// The visibility a stored value names; anything unknown is `private`,
    /// the narrowest.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "site" => Self::Site,
            "public" => Self::Public,
            _ => Self::Private,
        }
    }

    /// Whether the visibility makes a viewer a Visitor: anyone for
    /// `public`, a signed-in user for `site`.
    #[must_use]
    pub fn admits(self, signed_in: bool) -> bool {
        match self {
            Self::Private => false,
            Self::Site => signed_in,
            Self::Public => true,
        }
    }
}

impl std::fmt::Display for TreeVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which side of its root a scope reaches (§6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// The root and their ancestors.
    AncestorsOf,
    /// The root and their descendants.
    DescendantsOf,
    /// Both, as one set: an hourglass.
    AncestorsAndDescendantsOf,
}

impl ScopeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AncestorsOf => "ancestors_of",
            Self::DescendantsOf => "descendants_of",
            Self::AncestorsAndDescendantsOf => "ancestors_and_descendants_of",
        }
    }
}

fn default_true() -> bool {
    true
}

/// The persons a share is limited to (§6.5), stored as the JSON
/// `definition` of an `access_scope` (docs/data-model.md §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDefinition {
    pub kind: ScopeKind,
    /// The root person.
    pub person_id: Uuid,
    /// The generations walked, 1 to 30; all of them, up to
    /// [`MAX_SCOPE_GENERATIONS`], when none.
    #[serde(default)]
    pub depth: Option<u32>,
    /// The root itself.
    #[serde(default = "default_true")]
    pub include_self: bool,
    /// The spouses of the persons the walk reached, the root's own
    /// excepted, as the API's bulk targets name them.
    #[serde(default)]
    pub include_spouses: bool,
    /// The siblings of the persons reached, and the siblings' spouses.
    #[serde(default)]
    pub collateral: bool,
}

impl ScopeDefinition {
    /// Why the definition cannot be stored, if it cannot: a depth out of
    /// 1 to 30.
    pub fn check(&self) -> Result<(), String> {
        match self.depth {
            Some(depth) if !(MIN_SCOPE_DEPTH..=MAX_SCOPE_DEPTH).contains(&depth) => Err(format!(
                "a scope's depth is between {MIN_SCOPE_DEPTH} and {MAX_SCOPE_DEPTH} generations"
            )),
            _ => Ok(()),
        }
    }

    /// The persons the scope reaches among the live persons `kin` links:
    /// none when its root is not one of `live`, as for a deleted root, whose
    /// scope is empty until it is restored.
    #[must_use]
    pub fn persons(&self, kin: &Kinship, live: &HashSet<Uuid>) -> HashSet<Uuid> {
        let mut reached = HashSet::new();
        if !live.contains(&self.person_id) {
            return reached;
        }
        let depth = self.depth.unwrap_or(MAX_SCOPE_GENERATIONS);
        if self.kind != ScopeKind::DescendantsOf {
            walk(self.person_id, depth, live, &mut reached, |person| {
                kin.parents_of(person)
            });
        }
        if self.kind != ScopeKind::AncestorsOf {
            walk(self.person_id, depth, live, &mut reached, |person| {
                kin.children_of(person)
            });
        }
        // The spouses of the persons the walk reached, as the API's bulk
        // targets name them: not the root's own.
        let spouses = if self.include_spouses {
            relatives(&reached, live, |p| kin.spouses_of(p))
        } else {
            HashSet::new()
        };
        if self.include_self {
            reached.insert(self.person_id);
        }
        let mut persons = reached.clone();
        persons.extend(spouses);
        if self.collateral {
            let siblings = relatives(&reached, live, |p| kin.siblings_of(p));
            persons.extend(relatives(&siblings, live, |p| kin.spouses_of(p)));
            persons.extend(siblings);
        }
        persons
    }
}

/// The persons outside `scope` who are a parent, a spouse or a child of a
/// person inside it: those a scoped viewer sees as *Not shared* stubs, so a
/// chart keeps its shape (§6.5, open decision 8). Anyone further out does
/// not appear.
#[must_use]
pub fn boundary(scope: &HashSet<Uuid>, kin: &Kinship) -> HashSet<Uuid> {
    scope
        .iter()
        .flat_map(|&person| {
            kin.parents_of(person)
                .iter()
                .chain(kin.spouses_of(person))
                .chain(kin.children_of(person))
                .copied()
        })
        .filter(|person| !scope.contains(person))
        .collect()
}

/// Walk from `root` along `step`, generation by generation, for at most
/// `depth` generations, adding every live person met to `reached`; the root
/// itself is not added.
fn walk<'a>(
    root: Uuid,
    depth: u32,
    live: &HashSet<Uuid>,
    reached: &mut HashSet<Uuid>,
    step: impl Fn(Uuid) -> &'a [Uuid],
) {
    let mut seen = HashSet::from([root]);
    let mut generation = vec![root];
    for _ in 0..depth {
        generation = generation
            .iter()
            .flat_map(|&person| step(person).iter().copied())
            .filter(|person| live.contains(person) && seen.insert(*person))
            .collect();
        if generation.is_empty() {
            break;
        }
        reached.extend(generation.iter().copied());
    }
}

/// The live relatives `of` names for each of `persons`.
fn relatives<'a>(
    persons: &HashSet<Uuid>,
    live: &HashSet<Uuid>,
    of: impl Fn(Uuid) -> &'a [Uuid],
) -> HashSet<Uuid> {
    persons
        .iter()
        .flat_map(|&person| of(person).iter().copied())
        .filter(|person| live.contains(person))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::FamilyMembers;

    /// Fictitious persons by number.
    fn p(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    /// Two grandparents (1, 2) with a child 3, who with spouse 4 has
    /// children 5 and 6; 6 marries 7, who brings a sibling 8 married to 9;
    /// 5 has a child 10. 3 has a sibling 11, married to 12.
    fn fixture() -> (Kinship, HashSet<Uuid>) {
        let families = [
            FamilyMembers {
                spouses: vec![p(1), p(2)],
                children: vec![p(3), p(11)],
            },
            FamilyMembers {
                spouses: vec![p(3), p(4)],
                children: vec![p(5), p(6)],
            },
            FamilyMembers {
                spouses: vec![p(6), p(7)],
                children: vec![],
            },
            FamilyMembers {
                spouses: vec![p(20), p(21)],
                children: vec![p(7), p(8)],
            },
            FamilyMembers {
                spouses: vec![p(8), p(9)],
                children: vec![],
            },
            FamilyMembers {
                spouses: vec![p(5)],
                children: vec![p(10)],
            },
            FamilyMembers {
                spouses: vec![p(11), p(12)],
                children: vec![],
            },
        ];
        let live = (1..=21).map(p).collect();
        (Kinship::of_families(&families), live)
    }

    fn scope(kind: ScopeKind, root: u128) -> ScopeDefinition {
        ScopeDefinition {
            kind,
            person_id: p(root),
            depth: None,
            include_self: true,
            include_spouses: false,
            collateral: false,
        }
    }

    fn ids(persons: &HashSet<Uuid>) -> Vec<u128> {
        let mut ids: Vec<u128> = persons.iter().map(|id| id.as_u128()).collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn each_kind_walks_its_side_of_the_root() {
        let (kin, live) = fixture();
        let up = scope(ScopeKind::AncestorsOf, 5).persons(&kin, &live);
        assert_eq!(ids(&up), [1, 2, 3, 4, 5]);
        let down = scope(ScopeKind::DescendantsOf, 3).persons(&kin, &live);
        assert_eq!(ids(&down), [3, 5, 6, 10]);
        let both = scope(ScopeKind::AncestorsAndDescendantsOf, 3).persons(&kin, &live);
        assert_eq!(ids(&both), [1, 2, 3, 5, 6, 10]);
    }

    #[test]
    fn depth_self_spouses_and_collateral_widen_or_narrow_the_set() {
        let (kin, live) = fixture();
        let mut definition = scope(ScopeKind::DescendantsOf, 3);
        definition.depth = Some(1);
        definition.include_self = false;
        assert_eq!(ids(&definition.persons(&kin, &live)), [5, 6]);
        definition.include_spouses = true;
        assert_eq!(ids(&definition.persons(&kin, &live)), [5, 6, 7]);
        definition.collateral = true;
        definition.include_spouses = false;
        // 5 and 6 are each other's siblings: 6's spouse 7 comes in as a
        // sibling's spouse, not 7's own sibling 8.
        assert_eq!(ids(&definition.persons(&kin, &live)), [5, 6, 7]);
        let mut up = scope(ScopeKind::AncestorsOf, 3);
        up.collateral = true;
        // 3's sibling 11 and that sibling's spouse 12.
        assert_eq!(ids(&up.persons(&kin, &live)), [1, 2, 3, 11, 12]);
    }

    #[test]
    fn a_deleted_root_reaches_nobody_and_deleted_persons_stop_the_walk() {
        let (kin, mut live) = fixture();
        live.remove(&p(6));
        let down = scope(ScopeKind::DescendantsOf, 3).persons(&kin, &live);
        assert_eq!(ids(&down), [3, 5, 10]);
        live.remove(&p(3));
        assert!(
            scope(ScopeKind::DescendantsOf, 3)
                .persons(&kin, &live)
                .is_empty()
        );
    }

    #[test]
    fn the_boundary_is_one_link_out_and_no_further() {
        let (kin, live) = fixture();
        let scope = scope(ScopeKind::DescendantsOf, 6).persons(&kin, &live);
        // 6 alone: their parents 3 and 4 and their spouse 7 are stubs; 7's
        // sibling 8 and the grandparents are not.
        assert_eq!(ids(&boundary(&scope, &kin)), [3, 4, 7]);
    }

    #[test]
    fn a_depth_out_of_range_is_refused_and_absent_fields_take_their_defaults() {
        let mut definition = scope(ScopeKind::AncestorsOf, 1);
        assert!(definition.check().is_ok());
        definition.depth = Some(0);
        assert!(definition.check().is_err());
        definition.depth = Some(31);
        assert!(definition.check().is_err());
        let parsed: ScopeDefinition = serde_json::from_str(&format!(
            r#"{{"kind":"descendants_of","person_id":"{}"}}"#,
            p(3)
        ))
        .unwrap();
        assert!(parsed.include_self && !parsed.include_spouses && !parsed.collateral);
        assert_eq!(TreeVisibility::parse("unknown"), TreeVisibility::Private);
        assert!(TreeVisibility::Public.admits(false));
        assert!(!TreeVisibility::Site.admits(false) && TreeVisibility::Site.admits(true));
    }
}
