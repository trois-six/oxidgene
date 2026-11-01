//! Accounts of the web build (docs/authentication.md §6): users provisioned
//! from the identity provider's claims behind the signup gate, instance
//! roles from claims, the profile a user edits, deactivation, erasure, the
//! account data export, and the administrator's operations on users.
//!
//! A user is keyed by issuer and subject and never matched by email. The
//! surfaces call these functions with the request's [`Principal`]; nothing
//! here reads a token or a cookie.

use oxidgene_core::access::ViewerContext;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{
    AppUser, AppUserChanges, AppUserRepo, InstanceRole, MembershipRepo, NewAppUser,
    PaginationParams, UserStatus, db_err,
};
use sea_orm::{DatabaseConnection, TransactionTrait};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{Auth, Identity, Principal};
use crate::media::MediaStore;
use crate::reference::ReferenceLang;

/// Who gets an account at their first sign-in (`OXIDGENE_AUTH_SIGNUP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignupRule {
    /// An identity whose verified address has a pending invitation, and the
    /// bootstrap administrator.
    Invitation,
    /// An identity carrying one of the allowed values of the roles claim.
    Claim,
    /// Every identity the provider authenticates.
    Open,
}

/// Who may create trees (`OXIDGENE_AUTH_TREE_CREATION`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCreationRule {
    Everyone,
    /// The roles claim decides, at each sign-in.
    Claim,
}

/// The rules that turn claims into an account.
#[derive(Debug, Clone)]
pub struct SignupPolicy {
    pub signup: SignupRule,
    pub tree_creation: TreeCreationRule,
    /// Whether a roles claim is configured; when it is, it decides the
    /// administrator role at each sign-in.
    pub roles_claim: bool,
    pub admin_values: Vec<String>,
    pub creator_values: Vec<String>,
    pub allowed_values: Vec<String>,
    /// Treat `email` as verified when the provider sends no
    /// `email_verified`.
    pub trust_email: bool,
    pub bootstrap_admin_email: Option<String>,
}

impl SignupPolicy {
    fn carries(&self, roles: &[String], values: &[String]) -> bool {
        roles.iter().any(|role| values.contains(role))
    }
}

/// What the provider says about an identity, read from its ID token.
#[derive(Debug, Clone, Default)]
pub struct IdentityClaims {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub preferred_username: Option<String>,
    pub locale: Option<String>,
    /// The values of the roles claim; `None` when no roles claim is
    /// configured.
    pub roles: Option<Vec<String>>,
}

impl IdentityClaims {
    fn email_verified(&self, policy: &SignupPolicy) -> bool {
        self.email.is_some() && self.email_verified.unwrap_or(policy.trust_email)
    }

    fn provider_name(&self) -> Option<String> {
        clean(self.name.as_deref()).or_else(|| clean(self.preferred_username.as_deref()))
    }
}

/// A provider string kept only when it holds something printable, cut to a
/// sane length.
fn clean(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect(),
    )
}

/// An interface language of OxidGene for a BCP 47 tag (`fr-CA` → `fr`).
pub fn interface_language(tag: &str) -> Option<&'static str> {
    let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
    ReferenceLang::from_code(&primary).map(ReferenceLang::code)
}

/// Why a sign-in is refused: the `auth_error` the browser lands with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignInRefusal {
    /// The signup gate admits no new account for this identity.
    SignupClosed,
    /// An existing account no longer carries an allowed value.
    AccessDenied,
    AccountDeactivated,
}

impl SignInRefusal {
    pub fn code(self) -> &'static str {
        match self {
            Self::SignupClosed => "signup_closed",
            Self::AccessDenied => "access_denied",
            Self::AccountDeactivated => "account_deactivated",
        }
    }
}

/// The user a sign-in admits, and whether their instance role changed.
#[derive(Debug, Clone)]
pub struct Admission {
    pub user: AppUser,
    pub role_changed: bool,
}

