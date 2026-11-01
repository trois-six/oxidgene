//! Who a request acts for, and what they may do with each tree
//! (docs/authorization.md §3, §4).
//!
//! - A [`Principal`] is who the request's credential names: the local owner
//!   of the desktop and of the web build's `none` mode, an anonymous
//!   visitor, or a signed-in user with the [`Identity`] their validated
//!   token carries.
//! - A [`ViewerContext`] holds the principal for one request, with the
//!   [`TreeAccess`] of each tree the request names, resolved once and kept
//!   for the request's later fields. Every service that writes takes it, to
//!   attribute its audit entry ([`Author`]); every surface checks it before
//!   an operation runs.
//! - A [`TreeRole`] maps to a [`PermissionSet`] by the matrix of §3.2.
//!
//! The types are shared by every build. Only the web build's `auth` feature
//! resolves another principal than [`Principal::Local`]; the desktop's
//! requests are always the local owner's, with every permission.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The issuer and subject the `none` mode's local owner is known by in a
/// job's recorded actor (docs/authentication.md §2).
pub const LOCAL_IDENTITY: &str = "local";

/// What a user may do on the instance as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceRole {
    /// Manages users and ownerless trees; reads no tree's content by virtue
    /// of the role.
    Admin,
    User,
}

impl InstanceRole {
    /// The stored and serialized value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    /// The role a stored value names; anything but `admin` is a user.
    pub fn parse(value: &str) -> Self {
        if value == "admin" {
            Self::Admin
        } else {
            Self::User
        }
    }
}

/// A viewer's role on one tree (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeRole {
    /// Derived from the tree's visibility or a share link: no membership.
    Visitor,
    /// Trusted with the whole tree, changes nothing.
    Guest,
    /// Builds the tree with the owner.
    Editor,
    /// The one account the tree belongs to.
    Owner,
}

impl TreeRole {
    /// The roles a membership stores.
    pub const MEMBER_ROLES: [Self; 3] = [Self::Owner, Self::Editor, Self::Guest];

    /// The stored and serialized value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Editor => "editor",
            Self::Guest => "guest",
            Self::Visitor => "visitor",
        }
    }

    /// The role a stored value names.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "editor" => Some(Self::Editor),
            "guest" => Some(Self::Guest),
            "visitor" => Some(Self::Visitor),
            _ => None,
        }
    }

    /// What the role may do: the matrix of §3.2.
    ///
    /// A Guest's `export` depends on the tree's `guests_may_export`, a
    /// setting of the privacy settings (Privacy §9); until it exists, Guests
    /// do not export.
    pub fn permissions(self) -> PermissionSet {
        use Permission as P;
        const VISITOR: &[Permission] = &[P::Read];
        const GUEST: &[Permission] = &[P::Read, P::ReadPrivate, P::ReadTags, P::Leave];
        const EDITOR: &[Permission] = &[
            P::Read,
            P::ReadPrivate,
            P::ReadConfidential,
            P::ReadTags,
            P::HistoryRead,
            P::Tools,
            P::Create,
            P::Edit,
            P::PrivacyEdit,
            P::Delete,
            P::Merge,
            P::HistoryRestore,
            P::Export,
            P::Leave,
        ];
        match self {
            Self::Visitor => PermissionSet::of(VISITOR),
            Self::Guest => PermissionSet::of(GUEST),
            Self::Editor => PermissionSet::of(EDITOR),
            // Every permission but leaving: the owner transfers or deletes.
            Self::Owner => PermissionSet::of(Permission::ALL).without(P::Leave),
        }
    }
}

