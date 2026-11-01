//! What the viewer may do with the tree on screen, so that a page offers
//! only the controls the server would accept (docs/authorization.md §11,
//! "Read-only views"; docs/ui-common.md §1).
//!
//! One value per tree page: [`TreePermissionsProvider`], rendered by the
//! layout around every routed page, reads the tree's `viewer_access` once —
//! from the same cached tree read the page itself makes — and every
//! component asks [`use_tree_permissions`] or [`use_can`]. The server checks
//! every operation regardless: hiding a control is a courtesy, not the
//! enforcement.
//!
//! The desktop and the web build run without authentication have one local
//! user who owns every tree: their permissions are the owner's, known at
//! once and without a request, so both render exactly what they rendered
//! before roles existed. Where the server signs users in, nothing editable
//! is offered until the access is known, rather than flashing controls at a
//! Visitor.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::account::AccountState;
use crate::api::ApiClient;
pub use oxidgene_core::access::Permission;
use oxidgene_core::access::{PermissionSet, TreeRole, ViewerAccess};

/// What the viewer may do with one tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreePermissions(Access);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// The desktop's, or the web build's without authentication: the local
    /// owner of every tree, who may do everything. Also the web build's
    /// until the server says whether it signs users in, so that the build
    /// run without authentication draws from the first frame what it always
    /// drew; a signed-in or anonymous viewer's controls then follow their
    /// access.
    Local,
    /// The server signs users in, and has not said yet what the tree lets
    /// the viewer do: nothing is offered.
    Pending,
    /// What the tree read reported (§4.1).
    Viewer {
        role: TreeRole,
        permissions: PermissionSet,
        /// The viewer sees a share's scope rather than the whole tree.
        scoped: bool,
    },
}

impl TreePermissions {
    /// The local owner's: every permission, without a request.
    pub const fn local() -> Self {
        Self(Access::Local)
    }

    /// Nothing yet: the access is still being asked for.
    pub const fn pending() -> Self {
        Self(Access::Pending)
    }

    /// The access a tree read reported.
    pub fn of(access: &ViewerAccess) -> Self {
        Self(Access::Viewer {
            role: access.role,
            permissions: PermissionSet::of(&access.permissions),
            scoped: access.scoped,
        })
    }

    /// What the viewer described by `account` may do with a tree whose read
    /// reported `access`, if it was read: the local owner's where the
    /// server signs nobody in, else the tree's answer, and nothing while
    /// either is unknown.
    pub fn resolve(account: &AccountState, access: Option<&ViewerAccess>) -> Self {
        match (account, access) {
            (AccountState::Absent, _) => Self::local(),
            (AccountState::Loading, _) => Self::local(),
            (_, None) => Self::pending(),
            (AccountState::Anonymous | AccountState::SignedIn(_), Some(access)) => Self::of(access),
        }
    }

    /// Whether the viewer holds `permission` (§3.2).
    pub fn can(self, permission: Permission) -> bool {
        match self.0 {
            Access::Local => TreeRole::Owner.permissions().contains(permission),
            Access::Pending => false,
            Access::Viewer { permissions, .. } => permissions.contains(permission),
        }
    }

    /// Whether the viewer may use `permission` on what the whole tree
    /// shares — places, sources, repositories, tags, the dictionary's
    /// values — and on its history: a scoped Editor creates and links
    /// them, but edits, merges, deletes, bulk-tags and restores nothing of
    /// it (§3.3).
    pub fn can_shared(self, permission: Permission) -> bool {
        self.can(permission) && !self.scoped()
    }

    /// Whether the viewer may edit a place, a source, a repository or a tag.
    pub fn can_edit_shared(self) -> bool {
        self.can_shared(Permission::Edit)
    }

    /// Whether the viewer may change any record of the tree: the person and
    /// record controls that take a write are offered.
    pub fn writes(self) -> bool {
        self.can(Permission::Create) || self.can(Permission::Edit) || self.can(Permission::Delete)
    }

    /// Whether the viewer may put tags on a record and take them off: the
    /// tags must be known to them, and the record theirs to edit.
    pub fn tags_records(self) -> bool {
        self.can(Permission::Edit) && self.can(Permission::ReadTags)
    }

    /// The viewer's role, once known; the owner's for the local user.
    pub fn role(self) -> Option<TreeRole> {
        match self.0 {
            Access::Local => Some(TreeRole::Owner),
            Access::Pending => None,
            Access::Viewer { role, .. } => Some(role),
        }
    }

    /// Whether the viewer is a Visitor of the tree (§3.1).
    pub fn is_visitor(self) -> bool {
        self.role() == Some(TreeRole::Visitor)
    }

