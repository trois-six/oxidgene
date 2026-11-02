//! Sharing a tree with people (docs/authorization.md §6.1, §6.2): with a
//! user of the instance, named by exact email, exact username or among the
//! people the owner already shares with, and by email with someone who has
//! no account yet. REST and GraphQL both go through here. Web build only.
//!
//! - A match makes a membership at once, with the role, scope and expiry
//!   the owner chose.
//! - An address no account has verified becomes an invitation, which its
//!   holder accepts or declines once signed in with that verified address.
//!   OxidGene mails it through the [`InvitationOutbox`] seam when the
//!   deployment has one (docs/messaging.md §3); without, nothing is mailed
//!   and the owner copies the invitation link and sends it themselves.
//! - Looking an address or a username up says whether it has an account, as
//!   any invitation system does: it is limited to [`LOOKUPS_PER_HOUR`] per
//!   user and recorded in the access log.
//!
//! Every change is audited in the tree's history under the `access`
//! category and stores no version (§9.1).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::{Identity, Permission, Principal, TreeRole, ViewerContext};
use oxidgene_core::history::{AuditAction, AuditEntity};
use oxidgene_core::sharing::ScopeDefinition;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{
    AppUserRepo, InvitationEnd, InvitationRepo, MembershipRepo, NewInvitation, PaginationParams,
    SharedMembership, TreeInvitation, TreeRepo, UserStatus,
};
use oxidgene_db::sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::authz;
use crate::service::access_scope::{self, ScopeView};
use crate::service::history::Change;
use crate::service::membership::{MemberView, member_view};
use crate::service::notification::{About, Mailing, Notices, NotificationKind};
use crate::service::scope::{begin_tx, commit_tx};

/// How long an invitation waits for an answer by default.
pub const INVITATION_DAYS: i64 = 14;
/// The longest an invitation may wait.
pub const MAX_INVITATION_DAYS: i64 = 90;
/// Lookups by email or username a user may make in an hour (§6.1).
pub const LOOKUPS_PER_HOUR: usize = 30;
/// The most people the share candidates list.
pub const MAX_SHARE_CANDIDATES: u64 = 50;

// ── The mail seam ───────────────────────────────────────────────────────

/// An invitation to mail (docs/messaging.md §3.3): the ids it is rendered
/// from when the worker sends it — the tree's name, the inviter's, the role
/// and the link are read then — and the address. Never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct InvitationMail {
    pub invitation_id: Uuid,
    pub tree_id: Uuid,
    /// The invited address.
    pub email: String,
    /// The owner who invited.
    pub inviter_id: Option<Uuid>,
}

impl std::fmt::Debug for InvitationMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvitationMail")
            .field("invitation_id", &self.invitation_id)
            .field("tree_id", &self.tree_id)
            .finish_non_exhaustive()
    }
}

/// Where an invitation's mail goes: the outbox of docs/messaging.md §3.3,
/// written in the transaction of the invitation so that a rolled-back
/// write sends nothing. [`crate::mail::MailOutbox`] implements it; a
/// deployment without a relay mails nothing.
#[async_trait::async_trait]
pub trait InvitationOutbox: Send + Sync + std::fmt::Debug {
    /// Whether invitations are mailed at all: SMTP is configured.
    fn delivers(&self) -> bool;

    /// Queue `mail`, in `txn`.
    async fn enqueue(
        &self,
        txn: &DatabaseTransaction,
        mail: InvitationMail,
    ) -> Result<(), OxidGeneError>;

    /// The transaction that queued a mail committed: wake whoever sends it.
    fn committed(&self) {}
}

/// No mail relay: nothing is mailed, and the owner shares the invitation
/// link by hand (docs/messaging.md §3.5).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoOutbox;

#[async_trait::async_trait]
impl InvitationOutbox for NoOutbox {
    fn delivers(&self) -> bool {
        false
    }

    async fn enqueue(
        &self,
        _txn: &DatabaseTransaction,
        _mail: InvitationMail,
    ) -> Result<(), OxidGeneError> {
        Ok(())
    }
}

/// What sharing needs of the deployment: the public URL its links start
/// with and the invitations' outbox.
#[derive(Debug, Clone)]
pub struct SharingEnv {
    /// The application's public URL, without a trailing slash.
    pub public_url: String,
    pub outbox: Arc<dyn InvitationOutbox>,
}