/// One permission of the matrix (§3.2): a stable identifier the server
/// checks and `viewer_access.permissions` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Persons, families, events, places, sources, citations, notes and
    /// media within the viewer's scope.
    Read,
    /// Private records and contemporaries unredacted.
    ReadPrivate,
    /// Persons excluded from sharing.
    ReadExcluded,
    /// Confidential notes.
    ReadConfidential,
    /// The tag vocabulary and the tags of records.
    ReadTags,
    /// The audit log and record versions.
    HistoryRead,
    /// Anomalies, potential duplicates, ancestry completeness, unlocated
    /// places.
    Tools,
    Create,
    Edit,
    /// A record's privacy and a note's confidentiality.
    PrivacyEdit,
    Delete,
    /// Person merges, dictionary renames and merges, bulk tagging.
    Merge,
    HistoryRestore,
    Export,
    /// Into the existing tree.
    Import,
    /// Name, roots, privacy settings, visibility.
    SettingsManage,
    ExclusionsManage,
    /// Members, invitations, share links.
    MembersManage,
    ApiClientsManage,
    AccessLogRead,
    /// With the instance's tree creation right.
    TreeDuplicate,
    TreeDelete,
    OwnershipTransfer,
    /// End one's own membership.
    Leave,
}

impl Permission {
    /// Every permission, in the matrix's order.
    pub const ALL: &'static [Self] = &[
        Self::Read,
        Self::ReadPrivate,
        Self::ReadExcluded,
        Self::ReadConfidential,
        Self::ReadTags,
        Self::HistoryRead,
        Self::Tools,
        Self::Create,
        Self::Edit,
        Self::PrivacyEdit,
        Self::Delete,
        Self::Merge,
        Self::HistoryRestore,
        Self::Export,
        Self::Import,
        Self::SettingsManage,
        Self::ExclusionsManage,
        Self::MembersManage,
        Self::ApiClientsManage,
        Self::AccessLogRead,
        Self::TreeDuplicate,
        Self::TreeDelete,
        Self::OwnershipTransfer,
        Self::Leave,
    ];

    /// The stable identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadPrivate => "read_private",
            Self::ReadExcluded => "read_excluded",
            Self::ReadConfidential => "read_confidential",
            Self::ReadTags => "read_tags",
            Self::HistoryRead => "history_read",
            Self::Tools => "tools",
            Self::Create => "create",
            Self::Edit => "edit",
            Self::PrivacyEdit => "privacy_edit",
            Self::Delete => "delete",
            Self::Merge => "merge",
            Self::HistoryRestore => "history_restore",
            Self::Export => "export",
            Self::Import => "import",
            Self::SettingsManage => "settings_manage",
            Self::ExclusionsManage => "exclusions_manage",
            Self::MembersManage => "members_manage",
            Self::ApiClientsManage => "api_clients_manage",
            Self::AccessLogRead => "access_log_read",
            Self::TreeDuplicate => "tree_duplicate",
            Self::TreeDelete => "tree_delete",
            Self::OwnershipTransfer => "ownership_transfer",
            Self::Leave => "leave",
        }
    }

    fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// A set of permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionSet(u32);

impl PermissionSet {
    /// The set of `permissions`.
    pub fn of(permissions: &[Permission]) -> Self {
        Self(permissions.iter().fold(0, |bits, p| bits | p.bit()))
    }

    /// This set less `permission`.
    #[must_use]
    pub fn without(self, permission: Permission) -> Self {
        Self(self.0 & !permission.bit())
    }

    pub fn contains(self, permission: Permission) -> bool {
        self.0 & permission.bit() != 0
    }

    /// The permissions held, in the matrix's order.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::ALL
            .iter()
            .copied()
            .filter(move |p| self.contains(*p))
    }
}

/// What a viewer may do with one tree, for one request (§4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeAccess {
    pub role: TreeRole,
    pub permissions: PermissionSet,
    /// The access scope the viewer is limited to: the persons a share
    /// reaches (§6.5). Sharing fills it; until then, nobody is scoped.
    pub scope_id: Option<Uuid>,
    /// Whether the owner's exclusions hide persons from the viewer: always,
    /// unless the viewer is the owner (Privacy §6).
    pub exclusions_apply: bool,
}

impl TreeAccess {
    /// The access of a member holding `role`, unscoped.
    pub fn of_role(role: TreeRole) -> Self {
        Self {
            role,
            permissions: role.permissions(),
            scope_id: None,
            exclusions_apply: role != TreeRole::Owner,
        }
    }