    /// Whether the viewer sees a share's scope rather than the whole tree.
    pub fn scoped(self) -> bool {
        matches!(self.0, Access::Viewer { scoped: true, .. })
    }

    /// Whether this build offers the surfaces for a tree's other viewers:
    /// the server signs users in, as [`crate::sharing::offered`] says.
    pub fn shares(self) -> bool {
        matches!(self.0, Access::Pending | Access::Viewer { .. })
    }
}

/// The context [`TreePermissionsProvider`] provides.
#[derive(Clone, Copy)]
struct TreePermissionsContext {
    permissions: Memo<TreePermissions>,
    access: Resource<Option<ViewerAccess>>,
}

/// Provides what the viewer may do with tree `tree_id` to `children`: the
/// layout renders it around every routed page, keyed by the tree, so the
/// tree's access is read once per page from the cached tree read.
#[component]
pub fn TreePermissionsProvider(tree_id: Option<Uuid>, children: Element) -> Element {
    let api = use_context::<ApiClient>();
    let account = crate::account::use_account();
    // What an earlier page already read of the tree: a navigation draws its
    // first frame with it rather than with nothing.
    let seed = use_hook(|| tree_id.and_then(|tree| api.cached_viewer_access(tree)));
    let access = crate::ui_observability::use_ui_resource("viewer_access", move || {
        let api = api.clone();
        let surfaces = crate::sharing::offered(&account.read());
        async move {
            match tree_id.filter(|_| surfaces) {
                Some(tree) => api.viewer_access(tree).await.ok(),
                None => None,
            }
        }
    });
    let permissions = use_memo(move || {
        let read = access.read();
        let known = match &*read {
            Some(answer) => answer.as_ref(),
            None => seed.as_ref(),
        };
        TreePermissions::resolve(&account.read(), known)
    });
    use_context_provider(|| TreePermissionsContext {
        permissions,
        access,
    });
    children
}

/// What the viewer may do with the tree on screen. Outside a
/// [`TreePermissionsProvider`] — no tree — the local owner's where the
/// server signs nobody in, and nothing otherwise.
pub fn use_tree_permissions() -> TreePermissions {
    // Both asked on every render: a context lookup is a hook.
    let context = try_use_context::<TreePermissionsContext>();
    let account = try_use_context::<Signal<AccountState>>();
    match (context, account) {
        (Some(context), _) => (context.permissions)(),
        (None, Some(account)) => TreePermissions::resolve(&account.read(), None),
        (None, None) => TreePermissions::local(),
    }
}

/// Whether the viewer holds `permission` on the tree on screen.
pub fn use_can(permission: Permission) -> bool {
    use_tree_permissions().can(permission)
}

/// Renders `children` only when the viewer holds `permission`: the gate of
/// a control drawn by a plain function, which cannot call a hook itself.
#[component]
pub fn IfCan(permission: Permission, children: Element) -> Element {
    if use_can(permission) {
        children
    } else {
        rsx! {}
    }
}

/// What a page reserved to one permission shows, opened by its address
/// rather than from a control the viewer was offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAccess {
    /// The page itself.
    Open,
    /// The forbidden page: the viewer sees the tree but lacks the
    /// permission, as the server's `403` says (docs/authorization.md §4.3).
    Refused,
    /// Nothing yet: whether the server signs users in, or what the tree lets
    /// the viewer do, is still being asked. The page's body is not drawn
    /// until it is known.
    Unknown,
}

/// What a page needing `permission` shows to the user `account` describes,
/// who may do `permissions` with its tree. The local owner — the desktop's,
/// the `none` mode's — opens it as before; the web build waits for
/// `/auth/session` before drawing it, so that no one is shown a body they
/// may not read.
pub fn page_access(
    account: &AccountState,
    permissions: TreePermissions,
    permission: Permission,
) -> PageAccess {
    match (account, permissions.0) {
        (AccountState::Loading, _) | (_, Access::Pending) => PageAccess::Unknown,
        _ if permissions.can(permission) => PageAccess::Open,
        _ => PageAccess::Refused,
    }
}

/// Renders `children`, a page reserved to whom holds `permission`, or in
/// its place the forbidden page, or nothing while the access is unknown
/// (docs/authorization.md §11). The navigation offers the page only to
/// whom may open it; this answers its address typed or bookmarked.
#[component]
pub fn RequirePermission(permission: Permission, children: Element) -> Element {
    let permissions = use_tree_permissions();
    let account = try_use_context::<Signal<AccountState>>();
    let access = match account {
        Some(account) => page_access(&account.read(), permissions, permission),
        None => page_access(&AccountState::Absent, permissions, permission),
    };
    match access {
        PageAccess::Open => children,
        PageAccess::Refused => rsx! { crate::pages::not_found::Forbidden {} },
        PageAccess::Unknown => rsx! {},
    }
}

