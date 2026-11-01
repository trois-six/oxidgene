//! Memberships and ownership (docs/authorization.md §5): who holds which
//! role on a tree, leaving it, handing it over, and the administrator's
//! view of every tree with its owner. Web build only (`auth` feature): the
//! desktop and the `none` mode have one local owner of every tree and no
//! accounts, so these operations answer `404` there, as accounts do.
//!
//! The operation's requirement is checked before the service runs
//! (`crate::authz`); a member's operations then narrow it: a member reads
//! their own row and changes their own "Who am I?", the owner manages the
//! others. Every change is audited under the tree's `access` category and
//! stores no version (§9.1).

use chrono::{DateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Identity, Permission, Principal, TreeRole, ViewerContext};
use oxidgene_core::history::{AuditAction, AuditEntity};
use oxidgene_core::types::{Connection, Edge, PageInfo};
use oxidgene_db::repo::{
    AppUserRepo, MembershipChanges, MembershipRepo, PaginationParams, PersonRepo, TreeMembership,
    TreeRepo, UserStatus,
};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use std::collections::HashMap;

use oxidgene_core::sharing::ScopeDefinition;

use crate::authz;
use crate::service::access_scope::{self, ScopeView};
use crate::service::account::require_admin;
use crate::service::history::Change;
use crate::service::notification::{About, Mailing, Notices, NotificationKind};
use crate::service::patch::double_option;
use crate::service::scope::{TreeResource, begin_tx, commit_tx, require_tree_resource};

/// A member of a tree, as the members list shows them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemberView {
    pub user_id: Uuid,
    /// The member's name; `None` for one without, shown by the generated
    /// avatar's initials.
    pub display_name: Option<String>,
    pub role: TreeRole,
    /// The part of the tree the member is limited to; none for the whole
    /// tree (§6.5).
    pub scope: Option<ScopeView>,
    /// The member's own "Who am I?" person.
    pub self_person_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What a member update changes: `None` keeps a field, `Some(None)` clears
/// an optional one. The role and the expiry are the owner's to change; the
/// "Who am I?" person is the member's own.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemberPatch {
    pub role: Option<TreeRole>,
    /// The part of the tree the member is limited to; `null` for the
    /// whole tree.
    #[serde(default, deserialize_with = "double_option")]
    pub scope: Option<Option<ScopeDefinition>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub self_person_id: Option<Option<Uuid>>,
}

/// A tree as an administrator sees it: metadata only, never its content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdminTree {
    pub id: Uuid,
    pub name: String,
    /// `None` for an ownerless tree, created in the `none` mode.
    pub owner_id: Option<Uuid>,
    pub owner_name: Option<String>,
    /// The number of persons: the tree's size.
    pub person_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The signed-in member `viewer` is. The `none` mode's local owner has no
/// account: memberships are absent there, as on the desktop (`404`).
fn member_identity(viewer: &ViewerContext) -> Result<&Identity, OxidGeneError> {
    match viewer.principal() {
        Principal::User(identity) => Ok(identity),
        Principal::Anonymous => Err(OxidGeneError::Unauthenticated),
        Principal::Local => Err(OxidGeneError::NotFound {
            entity: "Membership",
            id: Uuid::nil(),
        }),
    }
}

fn membership_not_found(user_id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "Membership",
        id: user_id,
    }
}

