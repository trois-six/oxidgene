//! The [`Redaction`] of a request's viewer on a tree (docs/privacy.md §5,
//! §7): the decisions of the viewer's class — built from the viewer's
//! access, the tree's privacy settings, the owner's exclusions and, for a
//! viewer without `read_private`, the stored inputs of the contemporary
//! rule — with pseudonyms of the request's own; kept in the
//! [`ViewerContext`] for the request's later reads.
//!
//! The owner's is built from the access alone, without a query: every
//! enforcement point then costs nothing for the desktop, the `none` mode and
//! the owner of a shared tree. A restricted viewer's class decisions are
//! kept by the tree cache between requests (docs/architecture.md §4.1),
//! used only while the tree's redaction generation has not moved since they
//! were read and the year is the request's.

use std::sync::Arc;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::ViewerContext;
use oxidgene_core::redaction::{Redaction, Viewing};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::tree_cache::{ClassRecords, ViewerClass};

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
        let class = ViewerClass {
            viewing,
            scope_id: access.scope_id,
            year: profiles.cache().current_year(),
        };
        // On the heap: the build reads several tables, and every resolver
        // awaits it, on small stacks too.
        let records = Box::pin(class_records(db, profiles, class, tree_id)).await?;
        // The hidden records, for the request's record reads: already set
        // only if another resolver of the request got here first, with the
        // same records.
        let _ = viewer
            .derived::<super::records::Slot>(tree_id)
            .set(Arc::clone(&records.hidden));
        // Pseudonyms of the request's own: the kept decisions carry none.
        Arc::new(Redaction::new(Arc::clone(&records.redaction)))
    };
    viewer.remember_redaction(tree_id, Arc::clone(&redaction));
    Ok(redaction)
}

/// What viewer class `class` sees of tree `tree_id`: kept by the tree
/// cache when the tree's redaction generation has not moved since it was
/// read, else built and kept (docs/architecture.md §4.1).
///
/// The generation is read first, one primary-key read, and stamps what is
/// built after it: a write committed meanwhile leaves the build newer than
/// its stamp, never older. When it cannot be read, the redaction is built
/// and not kept: nothing kept is ever used unchecked.
#[cfg(feature = "auth")]
async fn class_records(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    class: ViewerClass,
    tree_id: Uuid,
) -> Result<Arc<ClassRecords>, OxidGeneError> {
    use oxidgene_db::repo::RedactionRepo;

    let cache = profiles.cache();
    let generation = match RedactionRepo::generation(profiles.reader(), tree_id).await {
        Ok(Some(generation)) => generation,
        Ok(None) => {
            // The tree is gone: nothing of it is worth keeping.
            cache.forget_tree(tree_id);
            return build(db, profiles, class, tree_id).await.map(Arc::new);
        }
        Err(error) => {
            tracing::warn!(
                error.kind = crate::error_contract::error_kind(&error),
                "Redaction generation unreadable, redaction built without the cache"
            );
            return build(db, profiles, class, tree_id).await.map(Arc::new);
        }
    };
    if let Some(kept) = cache.kept_redaction(tree_id, class, generation) {
        return Ok(kept);
    }
    let gate = cache.redactions_gate(tree_id);
    let _building = gate.lock().await;
    // Built by the request this one waited for.
    if let Some(kept) = cache.kept_redaction(tree_id, class, generation) {
        return Ok(kept);
    }
    let records = Arc::new(build(db, profiles, class, tree_id).await?);
    cache.keep_redaction(tree_id, class, generation, Arc::clone(&records));
    Ok(records)
}

/// The decisions of viewer class `class` on tree `tree_id`, and the hidden
/// records they imply, read now. It receives the class and nothing else
/// about the viewer: every viewer of the class is shown the same, which is
/// what lets them share it.
#[cfg(feature = "auth")]
async fn build(
    db: &DatabaseConnection,
    profiles: &ProfileService,
    class: ViewerClass,
    tree_id: Uuid,
) -> Result<ClassRecords, OxidGeneError> {
    use std::collections::HashSet;

    use oxidgene_core::enums::Privacy;
    use oxidgene_core::redaction::{ClassRedaction, RedactionInputs, ScopeInputs};
    use oxidgene_db::repo::{AncestryRepo, TreeRepo};

    let ViewerClass {
        viewing,
        scope_id,
        year,
    } = class;
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
    let mut redaction = ClassRedaction::build(
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
        year,
    );
    let hidden = Box::pin(super::records::withhold_records(
        db,
        tree_id,
        &mut redaction,
    ))
    .await?;
    Ok(ClassRecords {
        redaction: Arc::new(redaction),
        hidden: Arc::new(hidden),
    })
}

/// Without the `auth` feature the viewer is always the local owner, whose
/// viewing is unrestricted: nothing to redact, read or keep.
#[cfg(not(feature = "auth"))]
async fn class_records(
    _db: &DatabaseConnection,
    _profiles: &ProfileService,
    _class: ViewerClass,
    _tree_id: Uuid,
) -> Result<Arc<ClassRecords>, OxidGeneError> {
    Ok(Arc::new(ClassRecords {
        redaction: Arc::new(oxidgene_core::redaction::ClassRedaction::unrestricted()),
        hidden: Arc::default(),
    }))
}
