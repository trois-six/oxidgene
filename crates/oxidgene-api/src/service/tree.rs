//! Tree writes and the tree list: the same steps whether REST or GraphQL
//! asked.
//!
//! A tree's settings name persons of their own — the SOSA root, the user's
//! own record — and those must belong to the tree: every write checks them in
//! the transaction that stores them.

use std::collections::HashMap;

use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Principal, TreeAccess, TreeRole, ViewerAccess, ViewerContext};
use oxidgene_core::enums::{HiddenDisplay, TreeDefaultPrivacy};
use oxidgene_core::history::AuditEntity;
use oxidgene_core::types::{AnomalyThresholds, Connection, Tree, check_contemporary_years};
use oxidgene_db::repo::{
    BackgroundJobRepo, MembershipRepo, PaginationParams, TreeChanges, TreeRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::ProfileService;
use crate::service::background_job::{active_job_cancelling, parse_off_thread};
use crate::service::gedcom;
use crate::service::history::{self, Change};
use crate::service::patch::double_option;
use crate::service::purge::PurgeQueue;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A tree as the tree list shows it, with the import running into it.
///
/// The import fields come from the job queue, not the tree: they are set
/// while a job is queued or running and vanish once it completes, fails or
/// is cancelled.
#[derive(Debug, Clone, Serialize)]
pub struct TreeListItem {
    #[serde(flatten)]
    pub tree: Tree,
    /// Whether an import is queued or running into the tree.
    pub import_in_progress: bool,
    /// That import's job, while it is.
    pub import_job_id: Option<Uuid>,
    /// Whether that import was asked to stop and is undoing what it wrote.
    pub import_cancelling: bool,
    /// What the viewer may do with the tree (docs/authorization.md §4.1).
    pub viewer_access: ViewerAccess,
}

/// A tree as a read returns it: with what the viewer may do with it.
#[derive(Debug, Clone, Serialize)]
pub struct TreeView {
    #[serde(flatten)]
    pub tree: Tree,
    pub viewer_access: ViewerAccess,
}

/// The import queued or running into a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveImport {
    pub job_id: Uuid,
    /// It was asked to stop and is undoing what it wrote.
    pub cancelling: bool,
}

/// The imports queued or running, by tree. A job cancelled in this
/// process's memory, its cancellation not written yet, is not.
async fn active_imports(
    db: &DatabaseConnection,
) -> Result<HashMap<Uuid, ActiveImport>, OxidGeneError> {
    Ok(BackgroundJobRepo::active_imports(db)
        .await?
        .into_iter()
        .filter_map(|job| {
            let cancelling = active_job_cancelling(job.id, job.cancel_requested)?;
            Some((
                job.tree_id,
                ActiveImport {
                    job_id: job.id,
                    cancelling,
                },
            ))
        })
        .collect())
}

/// A tree to create.
#[derive(Debug, Deserialize)]
pub struct NewTree {
    pub name: String,
    pub description: Option<String>,
}

/// The settings a tree update changes: `None` keeps a field, and `Some(None)`
/// clears an optional one.
#[derive(Debug, Default, Deserialize)]
pub struct TreePatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// The person SOSA numbers count from.
    #[serde(default, deserialize_with = "double_option")]
    pub sosa_root_person_id: Option<Option<Uuid>>,
    /// The person identifying the current user.
    #[serde(default, deserialize_with = "double_option")]
    pub self_person_id: Option<Option<Uuid>>,
    /// What `privacy: "default"` resolves to for everything in the tree.
    pub default_privacy: Option<TreeDefaultPrivacy>,
    /// Whether contemporary persons are hidden (`docs/privacy.md` §9).
    pub contemporary_rule: Option<bool>,
    /// The contemporary rule's threshold, 50 to 150 years.
    pub contemporary_years: Option<i32>,
    /// How a hidden person appears.
    pub hidden_display: Option<HiddenDisplay>,
    /// Whether a hidden person's placeholder can be navigated through.
    pub hidden_navigation: Option<bool>,
    /// Whether visitors see the photographs of contemporary persons.
    pub contemporary_photos: Option<bool>,
    /// Whether guests may export the tree.
    pub guests_may_export: Option<bool>,
    /// Who is a Visitor (`docs/authorization.md` §6.4). Web build only.
    #[cfg(feature = "auth")]
    pub visibility: Option<oxidgene_core::sharing::TreeVisibility>,
    /// The scope Visitors are limited to; `null` for the whole tree. Web
    /// build only.
    #[cfg(feature = "auth")]
    #[serde(default, deserialize_with = "double_option")]
    pub visitor_scope: Option<Option<oxidgene_core::sharing::ScopeDefinition>>,
    /// Who the tree's GEDCOM exports say they are from.
    #[serde(default, deserialize_with = "double_option")]
    pub submitter_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub submitter_email: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub submitter_address: Option<Option<String>>,
    /// The thresholds of the anomaly rules, all at once: a threshold left
    /// out takes its default.
    pub anomaly_thresholds: Option<AnomalyThresholds>,
}