    pub fn allows(&self, permission: Permission) -> bool {
        self.permissions.contains(permission)
    }

    /// The part the frontend sees, to offer what the viewer may do.
    pub fn viewer_access(&self) -> ViewerAccess {
        ViewerAccess {
            role: self.role,
            permissions: self.permissions.iter().collect(),
            scoped: self.scope_id.is_some(),
        }
    }
}

/// The public part of a [`TreeAccess`], returned on every tree read as
/// `viewer_access` (§4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewerAccess {
    pub role: TreeRole,
    pub permissions: Vec<Permission>,
    /// The viewer sees a share's scope rather than the whole tree.
    pub scoped: bool,
}

impl ViewerAccess {
    /// The local owner's: every permission of the owner, unscoped.
    pub fn local() -> Self {
        TreeAccess::of_role(TreeRole::Owner).viewer_access()
    }

    pub fn allows(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }
}

impl Default for ViewerAccess {
    /// What a tree read without a viewer reports: the local owner's, as the
    /// desktop has.
    fn default() -> Self {
        Self::local()
    }
}

/// The identity of a signed-in request, from a token validated for it
/// (docs/authentication.md §5.4).
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    /// The access token's `iss`.
    pub issuer: String,
    /// The access token's `sub`: never logged.
    pub subject: String,
    /// The user row the issuer and subject key.
    pub user_id: Uuid,
    /// The token's `client_id`, else `azp`.
    pub client_id: Option<String>,
    /// The token's scopes.
    pub scopes: Vec<String>,
    /// The digest of the session id, for a cookie session.
    pub session_id: Option<String>,
    /// The API client the `client_id` names, once API clients ship.
    pub api_client_id: Option<Uuid>,
    /// The user's instance role, read with the user row.
    pub instance_role: InstanceRole,
    /// Whether the user may create trees, read with the user row.
    pub can_create_trees: bool,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The subject identifies a person at the provider: never printed.
        f.debug_struct("Identity")
            .field("user_id", &self.user_id)
            .field("client_id", &self.client_id)
            .field("scopes", &self.scopes)
            .field("instance_role", &self.instance_role)
            .field("can_create_trees", &self.can_create_trees)
            .finish_non_exhaustive()
    }
}

/// Who a request acts for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// The single user of the desktop and of the `none` mode, owner of
    /// every tree: issuer and subject `local`.
    Local,
    /// No credential.
    Anonymous,
    /// A signed-in user, through a session or a trusted client's token.
    User(Identity),
}

impl Principal {
    /// The signed-in user, if any.
    pub fn user(&self) -> Option<&Identity> {
        match self {
            Self::User(identity) => Some(identity),
            Self::Local | Self::Anonymous => None,
        }
    }

    /// The client the request came through, for logs, spans and the
    /// history: configuration, not personal data.
    pub fn client_id(&self) -> Option<&str> {
        self.user()
            .and_then(|identity| identity.client_id.as_deref())
    }
}

/// Who an audit entry names as its author (§9.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    User,
    ApiClient,
    /// The desktop's user, or the `none` mode's.
    #[default]
    Local,
}

impl AuthorKind {
    /// The stored and serialized value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::ApiClient => "api_client",
            Self::Local => "local",
        }
    }

    /// The kind a stored value names; anything unknown is local.
    pub fn parse(value: &str) -> Self {
        match value {
            "user" => Self::User,
            "api_client" => Self::ApiClient,
            _ => Self::Local,
        }
    }
}

/// The author a write records: who, and through which client.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Author {
    pub kind: AuthorKind,
    pub user_id: Option<Uuid>,
    pub api_client_id: Option<Uuid>,
    /// The OAuth client the write came through: the web client, an
    /// assistant, an API client.
    pub client_id: Option<String>,
}

