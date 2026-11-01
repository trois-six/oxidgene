//! What the Tools page's computations read of the tree for a restricted
//! viewer (docs/privacy.md §7, "Tools"): the projections of the persons the
//! viewer's [`Redaction`] shows in full, each as the viewer sees it — the
//! relatives they may not see renamed or gone, a withheld family's events
//! left out — so that a rule never examines, counts or names a hidden
//! person.

use std::collections::HashSet;
use std::sync::Arc;

use oxidgene_core::projection::PersonProfile;
use oxidgene_core::redaction::Redaction;
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