/// The viewer's trees, a page at a time, each with the import running into
/// it: every tree for the local owner; a user's memberships otherwise, and
/// the trees a share link their session holds opens, as a Visitor
/// (docs/authorization.md §6.4). A `public` or `site` tree they are no
/// member of is reached by its address, never listed: there is no
/// directory of trees.
pub async fn list_trees(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    profiles: &ProfileService,
    params: &PaginationParams,
) -> Result<Connection<TreeListItem>, OxidGeneError> {
    let now = chrono::Utc::now();
    let (trees, roles) = match viewer.principal() {
        Principal::Local => (TreeRepo::list(db, params).await?, None),
        Principal::User(identity) => {
            let opened = crate::authz::link_trees(db, viewer).await?;
            let trees =
                TreeRepo::list_of_viewer(db, identity.user_id, &opened, now, params).await?;
            let ids: Vec<Uuid> = trees.edges.iter().map(|edge| edge.node.id).collect();
            let roles = MembershipRepo::live_roles(db, identity.user_id, &ids, now).await?;
            (trees, Some(roles))
        }
        Principal::Anonymous => return Err(OxidGeneError::Unauthenticated),
    };
    let mut accesses = std::collections::HashMap::new();
    for tree in trees.edges.iter().map(|edge| &edge.node) {
        let access = match &roles {
            None => TreeAccess::of_role(TreeRole::Owner),
            Some(roles) => match roles.get(&tree.id) {
                Some(&(role, scope)) => TreeAccess::of_role(role).scoped(scope),
                // A tree a share link opens: the link's own access.
                None => crate::authz::tree_access(db, viewer, tree.id).await?,
            },
        };
        accesses.insert(tree.id, access);
    }
    let active = active_imports(db).await?;
    let mut trees = trees;
    // The roots a member may not see are not named (docs/privacy.md §6.4).
    for edge in &mut trees.edges {
        let restricted = roles.as_ref().is_some_and(|roles| {
            roles.get(&edge.node.id).map(|(role, _)| role) != Some(&TreeRole::Owner)
        });
        if restricted {
            let redaction =
                crate::authz::redaction::redaction(db, profiles, viewer, edge.node.id).await?;
            crate::profile::redact_tree(&redaction, &mut edge.node);
        }
    }
    Ok(trees.map(|tree| {
        let import = active.get(&tree.id).copied();
        let access = accesses
            .get(&tree.id)
            .copied()
            .unwrap_or_else(|| TreeAccess::of_role(TreeRole::Visitor));
        TreeListItem {
            tree,
            import_in_progress: import.is_some(),
            import_job_id: import.map(|import| import.job_id),
            import_cancelling: import.is_some_and(|import| import.cancelling),
            viewer_access: access.viewer_access(),
        }
    }))
}

/// Tree `tree_id` as the viewer reads it, with what they may do with it.
/// The viewer's access was resolved when the request was authorized.
pub async fn get_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    profiles: &ProfileService,
    tree_id: Uuid,
) -> Result<TreeView, OxidGeneError> {
    let access = crate::authz::tree_access(db, viewer, tree_id).await?;
    let redaction = crate::authz::redaction::redaction(db, profiles, viewer, tree_id).await?;
    let mut tree = TreeRepo::get(db, tree_id).await?;
    crate::profile::redact_tree(&redaction, &mut tree);
    Ok(TreeView {
        tree,
        viewer_access: access.viewer_access(),
    })
}

