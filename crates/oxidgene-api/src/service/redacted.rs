//! What the Tools page's computations read of the tree for a restricted
//! viewer (docs/privacy.md §7, "Tools"): the projections of the persons the
//! viewer's [`Redaction`] shows in full, each as the viewer sees it — the
//! relatives they may not see renamed or gone, a withheld family's events
//! left out — so that a rule never examines, counts or names a hidden
//! person. Walks of the family links, too, go through the persons the
//! viewer sees only.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use oxidgene_core::projection::PersonProfile;
use oxidgene_core::redaction::Redaction;
use oxidgene_core::types::AncestryLink;
use oxidgene_db::repo::FamilyLink;
use uuid::Uuid;

use crate::tree_cache::TreeProfiles;

/// `profiles` as `redaction`'s viewer may compute over them: the tree
/// cache's own, shared and untouched, for the owner.
#[must_use]
pub fn shown_profiles(redaction: &Redaction, profiles: Arc<TreeProfiles>) -> Arc<TreeProfiles> {
    if redaction.is_unrestricted() {
        return profiles;
    }
    Arc::new(TreeProfiles::new(
        profiles
            .iter()
            .filter(|profile| redaction.shows(profile.person_id))
            .filter_map(|profile| redaction.profile(profile))
            .collect(),
    ))
}

/// The ids of `profiles`: the persons a computation may name.
#[must_use]
pub fn ids(profiles: &[PersonProfile]) -> HashSet<Uuid> {
    profiles.iter().map(|profile| profile.person_id).collect()
}

/// The ancestors (`up`) or the descendants of `root` within `max_depth`
/// generations, each at its shortest distance, as `redaction`'s viewer can
/// walk to them: through the persons who appear to them only, so a person
/// reached only through an absent one is not reached. Only the persons shown
/// in full are returned, by distance.
#[must_use]
pub fn seen_lineage(
    links: &[FamilyLink],
    redaction: &Redaction,
    root: Uuid,
    up: bool,
    max_depth: i32,
) -> Vec<AncestryLink> {
    let mut spouses: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut as_spouse: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut as_child: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for link in links
        .iter()
        .filter(|link| redaction.appears(link.person_id))
    {
        let (members, families) = if link.spouse_role.is_some() {
            (&mut spouses, &mut as_spouse)
        } else {
            (&mut children, &mut as_child)
        };
        members
            .entry(link.family_id)
            .or_default()
            .push(link.person_id);
        families
            .entry(link.person_id)
            .or_default()
            .push(link.family_id);
    }
    let (families_of, members_of) = if up {
        (&as_child, &spouses)
    } else {
        (&as_spouse, &children)
    };
    let mut depth_of: HashMap<Uuid, i32> = HashMap::from([(root, 0)]);
    let mut queue = VecDeque::from([root]);
    while let Some(person) = queue.pop_front() {
        let depth = depth_of[&person];
        if depth >= max_depth {
            continue;
        }
        for family in families_of.get(&person).into_iter().flatten() {
            for &next in members_of.get(family).into_iter().flatten() {
                if let std::collections::hash_map::Entry::Vacant(entry) = depth_of.entry(next) {
                    entry.insert(depth + 1);
                    queue.push_back(next);
                }
            }
        }
    }
    let mut lineage: Vec<AncestryLink> = depth_of
        .into_iter()
        .filter(|&(id, depth)| depth > 0 && redaction.shows(id))
        .map(|(person_id, depth)| AncestryLink { person_id, depth })
        .collect();
    lineage.sort_by_key(|link| (link.depth, link.person_id));
    lineage
}

#[cfg(test)]
mod tests {
    use oxidgene_core::enums::{SpouseRole, TreeDefaultPrivacy};
    use oxidgene_core::redaction::{RedactionInputs, Viewing};
    use oxidgene_core::types::PrivacySettings;

    use super::*;

    fn link(family_id: Uuid, person_id: Uuid, spouse: bool) -> FamilyLink {
        FamilyLink {
            family_id,
            person_id,
            spouse_role: spouse.then_some(SpouseRole::Partner),
            sort_order: 0,
        }
    }

    /// Grandparent → excluded parent → child, and the child's other parent
    /// with their own parent: the editor reaches the child's other side
    /// only.
    #[test]
    fn a_lineage_walks_through_no_absent_person() {
        let [grandparent, excluded, other, other_parent, child] = [(); 5].map(|()| Uuid::now_v7());
        let [upper, lower, other_side] = [(); 3].map(|()| Uuid::now_v7());
        let links = vec![
            link(upper, grandparent, true),
            link(upper, excluded, false),
            link(lower, excluded, true),
            link(lower, other, true),
            link(lower, child, false),
            link(other_side, other_parent, true),
            link(other_side, other, false),
        ];
        let excluded_set = HashSet::from([excluded]);
        let families = vec![(
            lower,
            oxidgene_core::enums::Privacy::Default,
            vec![excluded, other],
        )];
        let editor = Redaction::build(
            Viewing {
                read_private: true,
                read_confidential: true,
                read_tags: true,
                exclusions_apply: true,
            },
            TreeDefaultPrivacy::Public,
            &PrivacySettings::default(),
            &RedactionInputs {
                excluded: Some(&excluded_set),
                visibility: None,
                families: &families,
            },
            2026,
        );
        let ids = |lineage: Vec<AncestryLink>| -> Vec<(Uuid, i32)> {
            lineage
                .into_iter()
                .map(|l| (l.person_id, l.depth))
                .collect()
        };
        let mut expected = vec![(other, 1), (other_parent, 2)];
        expected.sort_by_key(|&(id, depth)| (depth, id));
        assert_eq!(ids(seen_lineage(&links, &editor, child, true, 5)), expected);
        assert_eq!(
            ids(seen_lineage(&links, &editor, child, true, 1)),
            vec![(other, 1)]
        );
        assert!(seen_lineage(&links, &editor, grandparent, false, 5).is_empty());

        let owner = Redaction::unrestricted();
        let all = ids(seen_lineage(&links, &owner, child, true, 5));
        assert_eq!(all.len(), 4, "{all:?}");
    }
}