impl SharingEnv {
    /// The invitation link: `{public_url}/invitations`.
    pub fn invitation_link(&self) -> String {
        format!("{}/invitations", self.public_url)
    }

    /// Whether the deployment mails: invitations and notifications alike.
    pub fn mailing(&self) -> Mailing {
        Mailing(self.outbox.delivers())
    }

    /// How invitations reach their address.
    pub fn delivery(&self) -> Delivery {
        if self.outbox.delivers() {
            Delivery::Email
        } else {
            Delivery::Manual
        }
    }
}

/// How an invitation reaches its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// OxidGene mails it.
    Email,
    /// The owner copies the link and sends it themselves.
    Manual,
}

// ── Views ───────────────────────────────────────────────────────────────

/// An invitation as the owner's members section lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InvitationView {
    pub id: Uuid,
    pub email: String,
    pub role: TreeRole,
    pub scope: Option<ScopeView>,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub last_sent_at: Option<DateTime<Utc>>,
    /// The link to send by hand, `{public_url}/invitations`: it carries no
    /// secret, and only the invited verified address accepts.
    pub link: String,
    pub delivery: Delivery,
}

/// An invitation as the invited user sees it on their home.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MyInvitation {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub tree_name: String,
    /// The display name of the owner who invited.
    pub inviter_name: Option<String>,
    pub role: TreeRole,
    /// Whether the membership will be limited to a part of the tree.
    pub scoped: bool,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// One of the people the owner already shares with (§6.1): never their
/// address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ShareCandidate {
    pub user_id: Uuid,
    pub display_name: Option<String>,
}

/// What sharing made: a membership (`201`), or an invitation (`202`) for
/// an address no account has.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShareOutcome {
    pub member: Option<MemberView>,
    pub invitation: Option<InvitationView>,
}

/// A share: one of `email`, `username` and `user_id`, the role, and
/// optionally a scope, the membership's expiry and the invitation's.
#[derive(Debug, Clone, Deserialize)]
pub struct ShareRequest {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub user_id: Option<Uuid>,
    pub role: TreeRole,
    #[serde(default)]
    pub scope: Option<ScopeDefinition>,
    /// When the membership ends; none for never.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// When an invitation this share makes stops waiting: 14 days by
    /// default, at most 90.
    #[serde(default)]
    pub invitation_expires_at: Option<DateTime<Utc>>,
}

/// Who a share names.
enum Target {
    Email(String),
    Username(String),
    User(Uuid),
}

impl ShareRequest {
    fn target(&self) -> Result<Target, OxidGeneError> {
        let email = self
            .email
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let username = self
            .username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        match (email, username, self.user_id) {
            (Some(email), None, None) => Ok(Target::Email(normalized_email(email)?)),
            (None, Some(username), None) => Ok(Target::Username(username.to_string())),
            (None, None, Some(user_id)) => Ok(Target::User(user_id)),
            _ => Err(OxidGeneError::Validation(
                "a share names exactly one of email, username or user_id".into(),
            )),
        }
    }
}

/// `email`, trimmed and lower-cased, if it looks like an address.
pub fn normalized_email(email: &str) -> Result<String, OxidGeneError> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.chars().any(char::is_whitespace)
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'));
    if valid {
        Ok(email)
    } else {
        Err(OxidGeneError::Validation("not an email address".into()))
    }
}

/// The role a share or a member update grants: Editor or Guest.
pub fn shared_role(role: TreeRole) -> Result<TreeRole, OxidGeneError> {
    match role {
        TreeRole::Editor | TreeRole::Guest => Ok(role),
        _ => Err(OxidGeneError::Validation(
            "a member is an editor or a guest".into(),
        )),
    }
}

/// Refuse an expiry already past.
pub fn future(expires_at: Option<DateTime<Utc>>) -> Result<(), OxidGeneError> {
    match expires_at {
        Some(at) if at <= Utc::now() => Err(OxidGeneError::Validation(
            "an expiry is in the future".into(),
        )),
        _ => Ok(()),
    }
}