/// Whether a form may delete a source its own edit left unused, as `access`,
/// the tree's last read, says: a scoped Editor creates sources, places,
/// repositories and tags but deletes none (§3.3), so the clean-up is not
/// attempted. Without a read at hand it is: the server keeps whatever it
/// refuses.
pub fn deletes_shared_records(access: Option<&ViewerAccess>) -> bool {
    access.is_none_or(|access| TreePermissions::of(access).can_shared(Permission::Delete))
}

/// Asks the tree's access again, after a change to the viewer's own role
/// on it: an ownership transferred.
pub fn use_refresh_tree_permissions() -> Callback<()> {
    let context = try_use_context::<TreePermissionsContext>();
    use_callback(move |()| {
        if let Some(mut context) = context {
            context.access.restart();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxidgene_core::access::TreeAccess;

    fn viewer(role: TreeRole, scoped: bool) -> TreePermissions {
        let access = TreeAccess::of_role(role).scoped(scoped.then(Uuid::nil));
        let read = access.viewer_access();
        TreePermissions::resolve(&AccountState::Anonymous, Some(&read))
    }

    /// Every permission, held or not, as a list to compare.
    fn held(permissions: TreePermissions) -> Vec<Permission> {
        Permission::ALL
            .iter()
            .copied()
            .filter(|permission| permissions.can(*permission))
            .collect()
    }

    #[test]
    fn the_local_owner_may_do_everything_without_asking() {
        let local = TreePermissions::resolve(&AccountState::Absent, None);
        assert_eq!(local, TreePermissions::local());
        assert_eq!(
            held(local),
            TreeRole::Owner.permissions().iter().collect::<Vec<_>>()
        );
        assert!(local.can_edit_shared());
        assert!(local.can_shared(Permission::Merge));
        assert!(local.can_shared(Permission::HistoryRestore));
        assert!(local.tags_records());
        assert!(!local.shares());
        assert!(!local.is_visitor());
    }

    #[test]
    fn nothing_is_offered_while_a_viewers_access_is_unknown() {
        let account = AccountState::Anonymous;
        let permissions = TreePermissions::resolve(&account, None);
        assert!(held(permissions).is_empty());
        assert!(!permissions.writes());
        assert_eq!(permissions.role(), None);
        // The sharing surfaces follow the account state, as before.
        assert!(permissions.shares());
        assert_eq!(permissions.shares(), crate::sharing::offered(&account));
        assert_eq!(
            TreePermissions::resolve(&AccountState::Anonymous, None),
            TreePermissions::pending()
        );
    }

    /// Until the server says whether it signs users in, the web build draws
    /// what the desktop and the `none` mode draw, sharing surfaces aside.
    #[test]
    fn the_web_build_draws_as_the_local_owner_until_the_server_answers() {
        let read = TreeAccess::of_role(TreeRole::Visitor).viewer_access();
        for access in [None, Some(&read)] {
            let loading = TreePermissions::resolve(&AccountState::Loading, access);
            assert_eq!(
                loading,
                TreePermissions::resolve(&AccountState::Absent, None)
            );
            assert!(!loading.shares());
        }
    }

    #[test]
    fn the_owner_may_do_everything_but_leave() {
        let owner = viewer(TreeRole::Owner, false);
        assert!(owner.writes());
        assert!(owner.can_edit_shared());
        assert!(owner.can(Permission::SettingsManage));
        assert!(owner.can(Permission::ExclusionsManage));
        assert!(owner.can(Permission::Import));
        assert!(!owner.can(Permission::Leave));
        assert!(owner.shares());
    }

    #[test]
    fn an_editor_writes_records_but_manages_nothing_of_the_tree() {
        let editor = viewer(TreeRole::Editor, false);
        for permission in [
            Permission::Create,
            Permission::Edit,
            Permission::Delete,
            Permission::Merge,
            Permission::PrivacyEdit,
            Permission::HistoryRead,
            Permission::HistoryRestore,
            Permission::Tools,
            Permission::Export,
        ] {
            assert!(editor.can(permission), "{}", permission.as_str());
        }
        assert!(editor.can_edit_shared());
        assert!(editor.can_shared(Permission::Merge));
        for permission in [
            Permission::SettingsManage,
            Permission::ExclusionsManage,
            Permission::Import,
            Permission::MembersManage,
        ] {
            assert!(!editor.can(permission), "{}", permission.as_str());
        }
    }

    #[test]
    fn a_scoped_editor_writes_persons_but_not_what_the_tree_shares() {
        let editor = viewer(TreeRole::Editor, true);
        assert!(editor.scoped());
        assert!(editor.can(Permission::Create));
        assert!(editor.can(Permission::Edit));
        assert!(editor.can(Permission::Delete));
        assert!(editor.can(Permission::Merge));
        assert!(editor.tags_records());
        assert!(!editor.can_edit_shared());
        assert!(!editor.can_shared(Permission::Delete));
        assert!(!editor.can_shared(Permission::Merge));
        assert!(!editor.can_shared(Permission::HistoryRestore));
    }

    #[test]
    fn a_guest_reads_everything_and_changes_nothing() {
        for scoped in [false, true] {
            let guest = viewer(TreeRole::Guest, scoped);
            assert!(guest.can(Permission::Read));
            assert!(guest.can(Permission::ReadTags));
            assert!(guest.can(Permission::Leave));
            assert!(!guest.writes());
            assert!(!guest.tags_records());
            assert!(!guest.can(Permission::PrivacyEdit));
            assert!(!guest.can(Permission::Merge));
            assert!(!guest.can(Permission::HistoryRead));
            assert!(!guest.can(Permission::Tools));
            assert!(!guest.can(Permission::SettingsManage));
            assert!(!guest.can_edit_shared());
            assert!(!guest.is_visitor());
        }
    }

    #[test]
    fn a_visitor_only_reads_and_knows_no_tags() {
        let visitor = viewer(TreeRole::Visitor, false);
        assert!(visitor.is_visitor());
        assert_eq!(held(visitor), vec![Permission::Read]);
        assert!(!visitor.can(Permission::ReadTags));
        assert!(!visitor.writes());
    }

    #[test]
    fn a_page_opens_to_whom_holds_its_permission_and_is_refused_to_the_others() {
        let signed_in = AccountState::Anonymous;
        for (role, scoped, opens) in [
            (TreeRole::Owner, false, true),
            (TreeRole::Editor, false, true),
            (TreeRole::Editor, true, true),
            (TreeRole::Guest, false, false),
            (TreeRole::Guest, true, false),
            (TreeRole::Visitor, false, false),
        ] {
            let permissions = viewer(role, scoped);
            let expected = if opens {
                PageAccess::Open
            } else {
                PageAccess::Refused
            };
            for permission in [Permission::Tools, Permission::HistoryRead] {
                assert_eq!(
                    page_access(&signed_in, permissions, permission),
                    expected,
                    "{role:?} scoped={scoped} {}",
                    permission.as_str()
                );
            }
        }
        // The desktop and the `none` mode open it at once, as before.
        let local = TreePermissions::resolve(&AccountState::Absent, None);
        assert_eq!(
            page_access(&AccountState::Absent, local, Permission::Tools),
            PageAccess::Open
        );
        // Nothing is drawn before the server has answered: whether it signs
        // users in, then what the tree lets the viewer do.
        let loading = TreePermissions::resolve(&AccountState::Loading, None);
        assert_eq!(
            page_access(&AccountState::Loading, loading, Permission::Tools),
            PageAccess::Unknown
        );
        assert_eq!(
            page_access(&signed_in, TreePermissions::pending(), Permission::Tools),
            PageAccess::Unknown
        );
    }

    #[test]
    fn a_form_cleans_up_shared_records_unless_its_editor_is_scoped() {
        let read = |role: TreeRole, scoped: bool| {
            TreeAccess::of_role(role)
                .scoped(scoped.then(Uuid::nil))
                .viewer_access()
        };
        assert!(deletes_shared_records(None));
        assert!(deletes_shared_records(Some(&read(TreeRole::Owner, false))));
        assert!(deletes_shared_records(Some(&read(TreeRole::Editor, false))));
        assert!(!deletes_shared_records(Some(&read(TreeRole::Editor, true))));
        assert!(!deletes_shared_records(Some(&read(TreeRole::Guest, false))));
    }

    #[test]
    fn a_guest_exports_when_the_tree_lets_guests_export() {
        let access = TreeAccess::of_role(TreeRole::Guest)
            .with_guest_export(true)
            .viewer_access();
        let guest = TreePermissions::resolve(&AccountState::Anonymous, Some(&access));
        assert!(guest.can(Permission::Export));
        assert!(!guest.writes());
    }
}