/// Provision or update the user `claims` identify, behind the signup gate
/// (§6.2), with the instance role and tree creation the claims grant (§6.3).
pub async fn sign_in(
    db: &DatabaseConnection,
    policy: &SignupPolicy,
    claims: &IdentityClaims,
) -> Result<Result<Admission, SignInRefusal>, OxidGeneError> {
    let txn = db.begin().await.map_err(db_err)?;
    let existing = AppUserRepo::find_by_identity(&txn, &claims.issuer, &claims.subject).await?;
    let (bootstrap, invited) = gates(&txn, policy, claims, existing.is_none()).await?;
    let outcome = match existing {
        Some(user) if user.status == UserStatus::Deactivated => {
            Err(SignInRefusal::AccountDeactivated)
        }
        Some(user) => {
            let roles = claims.roles.as_deref().unwrap_or_default();
            if policy.signup == SignupRule::Claim
                && !bootstrap
                && !policy.carries(roles, &policy.allowed_values)
            {
                Err(SignInRefusal::AccessDenied)
            } else {
                let mut changes = claim_changes(policy, claims, Some(&user));
                if bootstrap {
                    changes.instance_role = Some(InstanceRole::Admin);
                }
                changes.last_sign_in_at = Some(Utc::now());
                let updated = AppUserRepo::update(&txn, user.id, changes).await?;
                Ok(Admission {
                    role_changed: updated.instance_role != user.instance_role,
                    user: updated,
                })
            }
        }
        None if admitted(policy, claims, bootstrap, invited) => {
            let changes = claim_changes(policy, claims, None);
            let created = AppUserRepo::create(
                &txn,
                NewAppUser {
                    issuer: claims.issuer.clone(),
                    subject: claims.subject.clone(),
                    email: claims.email.clone(),
                    email_verified: claims.email_verified(policy),
                    provider_name: claims.provider_name(),
                    username: clean(claims.preferred_username.as_deref()),
                    locale: claims
                        .locale
                        .as_deref()
                        .and_then(interface_language)
                        .map(str::to_string),
                    instance_role: if bootstrap {
                        InstanceRole::Admin
                    } else {
                        changes.instance_role.unwrap_or(InstanceRole::User)
                    },
                    can_create_trees: changes.can_create_trees.unwrap_or(true),
                },
            )
            .await?;
            Ok(Admission {
                user: created,
                role_changed: false,
            })
        }
        None => Err(SignInRefusal::SignupClosed),
    };
    txn.commit().await.map_err(db_err)?;
    Ok(outcome)
}

/// Whether the gate admits a new identity: the bootstrap administrator, an
/// invited address, and whom the rule lets in.
fn admitted(
    policy: &SignupPolicy,
    claims: &IdentityClaims,
    bootstrap: bool,
    invited: bool,
) -> bool {
    bootstrap
        || invited
        || match policy.signup {
            SignupRule::Open => true,
            SignupRule::Claim => policy.carries(
                claims.roles.as_deref().unwrap_or_default(),
                &policy.allowed_values,
            ),
            SignupRule::Invitation => false,
        }
}

/// Whether the sign-in makes the bootstrap administrator, and whether a new
/// identity is invited.
async fn gates(
    db: &impl sea_orm::ConnectionTrait,
    policy: &SignupPolicy,
    claims: &IdentityClaims,
    new: bool,
) -> Result<(bool, bool), OxidGeneError> {
    Ok((
        bootstraps(db, policy, claims).await?,
        invited(db, policy, claims, new).await?,
    ))
}

/// Whether a new identity's verified address has a pending invitation to
/// a live tree: what the `invitation` signup gate admits beside the bootstrap
/// administrator (§6.2, docs/authorization.md §6.2).
async fn invited(
    db: &impl sea_orm::ConnectionTrait,
    policy: &SignupPolicy,
    claims: &IdentityClaims,
    new: bool,
) -> Result<bool, OxidGeneError> {
    let Some(email) = claims.email.as_deref().filter(|_| new) else {
        return Ok(false);
    };
    if policy.signup != SignupRule::Invitation || !claims.email_verified(policy) {
        return Ok(false);
    }
    oxidgene_db::repo::InvitationRepo::has_pending(db, &email.trim().to_lowercase(), Utc::now())
        .await
}

/// Whether this sign-in makes the bootstrap administrator: no active
/// administrator exists and the verified email is the configured one.
async fn bootstraps(
    db: &impl sea_orm::ConnectionTrait,
    policy: &SignupPolicy,
    claims: &IdentityClaims,
) -> Result<bool, OxidGeneError> {
    let (Some(expected), Some(email)) = (&policy.bootstrap_admin_email, &claims.email) else {
        return Ok(false);
    };
    if !claims.email_verified(policy) || !expected.trim().eq_ignore_ascii_case(email.trim()) {
        return Ok(false);
    }
    Ok(!AppUserRepo::exists_admin(db).await?)
}