/// The members of tree `tree_id`: all of them for the owner, the viewer's
/// own row for anyone else.
pub async fn list_members(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<MemberView>, OxidGeneError> {
    let identity = member_identity(viewer)?;
    let access = authz::tree_access(db, viewer, tree_id).await?;
    let members = if access.allows(Permission::MembersManage) {
        MembershipRepo::list(db, tree_id, params).await?
    } else {
        let own = MembershipRepo::get(db, tree_id, identity.user_id).await?;
        let edges = own
            .into_iter()
            .map(|membership| Edge {
                cursor: oxidgene_db::repo::encode_cursor(&membership.id),
                node: membership,
            })
            .collect::<Vec<_>>();
        Connection {
            total_count: edges.len() as i64,
            edges,
            page_info: PageInfo {
                has_next_page: false,
                end_cursor: None,
            },
        }
    };
    let user_ids: Vec<Uuid> = members.edges.iter().map(|e| e.node.user_id).collect();
    let names = AppUserRepo::shown_names(db, &user_ids).await?;
    let scope_ids: Vec<Uuid> = members
        .edges
        .iter()
        .filter_map(|e| e.node.scope_id)
        .collect();
    let scopes = access_scope::views(db, &scope_ids).await?;
    Ok(members.map(|membership| member_view(membership, &names, &scopes)))
}

/// One member as the members list shows them, from the names and the
/// scopes read for the page.
pub(crate) fn member_view(
    membership: TreeMembership,
    names: &HashMap<Uuid, Option<String>>,
    scopes: &HashMap<Uuid, ScopeView>,
) -> MemberView {
    MemberView {
        display_name: names.get(&membership.user_id).cloned().flatten(),
        scope: membership.scope_id.and_then(|id| scopes.get(&id).cloned()),
        user_id: membership.user_id,
        role: membership.role,
        self_person_id: membership.self_person_id,
        expires_at: membership.expires_at,
        created_at: membership.created_at,
        updated_at: membership.updated_at,
    }
}

/// Change the membership of `user_id` on tree `tree_id`: the owner changes
/// a member's role, between Editor and Guest, and expiry; a member sets
/// their own "Who am I?" person. A change of the role, scope or expiry
/// notifies the member (`membership_changed`).
pub async fn update_member(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    tree_id: Uuid,
    user_id: Uuid,
    patch: MemberPatch,
) -> Result<MemberView, OxidGeneError> {
    let identity = member_identity(viewer)?;
    let access = authz::tree_access(db, viewer, tree_id).await?;
    let manages = access.allows(Permission::MembersManage);
    let current = MembershipRepo::get(db, tree_id, user_id)
        .await?
        .filter(|membership| manages || membership.user_id == identity.user_id)
        .ok_or_else(|| membership_not_found(user_id))?;
    check_patch(&patch, &current, manages, user_id == identity.user_id)?;
    let txn = begin_tx(db).await?;
    let scope_id = prepare_patch(&txn, tree_id, &patch).await?;
    let updated = MembershipRepo::update(
        &txn,
        current.id,
        MembershipChanges {
            role: patch.role,
            scope_id,
            self_person_id: patch.self_person_id,
            expires_at: patch.expires_at,
        },
    )
    .await?;
    access_scope::drop_replaced(&txn, scope_id, current.scope_id).await?;
    record(&txn, viewer, tree_id, AuditAction::Update, current.id).await?;
    // The owner changing a member's access tells them; a member's own
    // "Who am I?" tells nobody.
    let note = (changes_access(&patch) && user_id != identity.user_id).then(|| {
        (
            NotificationKind::MembershipChanged,
            user_id,
            About::tree(tree_id, viewer),
        )
    });
    let notices = Notices::write(&txn, mailing, note).await?;
    commit_tx(txn).await?;
    notices.publish();
    viewer.forget(tree_id);
    one_view(db, updated).await
}

/// `membership` as the members list shows it.
async fn one_view(
    db: &DatabaseConnection,
    membership: TreeMembership,
) -> Result<MemberView, OxidGeneError> {
    let user_id = membership.user_id;
    crate::service::sharing::member_views(db, vec![membership])
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| membership_not_found(user_id))
}

/// Check the "Who am I?" person `patch` names, and create the scope it
/// asks for, in `txn`: the scope change, as [`access_scope::replacement`]
/// answers it.
async fn prepare_patch(
    txn: &impl ConnectionTrait,
    tree_id: Uuid,
    patch: &MemberPatch,
) -> Result<Option<Option<Uuid>>, OxidGeneError> {
    if let Some(Some(person_id)) = patch.self_person_id {
        require_tree_resource(txn, tree_id, TreeResource::Person, person_id).await?;
    }
    access_scope::replacement(txn, tree_id, patch.scope.as_ref()).await
}

/// Whether `patch` changes what the member may see: the role, the scope or
/// the expiry, the owner's to change.
fn changes_access(patch: &MemberPatch) -> bool {
    patch.role.is_some() || patch.expires_at.is_some() || patch.scope.is_some()
}

/// Refuse what `patch` may not change on `current`: the role and expiry
/// but by the owner (`manages`), and never the owner's own, which changes
/// by transfer; the "Who am I?" person but by the member themselves
/// (`own`).
fn check_patch(
    patch: &MemberPatch,
    current: &TreeMembership,
    manages: bool,
    own: bool,
) -> Result<(), OxidGeneError> {
    crate::service::sharing::future(patch.expires_at.flatten())?;
    let changes_role = changes_access(patch);
    if (changes_role && !manages) || (patch.self_person_id.is_some() && !own) {
        return Err(OxidGeneError::Forbidden);
    }
    if changes_role && current.role == TreeRole::Owner {
        return Err(OxidGeneError::Validation(
            "the owner's role changes by transferring the tree".into(),
        ));
    }
    if matches!(patch.role, Some(TreeRole::Owner | TreeRole::Visitor)) {
        return Err(OxidGeneError::Validation(
            "a member is an editor or a guest".into(),
        ));
    }
    Ok(())
}