/// The signed-in owner `viewer` is. The `none` mode's local owner has no
/// account: sharing is absent there, as on the desktop (`404`).
fn sharer(viewer: &ViewerContext) -> Result<&Identity, OxidGeneError> {
    match viewer.principal() {
        Principal::User(identity) => Ok(identity),
        Principal::Anonymous => Err(OxidGeneError::Unauthenticated),
        Principal::Local => Err(OxidGeneError::NotFound {
            entity: "Membership",
            id: Uuid::nil(),
        }),
    }
}

// ── Lookups' rate limit ─────────────────────────────────────────────────

/// The lookups each user made in the last hour, in this process: only the
/// users who made one in that hour, so it never outgrows them.
static LOOKUPS: LazyLock<Mutex<HashMap<Uuid, VecDeque<Instant>>>> = LazyLock::new(Mutex::default);

/// Count one lookup of `user_id` — or one resend of an invitation mail —:
/// `rate_limited` past [`LOOKUPS_PER_HOUR`] in the last hour, with the
/// seconds until the oldest one leaves it.
fn count_lookup(user_id: Uuid) -> Result<(), OxidGeneError> {
    count_lookup_at(
        &mut LOOKUPS.lock().unwrap_or_else(|poison| poison.into_inner()),
        user_id,
        Instant::now(),
    )
}

/// [`count_lookup`] in `lookups` at `now`.
fn count_lookup_at(
    lookups: &mut HashMap<Uuid, VecDeque<Instant>>,
    user_id: Uuid,
    now: Instant,
) -> Result<(), OxidGeneError> {
    let hour = Duration::from_secs(3600);
    // Every user's lookups older than the hour leave; a user left with
    // none leaves the map.
    lookups.retain(|_, made| {
        while made
            .front()
            .is_some_and(|at| now.duration_since(*at) >= hour)
        {
            made.pop_front();
        }
        !made.is_empty()
    });
    let made = lookups.entry(user_id).or_default();
    if made.len() >= LOOKUPS_PER_HOUR {
        let oldest = made.front().copied().unwrap_or(now);
        let wait = hour
            .saturating_sub(now.duration_since(oldest))
            .as_secs()
            .max(1);
        return Err(OxidGeneError::RateLimited { retry_after: wait });
    }
    made.push_back(now);
    Ok(())
}

/// Record a lookup in the access log: its outcome, never the address or
/// the username.
fn log_lookup(db: &DatabaseConnection, viewer: &ViewerContext, tree_id: Uuid, outcome: &str) {
    let mut event = authz::log::event(viewer, "share_lookup", outcome);
    event.tree_id = Some(tree_id);
    authz::log::record(db, event);
}

// ── Sharing ─────────────────────────────────────────────────────────────

/// Share tree `tree_id` as `request` says (§6.1): a membership for an
/// account the request names, an invitation for an address no account has
/// verified. A username or a user nobody has is `not_found`; someone
/// already a member, or already invited, is a `conflict`.
pub async fn share(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    request: ShareRequest,
) -> Result<ShareOutcome, OxidGeneError> {
    let identity = sharer(viewer)?;
    authz::authorize(db, viewer, tree_id, Permission::MembersManage).await?;
    let role = shared_role(request.role)?;
    future(request.expires_at)?;
    future(request.invitation_expires_at)?;
    if let Some(scope) = &request.scope {
        access_scope::check_definition(db, tree_id, scope).await?;
    }
    let target = request.target()?;
    let user = find(db, identity, tree_id, &target).await?;
    answer(viewer, db, env, tree_id, (user, target), role, &request).await
}

/// What sharing with what `found` names makes: a membership for an account,
/// an invitation for an address none has, `not_found` otherwise.
async fn answer(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    found: (Option<oxidgene_db::repo::AppUser>, Target),
    role: TreeRole,
    request: &ShareRequest,
) -> Result<ShareOutcome, OxidGeneError> {
    let lookup = !matches!(found.1, Target::User(_));
    match found {
        (Some(user), _) => {
            if lookup {
                log_lookup(db, viewer, tree_id, "member");
            }
            let member = add_member(viewer, (db, env), tree_id, user.id, role, request).await?;
            Ok(ShareOutcome {
                member: Some(member),
                invitation: None,
            })
        }
        (None, Target::Email(email)) => {
            log_lookup(db, viewer, tree_id, "invitation");
            let invitation = invite(viewer, db, env, tree_id, email, role, request).await?;
            Ok(ShareOutcome {
                member: None,
                invitation: Some(invitation),
            })
        }
        (None, target) => {
            if lookup {
                log_lookup(db, viewer, tree_id, "not_found");
            }
            Err(OxidGeneError::NotFound {
                entity: "User",
                id: match target {
                    Target::User(id) => id,
                    _ => Uuid::nil(),
                },
            })
        }
    }
}