/// Make a signed-in viewer the owner of tree `tree_id` they just created;
/// a tree the local owner creates has none (docs/authorization.md §2).
async fn make_owner(
    viewer: &ViewerContext,
    db: &impl sea_orm::ConnectionTrait,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    if let Some(identity) = viewer.user() {
        MembershipRepo::create(
            db,
            tree_id,
            identity.user_id,
            TreeRole::Owner,
            Some(identity.user_id),
        )
        .await?;
    }
    Ok(())
}

/// The import queued or running into tree `tree_id`, if any.
pub async fn active_import(
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<Option<ActiveImport>, OxidGeneError> {
    Ok(active_imports(db).await?.remove(&tree_id))
}

/// Create a tree. A blank name is refused.
pub async fn create_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    new: NewTree,
) -> Result<Tree, OxidGeneError> {
    require_name(&new.name)?;
    let id = Uuid::now_v7();
    let txn = begin_tx(db).await?;
    let pending = Change::create(viewer, id, AuditEntity::Tree, id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    let tree = TreeRepo::create(&txn, id, new.name, new.description).await?;
    make_owner(viewer, &txn, id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    Ok(tree)
}

/// Update tree `tree_id`. A blank name is refused, so are anomaly thresholds
/// and a contemporary threshold out of their ranges, and a SOSA root or own record from another tree is
/// not found.
pub async fn update_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    patch: TreePatch,
) -> Result<Tree, OxidGeneError> {
    if let Some(name) = &patch.name {
        require_name(name)?;
    }
    if let Some(thresholds) = &patch.anomaly_thresholds {
        thresholds.check().map_err(OxidGeneError::Validation)?;
    }
    if let Some(years) = patch.contemporary_years {
        check_contemporary_years(years).map_err(OxidGeneError::Validation)?;
    }
    let txn = begin_tx(db).await?;
    for person_id in [patch.sosa_root_person_id, patch.self_person_id]
        .into_iter()
        .flatten()
        .flatten()
    {
        require_tree_resource(&txn, tree_id, TreeResource::Person, person_id).await?;
    }
    #[cfg(feature = "auth")]
    let sharing = (patch.visibility, patch.visitor_scope);
    let pending = Change::update(viewer, tree_id, AuditEntity::Tree, tree_id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    let tree = TreeRepo::update(
        &txn,
        tree_id,
        TreeChanges {
            name: patch.name,
            description: patch.description,
            sosa_root_person_id: patch.sosa_root_person_id,
            self_person_id: patch.self_person_id,
            default_privacy: patch.default_privacy,
            contemporary_rule: patch.contemporary_rule,
            contemporary_years: patch.contemporary_years,
            hidden_display: patch.hidden_display,
            hidden_navigation: patch.hidden_navigation,
            contemporary_photos: patch.contemporary_photos,
            guests_may_export: patch.guests_may_export,
            submitter_name: patch.submitter_name.map(blank_to_none),
            submitter_email: patch.submitter_email.map(blank_to_none),
            submitter_address: patch.submitter_address.map(blank_to_none),
            anomaly_thresholds: patch.anomaly_thresholds,
            ..TreeChanges::default()
        },
    )
    .await?;
    pending.record(&txn).await?;
    #[cfg(feature = "auth")]
    let tree = update_sharing(viewer, &txn, tree_id, sharing)
        .await?
        .unwrap_or(tree);
    commit_tx(txn).await?;
    Ok(tree)
}

/// Apply what a tree update changes of who the tree is visible to
/// (docs/authorization.md §6.4), in `txn`: the visibility and the visitor
/// scope, the replaced scope deleted once nothing holds it, the change
/// recorded under `access`. The tree as it is now, or `None` when the
/// update changes neither. Refused for the `none` mode's local owner, who has
/// nobody to share with (docs/authentication.md §2).
#[cfg(feature = "auth")]
async fn update_sharing(
    viewer: &ViewerContext,
    txn: &oxidgene_db::sea_orm::DatabaseTransaction,
    tree_id: Uuid,
    (visibility, visitor_scope): (
        Option<oxidgene_core::sharing::TreeVisibility>,
        Option<Option<oxidgene_core::sharing::ScopeDefinition>>,
    ),
) -> Result<Option<Tree>, OxidGeneError> {
    use crate::service::access_scope;
    if visibility.is_none() && visitor_scope.is_none() {
        return Ok(None);
    }
    if viewer.is_local() {
        return Err(OxidGeneError::Validation(
            "a tree's visibility needs accounts".to_string(),
        ));
    }
    let former = TreeRepo::get(txn, tree_id).await?.visitor_scope_id;
    let scope = access_scope::replacement(txn, tree_id, visitor_scope.as_ref()).await?;
    let tree = TreeRepo::update(
        txn,
        tree_id,
        TreeChanges {
            visibility,
            visitor_scope_id: scope,
            ..TreeChanges::default()
        },
    )
    .await?;
    access_scope::drop_replaced(txn, scope, former).await?;
    Change::update(viewer, tree_id, AuditEntity::Visibility, tree_id)
        .about_tree()
        .record_unversioned(txn)
        .await?;
    Ok(Some(tree))
}

/// Delete tree `tree_id`: flag it, and leave its rows to the purge worker.
///
/// Removing them here took seconds on a tree of any size — SQLite walks the
/// `ON DELETE CASCADE` graph one row at a time — so the request only flags
/// the tree and the worker does the rest. The purge is queued once the flag
/// is committed, so it can never outrun it.
pub async fn delete_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    purge: &PurgeQueue,
    tree_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    let pending = Change::delete(viewer, tree_id, AuditEntity::Tree, tree_id)
        .tree_settings()
        .prepare(&txn)
        .await?;
    TreeRepo::soft_delete(&txn, tree_id).await?;
    pending.record(&txn).await?;
    commit_tx(txn).await?;
    purge.enqueue(tree_id);
    Ok(())
}

/// Copy tree `source_tree_id` into a new tree named `name`, through a
/// lossless GEDCOM round trip.
///
/// The copy never enables the export's compatibility options: those trade
/// fidelity for third-party importers, which a copy inside OxidGene has no
/// use for. Both trees record the duplication in their audit log.
///
/// The new tree, its rows, its projections and both audit entries are
/// written in one transaction: a copy that fails at any step leaves no
/// tree behind. The file is parsed before it opens, off the async workers.
pub async fn duplicate_tree(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    profiles: &ProfileService,
    source_tree_id: Uuid,
    name: String,
) -> Result<Tree, OxidGeneError> {
    require_name(&name)?;
    let source_name = TreeRepo::get(db, source_tree_id).await?.name;
    let export = gedcom::load_and_export(
        db,
        source_tree_id,
        gedcom::ExportChoices::default(),
        None,
        false,
        gedcom::ExportPrivacy::default(),
    )
    .await?;
    let new_tree_id = Uuid::now_v7();
    let text = export.gedcom;
    let parsed = parse_off_thread("gedcom", move || {
        Ok(oxidgene_gedcom::import::import_gedcom(&text, new_tree_id))
    })
    .await?;

    let txn = begin_tx(db).await?;
    let tree = TreeRepo::create(&txn, new_tree_id, name, None).await?;
    make_owner(viewer, &txn, new_tree_id).await?;
    let summary = gedcom::persist_import_result_in(&txn, parsed, &mut |_| Ok(())).await?;
    profiles.rebuild_tree_full(&txn, new_tree_id).await?;
    history::record_import(
        viewer,
        &txn,
        new_tree_id,
        history::DUPLICATE_FORMAT,
        Some(source_name),
        summary.persons_count,
    )
    .await?;
    history::record_export(
        viewer,
        &txn,
        source_tree_id,
        history::DUPLICATE_FORMAT,
        Some(tree.name),
        None,
        None,
    )
    .await?;
    // As the import left it: it fills the settings the file carries, such
    // as the submitter.
    let tree = TreeRepo::get(&txn, new_tree_id).await?;
    commit_tx(txn).await?;
    Ok(tree)
}

fn require_name(name: &str) -> Result<(), OxidGeneError> {
    if name.trim().is_empty() {
        return Err(OxidGeneError::Validation(
            "name must not be empty".to_string(),
        ));
    }
    Ok(())
}

/// `text` trimmed, unless it is blank.
fn blank_to_none(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}