/// End the membership of `user_id` on tree `tree_id`: the owner removes a
/// member, a member leaves. The owner cannot leave: they transfer the tree
/// or delete it (`409`). The member removed is notified
/// (`membership_ended`); the owner of a member who leaves too
/// (`member_left`).
pub async fn remove_member(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    tree_id: Uuid,
    user_id: Uuid,
) -> Result<(), OxidGeneError> {
    let identity = member_identity(viewer)?;
    let access = authz::tree_access(db, viewer, tree_id).await?;
    let leaving = user_id == identity.user_id;
    if leaving && !access.allows(Permission::Leave) {
        // The owner, the only member who cannot leave.
        return Err(OxidGeneError::Conflict(
            "the owner transfers or deletes the tree rather than leave it".into(),
        ));
    }
    if !leaving && !access.allows(Permission::MembersManage) {
        return Err(OxidGeneError::Forbidden);
    }
    let membership = MembershipRepo::get(db, tree_id, user_id)
        .await?
        .ok_or_else(|| membership_not_found(user_id))?;
    let txn = begin_tx(db).await?;
    MembershipRepo::delete(&txn, tree_id, user_id).await?;
    drop_scope(&txn, membership.scope_id).await?;
    record(&txn, viewer, tree_id, AuditAction::Delete, membership.id).await?;
    let notices = removal_notices(&txn, viewer, mailing, (tree_id, user_id), leaving).await?;
    commit_tx(txn).await?;
    notices.publish();
    viewer.forget(tree_id);
    Ok(())
}

/// Delete the scope a membership that ended or became the owner's held.
async fn drop_scope(
    txn: &impl ConnectionTrait,
    scope_id: Option<Uuid>,
) -> Result<(), OxidGeneError> {
    match scope_id {
        Some(scope_id) => oxidgene_db::repo::AccessScopeRepo::delete(txn, scope_id).await,
        None => Ok(()),
    }
}

/// The notices of member `user_id` leaving tree `tree_id` (`member_left`,
/// to its owner) or being removed from it (`membership_ended`, to them).
async fn removal_notices(
    txn: &impl ConnectionTrait,
    viewer: &ViewerContext,
    mailing: Mailing,
    (tree_id, user_id): (Uuid, Uuid),
    leaving: bool,
) -> Result<Notices, OxidGeneError> {
    let about = About::tree(tree_id, viewer);
    let note = if leaving {
        MembershipRepo::owner(txn, tree_id)
            .await?
            .map(|owner| (NotificationKind::MemberLeft, owner.user_id, about))
    } else {
        Some((NotificationKind::MembershipEnded, user_id, about))
    };
    Notices::write(txn, mailing, note).await
}

/// Hand tree `tree_id` to its member `user_id`, who becomes its owner while
/// the former owner becomes an Editor, in one transaction (§5); both are
/// notified (`ownership_transferred`).
pub async fn transfer_ownership(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    tree_id: Uuid,
    user_id: Uuid,
) -> Result<MemberView, OxidGeneError> {
    let identity = member_identity(viewer)?;
    if user_id == identity.user_id {
        return Err(OxidGeneError::Validation(
            "the tree already belongs to its owner".into(),
        ));
    }
    let target = MembershipRepo::get(db, tree_id, user_id)
        .await?
        .filter(|membership| membership.is_live(Utc::now()))
        .ok_or_else(|| membership_not_found(user_id))?;
    require_active(db, user_id).await?;
    let txn = begin_tx(db).await?;
    let owner = MembershipRepo::owner(&txn, tree_id)
        .await?
        .ok_or(OxidGeneError::Forbidden)?;
    // The former owner first: one owner per tree at every step.
    MembershipRepo::update(
        &txn,
        owner.id,
        MembershipChanges {
            role: Some(TreeRole::Editor),
            ..MembershipChanges::default()
        },
    )
    .await?;
    let promoted = MembershipRepo::update(
        &txn,
        target.id,
        MembershipChanges {
            role: Some(TreeRole::Owner),
            scope_id: Some(None),
            expires_at: Some(None),
            ..MembershipChanges::default()
        },
    )
    .await?;
    // The owner sees the whole tree: the scope the member had goes.
    drop_scope(&txn, target.scope_id).await?;
    record(&txn, viewer, tree_id, AuditAction::Update, target.id).await?;
    let about = About::tree(tree_id, viewer).subject(user_id);
    let notices = Notices::write(
        &txn,
        mailing,
        [user_id, owner.user_id]
            .map(|reader| (NotificationKind::OwnershipTransferred, reader, about)),
    )
    .await?;
    commit_tx(txn).await?;
    notices.publish();
    viewer.forget(tree_id);
    one_view(db, promoted).await
}