/// The active account `target` names: by verified email or username,
/// each lookup counted against the owner's limit, or among the people the
/// owner already shares with.
async fn find(
    db: &DatabaseConnection,
    owner: &Identity,
    tree_id: Uuid,
    target: &Target,
) -> Result<Option<oxidgene_db::repo::AppUser>, OxidGeneError> {
    let user = match target {
        Target::Email(email) => {
            count_lookup(owner.user_id)?;
            AppUserRepo::find_by_verified_email(db, email).await?
        }
        Target::Username(username) => {
            count_lookup(owner.user_id)?;
            AppUserRepo::find_by_username(db, username).await?
        }
        Target::User(user_id) => {
            let candidates =
                MembershipRepo::co_members(db, owner.user_id, tree_id, u64::MAX).await?;
            if candidates.contains(user_id) {
                Some(AppUserRepo::get(db, *user_id).await?)
            } else {
                None
            }
        }
    };
    Ok(user.filter(|user| user.status == UserStatus::Active))
}

/// Make `user_id` a member of `tree_id` as `request` says, and tell them
/// (`tree_shared`).
async fn add_member(
    viewer: &ViewerContext,
    (db, env): (&DatabaseConnection, &SharingEnv),
    tree_id: Uuid,
    user_id: Uuid,
    role: TreeRole,
    request: &ShareRequest,
) -> Result<MemberView, OxidGeneError> {
    // Checked in the transaction that writes, so that a concurrent share
    // meets the check or the unique index, a `409` either way.
    let txn = begin_tx(db).await?;
    if let Some(current) = MembershipRepo::get(&txn, tree_id, user_id).await? {
        if current.is_live(Utc::now()) {
            return Err(OxidGeneError::Conflict(
                "the user already holds a role on the tree".into(),
            ));
        }
        // An expired membership the maintenance pass has not deleted yet
        // grants nothing: it ends here, as that pass would end it, and the
        // new share replaces it.
        MembershipRepo::delete(&txn, tree_id, user_id).await?;
        oxidgene_db::repo::AccessEventRepo::insert(
            &txn,
            oxidgene_db::repo::NewAccessEvent {
                kind: "membership_expired",
                principal_kind: "user",
                user_id: Some(user_id),
                tree_id: Some(tree_id),
                outcome: current.role.as_str().to_string(),
                ..oxidgene_db::repo::NewAccessEvent::default()
            },
        )
        .await?;
    }
    let scope_id = access_scope::create_optional(&txn, tree_id, request.scope.as_ref()).await?;
    let membership = MembershipRepo::create_shared(
        &txn,
        SharedMembership {
            tree_id,
            user_id,
            role,
            scope_id,
            expires_at: request.expires_at,
            created_by: viewer.user().map(|identity| identity.user_id),
        },
    )
    .await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Create,
        AuditEntity::Membership,
        membership.id,
    )
    .await?;
    let mut notices = Notices::new(env.mailing());
    notices
        .add(
            &txn,
            NotificationKind::TreeShared,
            user_id,
            About::tree(tree_id, viewer),
        )
        .await?;
    commit_tx(txn).await?;
    notices.publish();
    let views = member_views(db, vec![membership]).await?;
    views.into_iter().next().ok_or(OxidGeneError::NotFound {
        entity: "Membership",
        id: user_id,
    })
}