/// The changes a set of claims makes to `user` (none yet for a new one):
/// the provider's name, username, email and locale, and the role and tree
/// creation the roles claim decides. A display name the user set is never
/// touched; a locale they chose is kept.
fn claim_changes(
    policy: &SignupPolicy,
    claims: &IdentityClaims,
    user: Option<&AppUser>,
) -> AppUserChanges {
    let roles = claims.roles.as_deref();
    let instance_role = (policy.roles_claim).then(|| {
        if policy.carries(roles.unwrap_or_default(), &policy.admin_values) {
            InstanceRole::Admin
        } else {
            InstanceRole::User
        }
    });
    let can_create_trees = match policy.tree_creation {
        TreeCreationRule::Claim => {
            Some(policy.carries(roles.unwrap_or_default(), &policy.creator_values))
        }
        TreeCreationRule::Everyone => user.is_none().then_some(true),
    };
    let locale = claims
        .locale
        .as_deref()
        .and_then(interface_language)
        .filter(|_| user.is_some_and(|user| user.locale.is_none()))
        .map(|locale| Some(locale.to_string()));
    AppUserChanges {
        email: Some(claims.email.clone()),
        email_verified: Some(claims.email_verified(policy)),
        provider_name: Some(claims.provider_name()),
        username: Some(clean(claims.preferred_username.as_deref())),
        locale,
        instance_role,
        can_create_trees,
        ..AppUserChanges::default()
    }
}

/// Refresh the user's claims from the ID token a token refresh returned.
/// The role the claims decide applies as at sign-in; whether it changed is
/// returned, since a role change ends the user's sessions.
pub async fn refresh_claims(
    db: &DatabaseConnection,
    policy: &SignupPolicy,
    user: &AppUser,
    claims: &IdentityClaims,
) -> Result<Admission, OxidGeneError> {
    let updated =
        AppUserRepo::update(db, user.id, claim_changes(policy, claims, Some(user))).await?;
    Ok(Admission {
        role_changed: updated.instance_role != user.instance_role,
        user: updated,
    })
}

// ── Views ──────────────────────────────────────────────────────────────

/// An uploaded avatar's version: the frontend fetches it by URL and caches
/// it by this `etag`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AvatarInfo {
    pub etag: String,
    pub updated_at: DateTime<Utc>,
}

fn avatar_of(user: &AppUser) -> Option<AvatarInfo> {
    Some(AvatarInfo {
        etag: user.avatar_sha256.clone()?,
        updated_at: user.avatar_updated_at.unwrap_or(user.updated_at),
    })
}

/// A user as those who may see them see them: `GET /users/{user_id}`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UserProfile {
    pub id: Uuid,
    /// The name the user set, else the provider's; `null` for an erased
    /// user, shown as a former member.
    pub display_name: Option<String>,
    pub avatar: Option<AvatarInfo>,
}

impl From<&AppUser> for UserProfile {
    fn from(user: &AppUser) -> Self {
        Self {
            id: user.id,
            display_name: user.shown_name().map(str::to_string),
            avatar: avatar_of(user),
        }
    }
}

/// The signed-in user's own account: `GET /me`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Me {
    pub id: Uuid,
    pub display_name: Option<String>,
    /// The provider's name, which returns when the user clears their own.
    pub provider_name: Option<String>,
    pub avatar: Option<AvatarInfo>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub locale: Option<String>,
    pub instance_role: &'static str,
    pub can_create_trees: bool,
}

impl From<&AppUser> for Me {
    fn from(user: &AppUser) -> Self {
        Self {
            id: user.id,
            display_name: user.shown_name().map(str::to_string),
            provider_name: user.provider_name.clone(),
            avatar: avatar_of(user),
            email: user.email.clone(),
            email_verified: user.email_verified,
            locale: user.locale.clone(),
            instance_role: user.instance_role.as_str(),
            can_create_trees: user.can_create_trees,
        }
    }
}

/// A user as an administrator manages them: `GET /admin/users`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ManagedUser {
    pub id: Uuid,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub avatar: Option<AvatarInfo>,
    pub instance_role: &'static str,
    pub can_create_trees: bool,
    /// `active`, `deactivated` or `erased`.
    pub status: &'static str,
    pub created_at: DateTime<Utc>,
    pub last_sign_in_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
    pub erased_at: Option<DateTime<Utc>>,
}

