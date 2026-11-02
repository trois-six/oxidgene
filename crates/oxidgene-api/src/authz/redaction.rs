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
        // On the heap: the build reads several tables, and every resolver
        // awaits it, on small stacks too.
        let mut redaction =
            Box::pin(restricted(db, profiles, viewing, access.scope_id, tree_id)).await?;
        Box::pin(super::records::withhold_media(
            db,
            viewer,
            tree_id,
            &mut redaction,
        ))
        .await?;
        Arc::new(redaction)
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
    scope_id: Option<Uuid>,
    tree_id: Uuid,
) -> Result<Redaction, OxidGeneError> {
    use std::collections::HashSet;

    use chrono::Datelike;
    use oxidgene_core::enums::Privacy;
    use oxidgene_core::redaction::{RedactionInputs, ScopeInputs};
    use oxidgene_db::repo::{AncestryRepo, TreeRepo};

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
    // The persons a scoped viewer sees, the boundary they see as stubs and
    // the tree's live persons, outside of which nobody is (Authorization
    // §6.5); the scope is rebuilt first on the writer when it is stale.
    let scope = match scope_id {
        Some(scope_id) => Some((
            scope_id,
            crate::service::access_scope::scoped_tree(profiles.writer(), tree_id, scope_id).await?,
        )),
        None => None,
    };
    // The families whose spouses may be hidden: every family of a tree
    // where anybody is, with its privacy and live spouses, in one
    // statement over the tree. Selecting them by the identifiers of the
    // tree's families or of the excluded persons bound thousands of values
    // and cost several times as much on a tree of 10,000 persons.
    let families: Vec<(Uuid, Privacy, Vec<Uuid>)> =
        if visibility.is_some() || scope.is_some() || !excluded.is_empty() {
            AncestryRepo::family_spouses(db, tree_id).await?
        } else {
            Vec::new()
        };
    Ok(Redaction::build(
        viewing,
        tree.default_privacy,
        &tree.privacy,
        &RedactionInputs {
            excluded: Some(&excluded),
            visibility: visibility.as_ref(),
            families: &families,
            scope: scope.as_ref().map(|(id, scope)| ScopeInputs {
                id: *id,
                live: &scope.live,
                inside: &scope.inside,
                boundary: &scope.boundary,
            }),
        },
        chrono::Utc::now().year(),
    ))
}

/// Without the `auth` feature the viewer is always the local owner, whose
/// viewing is unrestricted: nothing to redact.
#[cfg(not(feature = "auth"))]
async fn restricted(
    _db: &DatabaseConnection,
    _profiles: &ProfileService,
    _viewing: Viewing,
    _scope_id: Option<Uuid>,
    _tree_id: Uuid,
) -> Result<Redaction, OxidGeneError> {
    Ok(Redaction::unrestricted())
}