/// The members `memberships` are, as the members list shows them.
pub(crate) async fn member_views(
    conn: &impl ConnectionTrait,
    memberships: Vec<oxidgene_db::repo::TreeMembership>,
) -> Result<Vec<MemberView>, OxidGeneError> {
    let user_ids: Vec<Uuid> = memberships.iter().map(|m| m.user_id).collect();
    let names = AppUserRepo::shown_names(conn, &user_ids).await?;
    let scope_ids: Vec<Uuid> = memberships.iter().filter_map(|m| m.scope_id).collect();
    let scopes = access_scope::views(conn, &scope_ids).await?;
    Ok(memberships
        .into_iter()
        .map(|membership| member_view(membership, &names, &scopes))
        .collect())
}

/// Invite `email` to `tree_id` as `request` says, and queue its mail.
async fn invite(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    email: String,
    role: TreeRole,
    request: &ShareRequest,
) -> Result<InvitationView, OxidGeneError> {
    let now = Utc::now();
    let expires_at = invitation_expiry(now, request.invitation_expires_at)?;
    // Checked in the transaction that writes; a concurrent invitation of
    // the same address meets the unique index instead, a `409` too.
    let txn = begin_tx(db).await?;
    if InvitationRepo::pending_for(&txn, tree_id, &email, now)
        .await?
        .is_some()
    {
        return Err(OxidGeneError::Conflict(
            "the address already has a pending invitation".into(),
        ));
    }
    InvitationRepo::delete_expired_for(&txn, tree_id, &email, now).await?;
    let scope_id = access_scope::create_optional(&txn, tree_id, request.scope.as_ref()).await?;
    let invitation = InvitationRepo::create(
        &txn,
        NewInvitation {
            tree_id,
            email,
            role,
            scope_id,
            membership_expires_at: request.expires_at,
            expires_at,
            created_by: viewer.user().map(|identity| identity.user_id),
        },
    )
    .await?;
    mail(&txn, viewer, env, &invitation).await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Create,
        AuditEntity::TreeInvitation,
        invitation.id,
    )
    .await?;
    commit_tx(txn).await?;
    env.outbox.committed();
    invitation_views(db, env, vec![invitation])
        .await
        .map(|mut views| views.remove(0))
}

/// When an invitation sent at `now` stops waiting: `asked`, at most 90
/// days away, else in 14 days.
fn invitation_expiry(
    now: DateTime<Utc>,
    asked: Option<DateTime<Utc>>,
) -> Result<DateTime<Utc>, OxidGeneError> {
    let latest = now + chrono::Duration::days(MAX_INVITATION_DAYS);
    match asked {
        Some(at) if at > latest => Err(OxidGeneError::Validation(format!(
            "an invitation waits at most {MAX_INVITATION_DAYS} days"
        ))),
        Some(at) => Ok(at),
        None => Ok(now + chrono::Duration::days(INVITATION_DAYS)),
    }
}

/// Queue the mail of `invitation`, when the deployment mails.
async fn mail(
    txn: &DatabaseTransaction,
    viewer: &ViewerContext,
    env: &SharingEnv,
    invitation: &TreeInvitation,
) -> Result<(), OxidGeneError> {
    if !env.outbox.delivers() {
        return Ok(());
    }
    env.outbox
        .enqueue(
            txn,
            InvitationMail {
                invitation_id: invitation.id,
                tree_id: invitation.tree_id,
                email: invitation.email.clone(),
                inviter_id: viewer.user().map(|identity| identity.user_id),
            },
        )
        .await
}

/// The views of `invitations`.
async fn invitation_views(
    conn: &impl ConnectionTrait,
    env: &SharingEnv,
    invitations: Vec<TreeInvitation>,
) -> Result<Vec<InvitationView>, OxidGeneError> {
    let scope_ids: Vec<Uuid> = invitations.iter().filter_map(|i| i.scope_id).collect();
    let scopes = access_scope::views(conn, &scope_ids).await?;
    Ok(invitations
        .into_iter()
        .map(|invitation| InvitationView {
            scope: invitation.scope_id.and_then(|id| scopes.get(&id).cloned()),
            id: invitation.id,
            email: invitation.email,
            role: invitation.role,
            membership_expires_at: invitation.membership_expires_at,
            expires_at: invitation.expires_at,
            created_at: invitation.created_at,
            last_sent_at: invitation.last_sent_at,
            link: env.invitation_link(),
            delivery: env.delivery(),
        })
        .collect())
}