impl From<&AppUser> for ManagedUser {
    fn from(user: &AppUser) -> Self {
        Self {
            id: user.id,
            display_name: user.shown_name().map(str::to_string),
            username: user.username.clone(),
            email: user.email.clone(),
            email_verified: user.email_verified,
            issuer: user.issuer.clone(),
            subject: user.subject.clone(),
            avatar: avatar_of(user),
            instance_role: user.instance_role.as_str(),
            can_create_trees: user.can_create_trees,
            status: user.status.as_str(),
            created_at: user.created_at,
            last_sign_in_at: user.last_sign_in_at,
            deactivated_at: user.deactivated_at,
            erased_at: user.erased_at,
        }
    }
}

/// Everything OxidGene holds about the user, as JSON (GDPR Art. 15 and 20):
/// `GET /me/export`. Invitations, API clients, notification preferences
/// and conversations join it as the phases that store them ship.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountData {
    pub exported_at: DateTime<Utc>,
    pub user: ManagedUser,
    pub display_name_set: Option<String>,
    pub provider_name: Option<String>,
    pub locale: Option<String>,
    pub updated_at: DateTime<Utc>,
    /// The trees the user holds a role on.
    pub memberships: Vec<AccountMembership>,
    /// The invitations addressed to the user's address, whatever their
    /// state.
    pub invitations: Vec<AccountInvitation>,
    /// The access log's events about the user, oldest first.
    pub access_events: Vec<oxidgene_db::repo::AccessEvent>,
}

/// An invitation addressed to the user, in their account data.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountInvitation {
    pub tree_id: Uuid,
    pub role: oxidgene_core::access::TreeRole,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub declined_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One of the user's memberships, in their account data.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountMembership {
    pub tree_id: Uuid,
    pub role: oxidgene_core::access::TreeRole,
    pub self_person_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ── Who may see whom ───────────────────────────────────────────────────

/// The signed-in user `principal` names: `401` for an anonymous viewer, and
/// `404` in the `none` mode, whose local owner has no account.
pub fn require_user(principal: &Principal) -> Result<&Identity, OxidGeneError> {
    match principal {
        Principal::User(identity) => Ok(identity),
        Principal::Anonymous => Err(OxidGeneError::Unauthenticated),
        Principal::Local => Err(no_account()),
    }
}

/// Refuse anyone but an instance administrator. The `none` mode has no
/// accounts to administer: `404`, as on the desktop.
pub fn require_admin(principal: &Principal) -> Result<(), OxidGeneError> {
    match principal {
        Principal::Local => Err(no_account()),
        Principal::User(identity) if identity.instance_role == InstanceRole::Admin => Ok(()),
        Principal::User(_) => Err(OxidGeneError::Forbidden),
        Principal::Anonymous => Err(OxidGeneError::Unauthenticated),
    }
}

fn no_account() -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "User",
        id: Uuid::nil(),
    }
}

fn user_not_found(id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound { entity: "User", id }
}

/// The user `id`, if `principal` may see them (§6.7): themselves, and
/// administrators. Those who share a tree or a conversation with them join
/// with memberships and messaging. To anyone else the user does not exist.
pub async fn visible_user(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
    id: Uuid,
) -> Result<AppUser, OxidGeneError> {
    let allowed = match principal {
        Principal::Local => return Err(no_account()),
        Principal::User(identity) => {
            identity.user_id == id || identity.instance_role == InstanceRole::Admin
        }
        Principal::Anonymous => return Err(OxidGeneError::Unauthenticated),
    };
    if !allowed {
        return Err(user_not_found(id));
    }
    AppUserRepo::get(db, id).await
}

/// The signed-in user's account.
pub async fn me(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
) -> Result<AppUser, OxidGeneError> {
    let identity = require_user(principal)?;
    AppUserRepo::get(db, identity.user_id).await
}

// ── The profile ────────────────────────────────────────────────────────

/// What `PUT /me` changes; an absent field is kept, `display_name: null`
/// returns to the provider's name and `locale: null` to the provider's.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilePatch {
    #[serde(default, deserialize_with = "crate::service::patch::double_option")]
    pub display_name: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::service::patch::double_option")]
    pub locale: Option<Option<String>>,
}

