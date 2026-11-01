//! The [`Redaction`] of a request's viewer on a tree (docs/privacy.md §5,
//! §7): built once per request and tree from the viewer's access, the
//! tree's privacy settings, the owner's exclusions and, for a viewer
//! without `read_private`, the stored inputs of the contemporary rule; kept
//! in the [`ViewerContext`] for the request's later reads.
//!
//! The owner's is built from the access alone, without a query: every
//! enforcement point then costs nothing for the desktop, the `none` mode and
//! the owner of a shared tree. So does an Editor's or a Guest's on a tree
//! without exclusions, but for a Guest's withheld confidential notes.

use std::sync::Arc;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::redaction::{Redaction, Viewing};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::profile::ProfileService;

/// What `viewer` sees of tree `tree_id`.
///
/// # Errors
///
/// What resolving the viewer's access to the tree answers (`404` for a tree
/// they cannot see), and database failures.
pub async fn redaction(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    viewer: &ViewerContext,
    tree_id: Uuid,
) -> Result<Arc<Redaction>, OxidGeneError> {
    if let Some(redaction) = viewer.cached_redaction(tree_id) {
        return Ok(redaction);
    }
    let access = super::tree_access(db, viewer, tree_id).await?;
    let viewing = Viewing::of(&access);
    let redaction = if viewing.is_unrestricted() {
        Arc::new(Redaction::unrestricted())
    } else {
        Arc::new(restricted(db, profiles, viewing, tree_id).await?)
    };
    viewer.remember_redaction(tree_id, Arc::clone(&redaction));
    Ok(redaction)
}

/// The redaction of a viewer who does not see everything: only the web
/// build has one.
#[cfg(feature = "auth")]
async fn restricted(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    viewing: Viewing,
    tree_id: Uuid,
) -> Result<Redaction, OxidGeneError> {
    use std::collections::HashSet;

    use chrono::Datelike;
    use oxidgene_core::enums::Privacy;
    use oxidgene_core::redaction::RedactionInputs;
    use oxidgene_db::repo::{FamilyRepo, MembershipsOf, PersonVisibilityRepo, TreeRepo};

    let tree = TreeRepo::get(db, tree_id).await?;
    let excluded = if viewing.exclusions_apply {
        crate::service::exclusion::excluded_persons(profiles.writer(), tree_id).await?
    } else {
        HashSet::new()
    };
    let visibility = if viewing.read_private {
        None
    } else {
        Some(profiles.tree_visibility(tree_id).await?)
    };
    // The families whose spouses may be hidden: those of the excluded
    // persons, or every family for a viewer the privacy rules redact.
    let families: Vec<(Uuid, Privacy, Vec<Uuid>)> = if visibility.is_some() {
        let all = FamilyRepo::list_all(db, tree_id).await?;
        let ids: Vec<Uuid> = all.iter().map(|f| f.id).collect();
        let spouses = spouses_of(db, &ids).await?;
        all.into_iter()
            .map(|f| {
                let members = spouses.get(&f.id).cloned().unwrap_or_default();
                (f.id, f.privacy, members)
            })
            .collect()
    } else if excluded.is_empty() {
        Vec::new()
    } else {
        let ids: Vec<Uuid> = excluded.iter().copied().collect();
        let own = PersonVisibilityRepo::memberships(db, &ids, MembershipsOf::Persons).await?;
        let family_ids: Vec<Uuid> = own
            .iter()
            .filter(|m| m.spouse)
            .map(|m| m.family_id)
            .collect();
        spouses_of(db, &family_ids)
            .await?
            .into_iter()
            .map(|(id, spouses)| (id, Privacy::Default, spouses))
            .collect()
    };
    Ok(Redaction::build(
        viewing,
        tree.default_privacy,
        &tree.privacy,
        &RedactionInputs {
            excluded: Some(&excluded),
            visibility: visibility.as_ref(),
            families: &families,
        },
        chrono::Utc::now().year(),
    ))
}

/// The live spouses of each of `families`.
#[cfg(feature = "auth")]
async fn spouses_of(
    db: &DatabaseConnection,
    families: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, Vec<Uuid>>, OxidGeneError> {
    use oxidgene_db::repo::{MembershipsOf, PersonVisibilityRepo};
    let mut out: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    for member in PersonVisibilityRepo::memberships(db, families, MembershipsOf::Families).await? {
        if member.spouse {
            out.entry(member.family_id)
                .or_default()
                .push(member.person_id);
        }
    }
    Ok(out)
}

/// Without the `auth` feature the viewer is always the local owner, whose
/// viewing is unrestricted: nothing to redact.
#[cfg(not(feature = "auth"))]
async fn restricted(
    _db: &DatabaseConnection,
    _profiles: &ProfileService,
    _viewing: Viewing,
    _tree_id: Uuid,
) -> Result<Redaction, OxidGeneError> {
    Ok(Redaction::unrestricted())
}