/// The people the owner of `tree_id` already shares with (§6.1): the
/// members of their other trees who are not yet members of this one, at
/// most [`MAX_SHARE_CANDIDATES`].
pub async fn share_candidates(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
) -> Result<Vec<ShareCandidate>, OxidGeneError> {
    let identity = sharer(viewer)?;
    authz::authorize(db, viewer, tree_id, Permission::MembersManage).await?;
    let users =
        MembershipRepo::co_members(db, identity.user_id, tree_id, MAX_SHARE_CANDIDATES).await?;
    let names = AppUserRepo::shown_names(db, &users).await?;
    Ok(users
        .into_iter()
        .filter_map(|user_id| {
            // An erased user has no name, and is no one to share with.
            let display_name = names.get(&user_id)?.clone();
            display_name.as_ref()?;
            Some(ShareCandidate {
                user_id,
                display_name,
            })
        })
        .collect())
}

// ── The owner's invitations ─────────────────────────────────────────────

/// The pending invitations of `tree_id`, oldest first.
pub async fn list_invitations(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<InvitationView>, OxidGeneError> {
    sharer(viewer)?;
    let page = InvitationRepo::list_pending(db, tree_id, Utc::now(), params).await?;
    let cursors: Vec<String> = page.edges.iter().map(|e| e.cursor.clone()).collect();
    let invitations: Vec<TreeInvitation> = page.edges.into_iter().map(|e| e.node).collect();
    let views = invitation_views(db, env, invitations).await?;
    Ok(Connection {
        edges: views
            .into_iter()
            .zip(cursors)
            .map(|(node, cursor)| oxidgene_core::types::Edge { cursor, node })
            .collect(),
        page_info: page.page_info,
        total_count: page.total_count,
    })
}

/// The pending invitation `invitation_id` of `tree_id`; `not_found`
/// otherwise.
async fn pending_of_tree(
    conn: &impl ConnectionTrait,
    tree_id: Uuid,
    invitation_id: Uuid,
) -> Result<TreeInvitation, OxidGeneError> {
    InvitationRepo::get(conn, invitation_id)
        .await?
        .filter(|invitation| invitation.tree_id == tree_id && invitation.is_pending(Utc::now()))
        .ok_or(OxidGeneError::NotFound {
            entity: "Invitation",
            id: invitation_id,
        })
}

/// Send invitation `invitation_id` of `tree_id` again: it waits another 14
/// days, if that is later than it would have.
pub async fn resend_invitation(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    env: &SharingEnv,
    tree_id: Uuid,
    invitation_id: Uuid,
) -> Result<InvitationView, OxidGeneError> {
    let identity = sharer(viewer)?;
    // A resend that mails counts as a lookup, so that nobody makes the
    // relay mail an address without bound.
    if env.outbox.delivers() {
        count_lookup(identity.user_id)?;
    }
    let txn = begin_tx(db).await?;
    let current = pending_of_tree(&txn, tree_id, invitation_id).await?;
    let now = Utc::now();
    let expires_at = current
        .expires_at
        .max(now + chrono::Duration::days(INVITATION_DAYS));
    let invitation = InvitationRepo::resent(&txn, invitation_id, now, expires_at).await?;
    mail(&txn, viewer, env, &invitation).await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Update,
        AuditEntity::TreeInvitation,
        invitation_id,
    )
    .await?;
    commit_tx(txn).await?;
    env.outbox.committed();
    invitation_views(db, env, vec![invitation])
        .await
        .map(|mut views| views.remove(0))
}

/// Revoke invitation `invitation_id` of `tree_id`: it can no longer be
/// accepted.
pub async fn revoke_invitation(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    tree_id: Uuid,
    invitation_id: Uuid,
) -> Result<(), OxidGeneError> {
    sharer(viewer)?;
    let txn = begin_tx(db).await?;
    pending_of_tree(&txn, tree_id, invitation_id).await?;
    InvitationRepo::end(&txn, invitation_id, InvitationEnd::Revoked, Utc::now()).await?;
    record(
        &txn,
        viewer,
        tree_id,
        AuditAction::Delete,
        AuditEntity::TreeInvitation,
        invitation_id,
    )
    .await?;
    commit_tx(txn).await
}

// ── The invited user's side ─────────────────────────────────────────────