/// The actor of a background job, recorded at submission and acted for by
/// the worker (docs/authentication.md §5.5). Never a token.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct JobActor {
    pub issuer: Option<String>,
    pub subject: Option<String>,
    /// The user, `None` for the local owner.
    pub user_id: Option<Uuid>,
    pub client_id: Option<String>,
    pub api_client_id: Option<Uuid>,
    pub scopes: Vec<String>,
    /// The digest of the submitting session's id.
    pub session: Option<String>,
}

impl std::fmt::Debug for JobActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobActor")
            .field("user_id", &self.user_id)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

/// One request's viewer: its principal, and the access to each tree it
/// named, resolved once (§4.1). Never shared between requests.
#[derive(Debug)]
pub struct ViewerContext {
    principal: Principal,
    trees: Mutex<HashMap<Uuid, TreeAccess>>,
}

impl ViewerContext {
    pub fn new(principal: Principal) -> Self {
        Self {
            principal,
            trees: Mutex::default(),
        }
    }

    /// The local owner of every tree: the desktop, the `none` mode, the
    /// stdio MCP server.
    pub fn local() -> Self {
        Self::new(Principal::Local)
    }

    pub fn anonymous() -> Self {
        Self::new(Principal::Anonymous)
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The signed-in user, if any.
    pub fn user(&self) -> Option<&Identity> {
        self.principal.user()
    }

    pub fn is_local(&self) -> bool {
        matches!(self.principal, Principal::Local)
    }

    pub fn is_anonymous(&self) -> bool {
        matches!(self.principal, Principal::Anonymous)
    }

    /// The client the request came through.
    pub fn client_id(&self) -> Option<&str> {
        self.principal.client_id()
    }

    /// Whether the viewer may create a tree: the local owner always, a user
    /// with the instance's right.
    pub fn may_create_trees(&self) -> bool {
        match &self.principal {
            Principal::Local => true,
            Principal::User(identity) => identity.can_create_trees,
            Principal::Anonymous => false,
        }
    }

    /// Whether the viewer is an instance administrator.
    pub fn is_admin(&self) -> bool {
        self.user()
            .is_some_and(|identity| identity.instance_role == InstanceRole::Admin)
    }

    /// The author the viewer's writes record.
    pub fn author(&self) -> Author {
        match &self.principal {
            Principal::User(identity) => Author {
                kind: if identity.api_client_id.is_some() {
                    AuthorKind::ApiClient
                } else {
                    AuthorKind::User
                },
                user_id: Some(identity.user_id),
                api_client_id: identity.api_client_id,
                client_id: identity.client_id.clone(),
            },
            Principal::Local | Principal::Anonymous => Author::default(),
        }
    }

    /// The actor a job the viewer submits records.
    pub fn job_actor(&self) -> JobActor {
        match &self.principal {
            Principal::User(identity) => JobActor {
                issuer: Some(identity.issuer.clone()),
                subject: Some(identity.subject.clone()),
                user_id: Some(identity.user_id),
                client_id: identity.client_id.clone(),
                api_client_id: identity.api_client_id,
                scopes: identity.scopes.clone(),
                session: identity.session_id.clone(),
            },
            Principal::Local | Principal::Anonymous => JobActor {
                issuer: Some(LOCAL_IDENTITY.to_string()),
                subject: Some(LOCAL_IDENTITY.to_string()),
                ..JobActor::default()
            },
        }
    }

    /// The access to tree `tree_id` this request already resolved.
    pub fn cached(&self, tree_id: Uuid) -> Option<TreeAccess> {
        self.lock().get(&tree_id).copied()
    }

    /// Keep the access to tree `tree_id` for the rest of the request.
    pub fn remember(&self, tree_id: Uuid, access: TreeAccess) {
        self.lock().insert(tree_id, access);
    }

    /// Forget what the request resolved for tree `tree_id`: a write of the
    /// request changed the viewer's role on it.
    pub fn forget(&self, tree_id: Uuid) {
        self.lock().remove(&tree_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Uuid, TreeAccess>> {
        // A panic while holding it leaves a valid map.
        self.trees
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The matrix of docs/authorization.md §3.2, row by row: owner, editor,
    /// guest, visitor.
    const MATRIX: &[(Permission, [bool; 4])] = &[
        (Permission::Read, [true, true, true, true]),
        (Permission::ReadPrivate, [true, true, true, false]),
        (Permission::ReadExcluded, [true, false, false, false]),
        (Permission::ReadConfidential, [true, true, false, false]),
        (Permission::ReadTags, [true, true, true, false]),
        (Permission::HistoryRead, [true, true, false, false]),
        (Permission::Tools, [true, true, false, false]),
        (Permission::Create, [true, true, false, false]),
        (Permission::Edit, [true, true, false, false]),
        (Permission::PrivacyEdit, [true, true, false, false]),
        (Permission::Delete, [true, true, false, false]),
        (Permission::Merge, [true, true, false, false]),
        (Permission::HistoryRestore, [true, true, false, false]),
        (Permission::Export, [true, true, false, false]),
        (Permission::Import, [true, false, false, false]),
        (Permission::SettingsManage, [true, false, false, false]),
        (Permission::ExclusionsManage, [true, false, false, false]),
        (Permission::MembersManage, [true, false, false, false]),
        (Permission::ApiClientsManage, [true, false, false, false]),
        (Permission::AccessLogRead, [true, false, false, false]),
        (Permission::TreeDuplicate, [true, false, false, false]),
        (Permission::TreeDelete, [true, false, false, false]),
        (Permission::OwnershipTransfer, [true, false, false, false]),
        (Permission::Leave, [false, true, true, false]),
    ];

    #[test]
    fn each_role_holds_the_permissions_of_the_matrix() {
        assert_eq!(MATRIX.len(), Permission::ALL.len());
        let roles = [
            TreeRole::Owner,
            TreeRole::Editor,
            TreeRole::Guest,
            TreeRole::Visitor,
        ];
        for (permission, expected) in MATRIX {
            for (role, allowed) in roles.iter().zip(expected) {
                assert_eq!(
                    role.permissions().contains(*permission),
                    *allowed,
                    "{} for {}",
                    permission.as_str(),
                    role.as_str()
                );
            }
        }
    }

    #[test]
    fn identifiers_round_trip_through_serde() {
        for permission in Permission::ALL {
            let json = serde_json::to_string(permission).unwrap();
            assert_eq!(json, format!("\"{}\"", permission.as_str()));
        }
        for role in TreeRole::MEMBER_ROLES {
            assert_eq!(TreeRole::parse(role.as_str()), Some(role));
        }
        assert_eq!(TreeRole::parse("visitor"), Some(TreeRole::Visitor));
        assert_eq!(TreeRole::parse("admin"), None);
    }

    #[test]
    fn only_the_owner_skips_the_exclusions() {
        assert!(!TreeAccess::of_role(TreeRole::Owner).exclusions_apply);
        assert!(TreeAccess::of_role(TreeRole::Editor).exclusions_apply);
        assert!(TreeAccess::of_role(TreeRole::Guest).exclusions_apply);
    }

    #[test]
    fn the_local_owner_authors_as_local_and_a_user_as_themselves() {
        assert_eq!(ViewerContext::local().author(), Author::default());
        let identity = Identity {
            issuer: "https://id.example.invalid".into(),
            subject: "subject-value".into(),
            user_id: Uuid::nil(),
            client_id: Some("oxidgene-web".into()),
            scopes: vec![],
            session_id: Some("session-digest".into()),
            api_client_id: None,
            instance_role: InstanceRole::User,
            can_create_trees: false,
        };
        let viewer = ViewerContext::new(Principal::User(identity));
        let author = viewer.author();
        assert_eq!(author.kind, AuthorKind::User);
        assert_eq!(author.user_id, Some(Uuid::nil()));
        assert_eq!(author.client_id.as_deref(), Some("oxidgene-web"));
        assert!(!viewer.may_create_trees());
        let printed = format!("{viewer:?} {:?}", viewer.job_actor());
        assert!(!printed.contains("subject-value"), "{printed}");
        assert!(!printed.contains("session-digest"), "{printed}");
    }
}