/// The longest display name, in characters.
pub const MAX_DISPLAY_NAME: usize = 80;

/// A display name as stored: trimmed, 1 to 80 characters, no control
/// character.
fn display_name(value: &str) -> Result<String, OxidGeneError> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > MAX_DISPLAY_NAME || trimmed.chars().any(char::is_control) {
        return Err(OxidGeneError::Validation(
            "display_name must be 1 to 80 characters without control characters".into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Change the signed-in user's display name or language.
pub async fn update_profile(
    db: &DatabaseConnection,
    principal: &Principal,
    patch: ProfilePatch,
) -> Result<AppUser, OxidGeneError> {
    let identity = require_user(principal)?;
    let display_name = patch
        .display_name
        .map(|name| name.as_deref().map(display_name).transpose())
        .transpose()?;
    let locale = patch
        .locale
        .map(|locale| {
            locale
                .as_deref()
                .map(|code| {
                    ReferenceLang::from_code(code)
                        .map(|lang| lang.code().to_string())
                        .ok_or_else(|| {
                            OxidGeneError::Validation(crate::reference::UNSUPPORTED_LANGUAGE.into())
                        })
                })
                .transpose()
        })
        .transpose()?;
    AppUserRepo::update(
        db,
        identity.user_id,
        AppUserChanges {
            display_name,
            locale,
            ..AppUserChanges::default()
        },
    )
    .await
}

/// The signed-in user's account data.
pub async fn account_data(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
) -> Result<AccountData, OxidGeneError> {
    let user = me(db, principal).await?;
    let memberships = MembershipRepo::of_user(db, user.id)
        .await?
        .into_iter()
        .map(|membership| AccountMembership {
            tree_id: membership.tree_id,
            role: membership.role,
            self_person_id: membership.self_person_id,
            expires_at: membership.expires_at,
            created_at: membership.created_at,
        })
        .collect();
    let invitations = match user.email.as_deref() {
        Some(email) => {
            oxidgene_db::repo::InvitationRepo::of_email(db, &email.trim().to_lowercase())
                .await?
                .into_iter()
                .map(|invitation| AccountInvitation {
                    tree_id: invitation.tree_id,
                    role: invitation.role,
                    created_at: invitation.created_at,
                    expires_at: invitation.expires_at,
                    accepted_at: invitation.accepted_at,
                    declined_at: invitation.declined_at,
                    revoked_at: invitation.revoked_at,
                })
                .collect()
        }
        None => Vec::new(),
    };
    Ok(AccountData {
        exported_at: Utc::now(),
        user: ManagedUser::from(&user),
        display_name_set: user.display_name.clone(),
        provider_name: user.provider_name.clone(),
        locale: user.locale.clone(),
        updated_at: user.updated_at,
        memberships,
        invitations,
        access_events: oxidgene_db::repo::AccessEventRepo::of_user(db, user.id).await?,
    })
}

// ── Sessions ───────────────────────────────────────────────────────────

/// One of the signed-in user's sessions: `GET /me/sessions`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionView {
    /// An opaque handle, by which the session is ended.
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    /// The client the session signed in through.
    pub client_id: String,
    /// `firefox`, `chrome`, `edge`, `safari`, `opera` or `other`.
    pub browser: String,
    /// Whether it is the session of this request.
    pub current: bool,
}

/// The signed-in user's sessions, newest first.
pub async fn my_sessions(
    auth: &Auth,
    principal: &Principal,
) -> Result<Vec<SessionView>, OxidGeneError> {
    let identity = require_user(principal)?;
    let mut sessions: Vec<SessionView> = auth
        .user_sessions(identity.user_id)
        .await?
        .into_iter()
        .map(|session| SessionView {
            id: session.handle,
            created_at: at(session.created_at),
            last_used_at: at(session.last_used_at),
            current: identity.session_id.as_deref() == Some(session.id_hash.as_str()),
            client_id: session.client_id,
            browser: session.browser,
        })
        .collect();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
    Ok(sessions)
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap_or_default()
}

/// End one of the signed-in user's sessions; `true` when it was the
/// request's own, which signs the browser out.
pub async fn end_my_session(
    auth: &Auth,
    principal: &Principal,
    id: Uuid,
) -> Result<bool, OxidGeneError> {
    let identity = require_user(principal)?;
    let session = auth
        .user_sessions(identity.user_id)
        .await?
        .into_iter()
        .find(|session| session.handle == id)
        .ok_or(OxidGeneError::NotFound {
            entity: "Session",
            id,
        })?;
    auth.end_session(&session, "ended_by_user").await?;
    Ok(identity.session_id.as_deref() == Some(session.id_hash.as_str()))
}

// ── Administration ─────────────────────────────────────────────────────

/// Every user, by cursor.
pub async fn list_users(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
    params: &PaginationParams,
) -> Result<Connection<AppUser>, OxidGeneError> {
    require_admin(principal)?;
    AppUserRepo::list(db, params).await
}

/// One user, for an administrator.
pub async fn managed_user(
    db: &impl sea_orm::ConnectionTrait,
    principal: &Principal,
    id: Uuid,
) -> Result<AppUser, OxidGeneError> {
    require_admin(principal)?;
    AppUserRepo::get(db, id).await
}

/// What an administrator changes on a user: `PUT /admin/users/{user_id}`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserPatch {
    /// `admin` or `user`; refused while the roles claim decides it.
    #[serde(default)]
    pub instance_role: Option<String>,
    /// Refused while the roles claim decides it.
    #[serde(default)]
    pub can_create_trees: Option<bool>,
    /// `active` or `deactivated`.
    #[serde(default)]
    pub status: Option<String>,
}

/// Change a user's instance role, tree creation or status. Deactivating,
/// and changing the role, end the user's sessions at once.
pub async fn update_user(
    db: &DatabaseConnection,
    auth: &Auth,
    principal: &Principal,
    id: Uuid,
    patch: UserPatch,
) -> Result<AppUser, OxidGeneError> {
    require_admin(principal)?;
    let changes = admin_changes(auth.signup_policy(), &patch)?;
    let current = AppUserRepo::get(db, id).await?;
    if current.status == UserStatus::Erased {
        return Err(OxidGeneError::Conflict("the user is erased".into()));
    }
    let updated = AppUserRepo::update(db, id, changes).await?;
    let deactivated =
        updated.status == UserStatus::Deactivated && current.status != UserStatus::Deactivated;
    if deactivated || updated.instance_role != current.instance_role {
        let cause = if deactivated {
            "deactivated"
        } else {
            "role_changed"
        };
        auth.end_user_sessions(id, cause).await?;
    }
    let kind = match (current.status, updated.status) {
        (UserStatus::Active, UserStatus::Deactivated) => "user_deactivated",
        (UserStatus::Deactivated, UserStatus::Active) => "user_reactivated",
        _ => "user_updated",
    };
    access_event(db, principal, kind);
    Ok(updated)
}

/// Record an account act in the access log (docs/authorization.md §9.2):
/// who acted and through which client, never whom it named by name.
fn access_event(db: &DatabaseConnection, principal: &Principal, kind: &'static str) {
    let viewer = ViewerContext::new(principal.clone());
    crate::authz::log::record(db, crate::authz::log::event(&viewer, kind, "ok"));
}

/// The changes `patch` asks for, refusing those the roles claim decides.
fn admin_changes(
    policy: Option<&SignupPolicy>,
    patch: &UserPatch,
) -> Result<AppUserChanges, OxidGeneError> {
    let instance_role = match patch.instance_role.as_deref() {
        None => None,
        Some("admin") => Some(InstanceRole::Admin),
        Some("user") => Some(InstanceRole::User),
        Some(_) => return Err(invalid("instance_role must be `admin` or `user`")),
    };
    let status = match patch.status.as_deref() {
        None => None,
        Some("active") => Some(UserStatus::Active),
        Some("deactivated") => Some(UserStatus::Deactivated),
        Some(_) => return Err(invalid("status must be `active` or `deactivated`")),
    };
    let claims_decide_role = policy.is_some_and(|policy| policy.roles_claim);
    let claims_decide_creation =
        policy.is_some_and(|policy| policy.tree_creation == TreeCreationRule::Claim);
    if (instance_role.is_some() && claims_decide_role)
        || (patch.can_create_trees.is_some() && claims_decide_creation)
    {
        return Err(invalid("the identity provider's roles claim decides it"));
    }
    Ok(AppUserChanges {
        instance_role,
        can_create_trees: patch.can_create_trees,
        status,
        ..AppUserChanges::default()
    })
}

fn invalid(message: &str) -> OxidGeneError {
    OxidGeneError::Validation(message.to_string())
}

/// A user's new identity at the provider: `PUT /admin/users/{user_id}/identity`.
#[derive(Debug, Clone, Deserialize)]
pub struct IdentityRebind {
    pub issuer: String,
    pub subject: String,
}

/// Bind a user to another identity, when the instance moves to another
/// provider (§6.4). Their sessions under the old identity end.
pub async fn rebind_identity(
    db: &DatabaseConnection,
    auth: &Auth,
    principal: &Principal,
    id: Uuid,
    rebind: IdentityRebind,
) -> Result<AppUser, OxidGeneError> {
    require_admin(principal)?;
    let issuer = rebind.issuer.trim().to_string();
    let subject = rebind.subject.trim().to_string();
    if issuer.is_empty() || subject.is_empty() {
        return Err(invalid("issuer and subject are required"));
    }
    let txn = db.begin().await.map_err(db_err)?;
    let current = AppUserRepo::get(&txn, id).await?;
    if current.status == UserStatus::Erased {
        return Err(OxidGeneError::Conflict("the user is erased".into()));
    }
    if let Some(holder) = AppUserRepo::find_by_identity(&txn, &issuer, &subject).await?
        && holder.id != id
    {
        return Err(OxidGeneError::Conflict(
            "another user holds this identity".into(),
        ));
    }
    let updated = AppUserRepo::update(
        &txn,
        id,
        AppUserChanges {
            issuer: Some(issuer),
            subject: Some(subject),
            ..AppUserChanges::default()
        },
    )
    .await?;
    txn.commit().await.map_err(db_err)?;
    auth.end_user_sessions(id, "identity_rebound").await?;
    access_event(db, principal, "user_identity_rebound");
    Ok(updated)
}

/// Erase the signed-in user's account (GDPR Art. 17).
pub async fn delete_my_account(
    db: &DatabaseConnection,
    auth: &Auth,
    media: &Arc<dyn MediaStore>,
    principal: &Principal,
) -> Result<(), OxidGeneError> {
    let identity = require_user(principal)?;
    erase(db, auth, media, identity.user_id).await?;
    access_event(db, principal, "account_erased");
    Ok(())
}

/// Erase a user, as an administrator.
pub async fn erase_user(
    db: &DatabaseConnection,
    auth: &Auth,
    media: &Arc<dyn MediaStore>,
    principal: &Principal,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    require_admin(principal)?;
    erase(db, auth, media, id).await?;
    access_event(db, principal, "user_erased");
    Ok(())
}

/// The user `id`, unless they cannot be erased: erased already (`404`), or
/// owning a tree (`409`), which they transfer or delete first.
async fn erasable(db: &DatabaseConnection, id: Uuid) -> Result<AppUser, OxidGeneError> {
    let user = AppUserRepo::get(db, id).await?;
    if user.status == UserStatus::Erased {
        return Err(user_not_found(id));
    }
    if !MembershipRepo::owned_trees(db, id).await?.is_empty() {
        return Err(OxidGeneError::Conflict(
            "the user owns a tree: transfer or delete it first".into(),
        ));
    }
    Ok(user)
}

/// End the user's memberships and clear what identifies them, in one
/// transaction.
async fn tombstone(
    db: &DatabaseConnection,
    id: Uuid,
    email: Option<&str>,
) -> Result<(), OxidGeneError> {
    let txn = db.begin().await.map_err(db_err)?;
    MembershipRepo::delete_of_user(&txn, id).await?;
    // The invitations addressed to them go; those they sent and the share
    // links they made stay the tree's, without their author.
    let email = email.map(|email| email.trim().to_lowercase());
    oxidgene_db::repo::InvitationRepo::forget_user(&txn, id, email.as_deref()).await?;
    oxidgene_db::repo::ShareLinkRepo::forget_user(&txn, id).await?;
    AppUserRepo::erase(&txn, id).await?;
    txn.commit().await.map_err(db_err)
}

/// Turn the user into a tombstone (§6.6): refused (`409`) while they own a
/// tree, which they transfer or delete first; then their sessions end,
/// their memberships and the tombstone are written in one transaction, and
/// their avatar's files go once that commits. What later phases store about
/// a user — notifications, API clients, messages — is deleted here as each
/// ships.
async fn erase(
    db: &DatabaseConnection,
    auth: &Auth,
    media: &Arc<dyn MediaStore>,
    id: Uuid,
) -> Result<(), OxidGeneError> {
    let user = erasable(db, id).await?;
    auth.end_user_sessions(id, "erased").await?;
    // Only a verified address is theirs to clear.
    let email = user.email.as_deref().filter(|_| user.email_verified);
    tombstone(db, id, email).await?;
    if let Some(sha256) = user.avatar_sha256 {
        super::avatar::delete_files(media.as_ref(), id, &sha256).await;
    }
    tracing::info!(category = "user_erased", "A user was erased");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SignupPolicy {
        SignupPolicy {
            signup: SignupRule::Open,
            tree_creation: TreeCreationRule::Everyone,
            roles_claim: false,
            admin_values: vec!["oxidgene-admins".into()],
            creator_values: vec!["oxidgene-creators".into()],
            allowed_values: vec!["members".into()],
            trust_email: false,
            bootstrap_admin_email: None,
        }
    }

    #[test]
    fn display_names_are_trimmed_and_bounded() {
        assert_eq!(display_name("  Ada Fictive ").unwrap(), "Ada Fictive");
        assert_eq!(display_name(&"é".repeat(80)).unwrap().chars().count(), 80);
        for refused in ["", "   ", "line\nbreak", &"x".repeat(81)] {
            assert!(display_name(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn provider_locales_map_to_interface_languages() {
        assert_eq!(interface_language("fr-CA"), Some("fr"));
        assert_eq!(interface_language("PT_br"), Some("pt"));
        assert_eq!(interface_language("ja"), None);
    }

    #[test]
    fn email_is_verified_only_when_the_provider_or_the_operator_says_so() {
        let mut claims = IdentityClaims {
            email: Some("fixture@example.invalid".into()),
            ..IdentityClaims::default()
        };
        let mut policy = policy();
        assert!(!claims.email_verified(&policy));
        policy.trust_email = true;
        assert!(claims.email_verified(&policy));
        claims.email_verified = Some(false);
        assert!(!claims.email_verified(&policy), "an explicit no wins");
        claims.email = None;
        claims.email_verified = Some(true);
        assert!(
            !claims.email_verified(&policy),
            "no address, nothing verified"
        );
    }

    #[test]
    fn the_gate_admits_by_rule() {
        let mut policy = policy();
        let mut claims = IdentityClaims::default();
        assert!(admitted(&policy, &claims, false, false));
        policy.signup = SignupRule::Invitation;
        assert!(!admitted(&policy, &claims, false, false));
        assert!(
            admitted(&policy, &claims, true, false),
            "the bootstrap administrator"
        );
        assert!(
            admitted(&policy, &claims, false, true),
            "an invited address"
        );
        policy.signup = SignupRule::Claim;
        assert!(!admitted(&policy, &claims, false, false));
        claims.roles = Some(vec!["members".into()]);
        assert!(admitted(&policy, &claims, false, false));
    }

    #[test]
    fn the_roles_claim_decides_the_role_and_tree_creation() {
        let mut policy = policy();
        policy.roles_claim = true;
        policy.tree_creation = TreeCreationRule::Claim;
        let claims = IdentityClaims {
            roles: Some(vec!["oxidgene-admins".into()]),
            name: Some(" Ada ".into()),
            preferred_username: Some("ada".into()),
            ..IdentityClaims::default()
        };
        let changes = claim_changes(&policy, &claims, None);
        assert_eq!(changes.instance_role, Some(InstanceRole::Admin));
        assert_eq!(changes.can_create_trees, Some(false));
        assert_eq!(changes.provider_name, Some(Some("Ada".into())));
        let without = IdentityClaims::default();
        assert_eq!(
            claim_changes(&policy, &without, None).instance_role,
            Some(InstanceRole::User),
            "the claim's absence removes the role"
        );
        policy.roles_claim = false;
        policy.tree_creation = TreeCreationRule::Everyone;
        let changes = claim_changes(&policy, &claims, None);
        assert_eq!(changes.instance_role, None, "administrators grant it");
        assert_eq!(changes.can_create_trees, Some(true));
    }
}