/// The verified address of the signed-in user, lower-cased: none for one
/// whose provider did not verify it, who accepts nothing.
async fn verified_email(
    conn: &impl ConnectionTrait,
    viewer: &ViewerContext,
) -> Result<(Uuid, Option<String>), OxidGeneError> {
    let identity = crate::service::account::require_user(viewer.principal())?;
    let user = AppUserRepo::get(conn, identity.user_id).await?;
    let email = user
        .email
        .filter(|_| user.email_verified)
        .map(|email| email.trim().to_lowercase());
    Ok((user.id, email))
}

/// The pending invitations of the signed-in user's verified address.
pub async fn my_invitations(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
) -> Result<Vec<MyInvitation>, OxidGeneError> {
    let (_, email) = verified_email(db, viewer).await?;
    let Some(email) = email else {
        return Ok(Vec::new());
    };
    let invitations = InvitationRepo::pending_of_email(db, &email, Utc::now()).await?;
    let mut trees = HashMap::new();
    for tree_id in invitations.iter().map(|i| i.tree_id) {
        if let std::collections::hash_map::Entry::Vacant(entry) = trees.entry(tree_id) {
            entry.insert(TreeRepo::get(db, tree_id).await?.name);
        }
    }
    let inviters: Vec<Uuid> = invitations.iter().filter_map(|i| i.created_by).collect();
    let names = AppUserRepo::shown_names(db, &inviters).await?;
    Ok(invitations
        .into_iter()
        .map(|invitation| MyInvitation {
            tree_name: trees.get(&invitation.tree_id).cloned().unwrap_or_default(),
            inviter_name: invitation
                .created_by
                .and_then(|id| names.get(&id).cloned().flatten()),
            scoped: invitation.scope_id.is_some(),
            id: invitation.id,
            tree_id: invitation.tree_id,
            role: invitation.role,
            membership_expires_at: invitation.membership_expires_at,
            expires_at: invitation.expires_at,
            created_at: invitation.created_at,
        })
        .collect())
}

/// The pending invitation `invitation_id` addressed to the signed-in
/// user's verified address; `not_found` for anyone else, whoever holds the
/// link.
async fn my_pending(
    conn: &impl ConnectionTrait,
    viewer: &ViewerContext,
    invitation_id: Uuid,
) -> Result<(Uuid, TreeInvitation), OxidGeneError> {
    let (user_id, email) = verified_email(conn, viewer).await?;
    let invitation = InvitationRepo::get(conn, invitation_id)
        .await?
        .filter(|invitation| {
            invitation.is_pending(Utc::now()) && email.as_deref() == Some(&invitation.email)
        })
        .ok_or(OxidGeneError::NotFound {
            entity: "Invitation",
            id: invitation_id,
        })?;
    TreeRepo::get(conn, invitation.tree_id).await?;
    Ok((user_id, invitation))
}

/// Accept invitation `invitation_id`: it becomes the signed-in user's
/// membership, with its role, scope and expiry, and the tree's owner is
/// told (`invitation_accepted`). Someone already a member is a `conflict`.
pub async fn accept_invitation(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    mailing: Mailing,
    invitation_id: Uuid,
) -> Result<MemberView, OxidGeneError> {
    let txn = begin_tx(db).await?;
    let (user_id, invitation) = my_pending(&txn, viewer, invitation_id).await?;
    if MembershipRepo::get(&txn, invitation.tree_id, user_id)
        .await?
        .is_some()
    {
        return Err(OxidGeneError::Conflict(
            "the user already holds a role on the tree".into(),
        ));
    }
    let membership = MembershipRepo::create_shared(
        &txn,
        SharedMembership {
            tree_id: invitation.tree_id,
            user_id,
            role: shared_role(invitation.role)?,
            scope_id: invitation.scope_id,
            expires_at: invitation.membership_expires_at,
            created_by: invitation.created_by,
        },
    )
    .await?;
    InvitationRepo::end(
        &txn,
        invitation_id,
        InvitationEnd::Accepted(user_id),
        Utc::now(),
    )
    .await?;
    record(
        &txn,
        viewer,
        invitation.tree_id,
        AuditAction::Create,
        AuditEntity::Membership,
        membership.id,
    )
    .await?;
    let mut notices = Notices::new(mailing);
    if let Some(owner) = MembershipRepo::owner(&txn, invitation.tree_id).await? {
        notices
            .add(
                &txn,
                NotificationKind::InvitationAccepted,
                owner.user_id,
                About::tree(invitation.tree_id, viewer),
            )
            .await?;
    }
    commit_tx(txn).await?;
    notices.publish();
    viewer.forget(invitation.tree_id);
    let mut event = authz::log::event(viewer, "invitation_accepted", "ok");
    event.tree_id = Some(invitation.tree_id);
    authz::log::record(db, event);
    let views = member_views(db, vec![membership]).await?;
    views.into_iter().next().ok_or(OxidGeneError::NotFound {
        entity: "Membership",
        id: user_id,
    })
}