/// Every live tree with its owner and size, for an administrator: metadata
/// only (§2).
pub async fn admin_trees(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    params: &PaginationParams,
) -> Result<Connection<AdminTree>, OxidGeneError> {
    require_admin(viewer.principal())?;
    let trees = TreeRepo::list(db, params).await?;
    let tree_ids: Vec<Uuid> = trees.edges.iter().map(|e| e.node.id).collect();
    let owners = MembershipRepo::owners(db, &tree_ids).await?;
    let owner_ids: Vec<Uuid> = owners.values().copied().collect();
    let names = AppUserRepo::shown_names(db, &owner_ids).await?;
    let counts: std::collections::HashMap<Uuid, i64> = PersonRepo::count_by_trees(db, &tree_ids)
        .await?
        .into_iter()
        .collect();
    Ok(trees.map(|tree| {
        let owner_id = owners.get(&tree.id).copied();
        AdminTree {
            owner_name: owner_id.and_then(|id| names.get(&id).cloned().flatten()),
            owner_id,
            person_count: counts.get(&tree.id).copied().unwrap_or(0),
            id: tree.id,
            name: tree.name,
            created_at: tree.created_at,
            updated_at: tree.updated_at,
        }
    }))
}

/// Give ownerless tree `tree_id` the owner `user_id`: an administrator's
/// act, refused (`409`) for a tree that has one. A member of the tree is
/// promoted; anyone else becomes its owner. The new owner is notified
/// (`ownership_transferred`).
pub async fn assign_owner(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    tree_id: Uuid,
    user_id: Uuid,
) -> Result<MemberView, OxidGeneError> {
    require_admin(viewer.principal())?;
    TreeRepo::get(db, tree_id).await?;
    require_active(db, user_id).await?;
    let txn = begin_tx(db).await?;
    if MembershipRepo::owner(&txn, tree_id).await?.is_some() {
        return Err(OxidGeneError::Conflict("the tree has an owner".into()));
    }
    let created_by = viewer.user().map(|identity| identity.user_id);
    let owner = match MembershipRepo::get(&txn, tree_id, user_id).await? {
        Some(member) => {
            MembershipRepo::update(
                &txn,
                member.id,
                MembershipChanges {
                    role: Some(TreeRole::Owner),
                    // The owner sees the whole tree; the maintenance pass
                    // deletes the scope nothing holds any more.
                    scope_id: Some(None),
                    expires_at: Some(None),
                    ..MembershipChanges::default()
                },
            )
            .await?
        }
        None => MembershipRepo::create(&txn, tree_id, user_id, TreeRole::Owner, created_by).await?,
    };
    record(&txn, viewer, tree_id, AuditAction::Create, owner.id).await?;
    let mut notices = Notices::new(mailing);
    notices
        .add(
            &txn,
            NotificationKind::OwnershipTransferred,
            user_id,
            About::tree(tree_id, viewer).subject(user_id),
        )
        .await?;
    commit_tx(txn).await?;
    notices.publish();
    let mut event = authz::log::event(viewer, "tree_owner_assigned", "ok");
    event.tree_id = Some(tree_id);
    authz::log::record(db, event);
    one_view(db, owner).await
}

/// Refuse a user who cannot hold a tree: unknown (`404`), deactivated or
/// erased (`409`).
async fn require_active(db: &DatabaseConnection, user_id: Uuid) -> Result<(), OxidGeneError> {
    let user = AppUserRepo::get(db, user_id).await?;
    match user.status {
        UserStatus::Active => Ok(()),
        UserStatus::Erased => Err(OxidGeneError::NotFound {
            entity: "User",
            id: user_id,
        }),
        UserStatus::Deactivated => Err(OxidGeneError::Conflict(
            "a deactivated user cannot be given a tree".into(),
        )),
    }
}

/// Audit a membership change under the tree's `access` category.
async fn record(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
    tree_id: Uuid,
    action: AuditAction,
    membership_id: Uuid,
) -> Result<(), OxidGeneError> {
    Change::new(
        viewer,
        tree_id,
        action,
        AuditEntity::Membership,
        membership_id,
    )
    .about_tree()
    .record_unversioned(db)
    .await
    .map(drop)
}