/// Decline invitation `invitation_id`.
pub async fn decline_invitation(
    viewer: &ViewerContext,
    db: &DatabaseConnection,
    invitation_id: Uuid,
) -> Result<(), OxidGeneError> {
    let txn = begin_tx(db).await?;
    let (_, invitation) = my_pending(&txn, viewer, invitation_id).await?;
    InvitationRepo::end(&txn, invitation_id, InvitationEnd::Declined, Utc::now()).await?;
    record(
        &txn,
        viewer,
        invitation.tree_id,
        AuditAction::Update,
        AuditEntity::TreeInvitation,
        invitation_id,
    )
    .await?;
    commit_tx(txn).await
}

/// Audit a sharing change under the tree's `access` category.
pub(crate) async fn record(
    db: &impl ConnectionTrait,
    viewer: &ViewerContext,
    tree_id: Uuid,
    action: AuditAction,
    entity: AuditEntity,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    Change::new(viewer, tree_id, action, entity, id)
        .about_tree()
        .record_unversioned(db)
        .await
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_normalized_and_checked() {
        assert_eq!(
            normalized_email("  Fictive.Person@Example.INVALID ").unwrap(),
            "fictive.person@example.invalid"
        );
        for refused in [
            "",
            "no-at-sign",
            "@example.invalid",
            "a@nodot",
            "a b@example.invalid",
        ] {
            assert!(normalized_email(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn an_invitation_waits_fourteen_days_and_never_more_than_ninety() {
        let now = Utc::now();
        assert_eq!(
            invitation_expiry(now, None).unwrap(),
            now + chrono::Duration::days(14)
        );
        let asked = now + chrono::Duration::days(30);
        assert_eq!(invitation_expiry(now, Some(asked)).unwrap(), asked);
        assert!(invitation_expiry(now, Some(now + chrono::Duration::days(91))).is_err());
    }

    #[test]
    fn lookups_are_limited_per_user_and_hour() {
        let user = Uuid::now_v7();
        for _ in 0..LOOKUPS_PER_HOUR {
            count_lookup(user).unwrap();
        }
        match count_lookup(user) {
            Err(OxidGeneError::RateLimited { retry_after }) => {
                assert!(retry_after > 0 && retry_after <= 3600);
            }
            other => panic!("{other:?}"),
        }
        count_lookup(Uuid::now_v7()).expect("another user is not limited");
    }

    #[test]
    fn only_editors_and_guests_are_shared() {
        assert!(shared_role(TreeRole::Editor).is_ok());
        assert!(shared_role(TreeRole::Guest).is_ok());
        assert!(shared_role(TreeRole::Owner).is_err());
        assert!(shared_role(TreeRole::Visitor).is_err());
    }

    #[test]
    fn the_lookup_counters_keep_only_the_last_hours_users() {
        let mut lookups = HashMap::new();
        let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
        let start = Instant::now();
        for _ in 0..LOOKUPS_PER_HOUR {
            count_lookup_at(&mut lookups, first, start).unwrap();
        }
        assert!(matches!(
            count_lookup_at(&mut lookups, first, start),
            Err(OxidGeneError::RateLimited { retry_after: 3600 })
        ));
        // An hour on, the first user's lookups and their entry are gone.
        let later = start + Duration::from_secs(3600);
        count_lookup_at(&mut lookups, second, later).unwrap();
        assert_eq!(lookups.keys().copied().collect::<Vec<_>>(), [second]);
        count_lookup_at(&mut lookups, first, later).unwrap();
    }
}
