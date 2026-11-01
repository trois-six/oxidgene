//! GraphQL twins of the account operations of the web build
//! (docs/authentication.md §6, docs/authorization.md §10): the signed-in
//! user's profile, sessions, account data and deletion, other users'
//! profiles, and the administrator's user management. The avatar image
//! stays a direct HTTP read, which `UserProfile.avatarUrl` addresses.
//!
//! Every resolver reads the request's [`Principal`], which the
//! authentication layer resolved, and calls the account service as REST
//! does.

use std::sync::Arc;

use async_graphql::{
    ComplexObject, Context, ID, InputObject, MaybeUndefined, Object, Result, SimpleObject,
};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use oxidgene_db::repo::PaginationParams;

use super::scope::{boxed, uuid, viewer_from_ctx};
use super::types::{db_from_ctx, media_from_ctx, reader_from_ctx};
use crate::auth::{Auth, Principal};
use crate::service::account::{
    self, AvatarInfo, IdentityRebind, ManagedUser, Me, ProfilePatch, SessionView, UserPatch,
    UserProfile,
};
use crate::service::avatar;

/// The request's principal: anonymous when none was resolved.
fn principal<'a>(ctx: &'a Context<'_>) -> &'a Principal {
    viewer_from_ctx(ctx).principal()
}

/// The server's authentication; `none` when the request carries none.
pub(crate) fn auth_from_ctx(ctx: &Context<'_>) -> Arc<Auth> {
    ctx.data_opt::<Arc<Auth>>()
        .cloned()
        .unwrap_or_else(|| Arc::new(Auth::none()))
}

/// Clear the session cookie: the request's own session ended.
fn sign_out(ctx: &Context<'_>) {
    ctx.append_http_header(
        "set-cookie",
        "__Host-oxidgene=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0",
    );
}

/// An uploaded avatar's version.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUserAvatar {
    pub etag: String,
    pub updated_at: DateTime<Utc>,
}

impl From<AvatarInfo> for GqlUserAvatar {
    fn from(avatar: AvatarInfo) -> Self {
        Self {
            etag: avatar.etag,
            updated_at: avatar.updated_at,
        }
    }
}

/// The signed-in user's own account: REST `GET /me`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMe {
    pub id: ID,
    pub display_name: Option<String>,
    pub provider_name: Option<String>,
    pub avatar: Option<GqlUserAvatar>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub locale: Option<String>,
    /// `admin` or `user`.
    pub instance_role: String,
    pub can_create_trees: bool,
}

impl From<Me> for GqlMe {
    fn from(me: Me) -> Self {
        Self {
            id: ID(me.id.to_string()),
            display_name: me.display_name,
            provider_name: me.provider_name,
            avatar: me.avatar.map(Into::into),
            email: me.email,
            email_verified: me.email_verified,
            locale: me.locale,
            instance_role: me.instance_role.to_string(),
            can_create_trees: me.can_create_trees,
        }
    }
}

/// Another user as their viewers see them: REST `GET /users/{user_id}`.
#[derive(Debug, Clone, SimpleObject)]
#[graphql(complex)]
pub struct GqlUserProfile {
    pub id: ID,
    pub display_name: Option<String>,
    pub avatar: Option<GqlUserAvatar>,
}

#[ComplexObject]
impl GqlUserProfile {
    /// The address of the uploaded avatar's image at `size` (256 or 64),
    /// to fetch with the client's credentials; `null` with the generated
    /// avatar. Its `v` changes with the avatar.
    async fn avatar_url(&self, #[graphql(default = 256)] size: i32) -> Option<String> {
        self.avatar.as_ref().map(|avatar| {
            format!(
                "/api/v1/users/{}/avatar?size={size}&v={}",
                self.id.as_str(),
                avatar.etag
            )
        })
    }
}

impl From<UserProfile> for GqlUserProfile {
    fn from(profile: UserProfile) -> Self {
        Self {
            id: ID(profile.id.to_string()),
            display_name: profile.display_name,
            avatar: profile.avatar.map(Into::into),
        }
    }
}

/// A user as an administrator manages them: REST `GET /admin/users`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUser {
    pub id: ID,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
    pub email_verified: bool,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub avatar: Option<GqlUserAvatar>,
    pub instance_role: String,
    pub can_create_trees: bool,
    /// `active`, `deactivated` or `erased`.
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub last_sign_in_at: DateTime<Utc>,
    pub deactivated_at: Option<DateTime<Utc>>,
    pub erased_at: Option<DateTime<Utc>>,
}

impl From<ManagedUser> for GqlUser {
    fn from(user: ManagedUser) -> Self {
        Self {
            id: ID(user.id.to_string()),
            display_name: user.display_name,
            username: user.username,
            email: user.email,
            email_verified: user.email_verified,
            issuer: user.issuer,
            subject: user.subject,
            avatar: user.avatar.map(Into::into),
            instance_role: user.instance_role.to_string(),
            can_create_trees: user.can_create_trees,
            status: user.status.to_string(),
            created_at: user.created_at,
            last_sign_in_at: user.last_sign_in_at,
            deactivated_at: user.deactivated_at,
            erased_at: user.erased_at,
        }
    }
}

connection!(GqlUserEdge, GqlUserConnection, GqlUser, ManagedUser);

/// One of the signed-in user's sessions: REST `GET /me/sessions`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUserSession {
    pub id: ID,
    pub created_at: DateTime<Utc>,
    pub last_used_at: DateTime<Utc>,
    pub client_id: String,
    pub browser: String,
    pub current: bool,
}

impl From<SessionView> for GqlUserSession {
    fn from(session: SessionView) -> Self {
        Self {
            id: ID(session.id.to_string()),
            created_at: session.created_at,
            last_used_at: session.last_used_at,
            client_id: session.client_id,
            browser: session.browser,
            current: session.current,
        }
    }
}

/// The account data export: REST `GET /me/export`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccountData {
    pub exported_at: DateTime<Utc>,
    pub user: GqlUser,
    pub display_name_set: Option<String>,
    pub provider_name: Option<String>,
    pub locale: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub memberships: Vec<GqlAccountMembership>,
    pub invitations: Vec<GqlAccountInvitation>,
    pub notification_preferences: Vec<GqlAccountPreference>,
    pub access_events: Vec<super::member::GqlAccessEvent>,
}

/// A notification preference the user changed, in their account data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccountPreference {
    /// A notification kind, or `all` for the switch of all email.
    pub kind: String,
    /// `off`, `immediate`, `daily`, or `on` for the switch.
    pub email: String,
}

/// An invitation addressed to the user, in their account data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccountInvitation {
    pub tree_id: ID,
    /// `editor` or `guest`.
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub declined_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// One of the user's memberships, in their account data.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccountMembership {
    pub tree_id: ID,
    /// `owner`, `editor` or `guest`.
    pub role: String,
    pub self_person_id: Option<ID>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// REST `PUT /me`: an absent field is kept, `null` returns to the
/// provider's value.
#[derive(Debug, InputObject)]
pub struct UpdateMeInput {
    pub display_name: MaybeUndefined<String>,
    pub locale: MaybeUndefined<String>,
}

fn patch<T>(value: MaybeUndefined<T>) -> Option<Option<T>> {
    match value {
        MaybeUndefined::Undefined => None,
        MaybeUndefined::Null => Some(None),
        MaybeUndefined::Value(value) => Some(Some(value)),
    }
}

/// REST `PUT /me/avatar`: the image and the crop square, in pixels of the
/// upright image; without the square, the centred largest one.
#[derive(Debug, InputObject)]
pub struct UploadAvatarInput {
    pub content_base64: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub size: Option<i32>,
}

fn non_negative(value: i32) -> Result<u32> {
    u32::try_from(value).map_err(|_| {
        oxidgene_core::OxidGeneError::Validation("the crop square lies outside the image".into())
            .into()
    })
}

/// REST `PUT /admin/users/{user_id}`.
#[derive(Debug, InputObject)]
pub struct UpdateUserInput {
    /// `admin` or `user`.
    pub instance_role: Option<String>,
    pub can_create_trees: Option<bool>,
    /// `active` or `deactivated`.
    pub status: Option<String>,
}

/// REST `PUT /admin/users/{user_id}/identity`.
#[derive(Debug, InputObject)]
pub struct RebindUserIdentityInput {
    pub issuer: String,
    pub subject: String,
}

/// The account queries.
#[derive(Default)]
pub struct AccountQuery;

#[Object]
impl AccountQuery {
    /// The signed-in user's own account.
    async fn me(&self, ctx: &Context<'_>) -> Result<GqlMe> {
        boxed(async move {
            let user = account::me(reader_from_ctx(ctx), principal(ctx)).await?;
            Ok(Me::from(&user).into())
        })
        .await
    }

    /// Everything OxidGene holds about the signed-in user.
    async fn my_account_data(&self, ctx: &Context<'_>) -> Result<GqlAccountData> {
        boxed(async move {
            let data = account::account_data(reader_from_ctx(ctx), principal(ctx)).await?;
            Ok(GqlAccountData {
                exported_at: data.exported_at,
                user: data.user.into(),
                display_name_set: data.display_name_set,
                provider_name: data.provider_name,
                locale: data.locale,
                updated_at: data.updated_at,
                memberships: data
                    .memberships
                    .into_iter()
                    .map(|membership| GqlAccountMembership {
                        tree_id: ID(membership.tree_id.to_string()),
                        role: membership.role.as_str().to_string(),
                        self_person_id: membership.self_person_id.map(|id| ID(id.to_string())),
                        expires_at: membership.expires_at,
                        created_at: membership.created_at,
                    })
                    .collect(),
                invitations: data
                    .invitations
                    .into_iter()
                    .map(|invitation| GqlAccountInvitation {
                        tree_id: ID(invitation.tree_id.to_string()),
                        role: invitation.role.as_str().to_string(),
                        created_at: invitation.created_at,
                        expires_at: invitation.expires_at,
                        accepted_at: invitation.accepted_at,
                        declined_at: invitation.declined_at,
                        revoked_at: invitation.revoked_at,
                    })
                    .collect(),
                notification_preferences: data
                    .notification_preferences
                    .into_iter()
                    .map(|preference| GqlAccountPreference {
                        kind: preference.kind,
                        email: preference.email,
                    })
                    .collect(),
                access_events: data.access_events.into_iter().map(Into::into).collect(),
            })
        })
        .await
    }

    /// The signed-in user's sessions, newest first.
    async fn my_sessions(&self, ctx: &Context<'_>) -> Result<Vec<GqlUserSession>> {
        boxed(async move {
            let sessions = account::my_sessions(&auth_from_ctx(ctx), principal(ctx)).await?;
            Ok(sessions.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// A user, for whoever may see them; `null` for anyone else.
    async fn user_profile(&self, ctx: &Context<'_>, user_id: ID) -> Result<Option<GqlUserProfile>> {
        boxed(async move {
            let id = uuid(&user_id)?;
            match account::visible_user(reader_from_ctx(ctx), principal(ctx), id).await {
                Ok(user) => Ok(Some(UserProfile::from(&user).into())),
                Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }

    /// Every user of the instance, for an administrator.
    async fn users(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlUserConnection> {
        boxed(async move {
            let params = PaginationParams {
                first: first.map_or(25, |first| u64::try_from(first).unwrap_or(0)),
                after,
            };
            let users = account::list_users(reader_from_ctx(ctx), principal(ctx), &params).await?;
            Ok(users.map(|user| ManagedUser::from(&user)).into())
        })
        .await
    }

    /// One user, for an administrator; `null` when there is none.
    async fn user(&self, ctx: &Context<'_>, user_id: ID) -> Result<Option<GqlUser>> {
        boxed(async move {
            let id = uuid(&user_id)?;
            match account::managed_user(reader_from_ctx(ctx), principal(ctx), id).await {
                Ok(user) => Ok(Some(ManagedUser::from(&user).into())),
                Err(oxidgene_core::OxidGeneError::NotFound { .. }) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
        .await
    }
}

/// The account mutations.
#[derive(Default)]
pub struct AccountMutation;

#[Object]
impl AccountMutation {
    /// Change the signed-in user's display name or language.
    async fn update_me(&self, ctx: &Context<'_>, input: UpdateMeInput) -> Result<GqlMe> {
        boxed(async move {
            let patch = ProfilePatch {
                display_name: patch(input.display_name),
                locale: patch(input.locale),
            };
            let user = account::update_profile(db_from_ctx(ctx), principal(ctx), patch).await?;
            Ok(Me::from(&user).into())
        })
        .await
    }

    /// Replace the signed-in user's avatar. Mirrors `PUT /me/avatar`, the
    /// image base64-encoded as for `uploadMediaFile`, within the GraphQL
    /// body limit.
    async fn upload_my_avatar(&self, ctx: &Context<'_>, input: UploadAvatarInput) -> Result<GqlMe> {
        boxed(async move {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&input.content_base64)
                .map_err(|_| {
                    oxidgene_core::OxidGeneError::Validation("contentBase64 is not base64".into())
                })?;
            let crop = match (input.x, input.y, input.size) {
                (Some(x), Some(y), Some(size)) => Some(avatar::Crop {
                    x: non_negative(x)?,
                    y: non_negative(y)?,
                    size: non_negative(size)?,
                }),
                (None, None, None) => None,
                _ => {
                    return Err(oxidgene_core::OxidGeneError::Validation(
                        "x, y and size go together".into(),
                    )
                    .into());
                }
            };
            let user = avatar::set_avatar(
                db_from_ctx(ctx),
                media_from_ctx(ctx),
                principal(ctx),
                bytes,
                crop,
            )
            .await?;
            Ok(Me::from(&user).into())
        })
        .await
    }

    /// Return the signed-in user to the generated avatar.
    async fn delete_my_avatar(&self, ctx: &Context<'_>) -> Result<bool> {
        boxed(async move {
            avatar::delete_avatar(db_from_ctx(ctx), media_from_ctx(ctx), principal(ctx)).await?;
            Ok(true)
        })
        .await
    }

    /// End one of the signed-in user's sessions; ending the request's own
    /// signs the browser out.
    async fn end_my_session(&self, ctx: &Context<'_>, session_id: ID) -> Result<bool> {
        boxed(async move {
            let id = uuid(&session_id)?;
            if account::end_my_session(&auth_from_ctx(ctx), principal(ctx), id).await? {
                sign_out(ctx);
            }
            Ok(true)
        })
        .await
    }

    /// Erase the signed-in user's account.
    async fn delete_my_account(&self, ctx: &Context<'_>) -> Result<bool> {
        boxed(async move {
            account::delete_my_account(
                db_from_ctx(ctx),
                &auth_from_ctx(ctx),
                media_from_ctx(ctx),
                principal(ctx),
            )
            .await?;
            sign_out(ctx);
            Ok(true)
        })
        .await
    }

    /// Change a user's instance role, tree creation or status.
    async fn update_user(
        &self,
        ctx: &Context<'_>,
        user_id: ID,
        input: UpdateUserInput,
    ) -> Result<GqlUser> {
        boxed(async move {
            let id = uuid(&user_id)?;
            let patch = UserPatch {
                instance_role: input.instance_role,
                can_create_trees: input.can_create_trees,
                status: input.status,
            };
            let user = account::update_user(
                db_from_ctx(ctx),
                &auth_from_ctx(ctx),
                principal(ctx),
                id,
                patch,
            )
            .await?;
            Ok(ManagedUser::from(&user).into())
        })
        .await
    }

    /// Bind a user to another identity at the provider.
    async fn rebind_user_identity(
        &self,
        ctx: &Context<'_>,
        user_id: ID,
        input: RebindUserIdentityInput,
    ) -> Result<GqlUser> {
        boxed(async move {
            let id = uuid(&user_id)?;
            let rebind = IdentityRebind {
                issuer: input.issuer,
                subject: input.subject,
            };
            let user = account::rebind_identity(
                db_from_ctx(ctx),
                &auth_from_ctx(ctx),
                principal(ctx),
                id,
                rebind,
            )
            .await?;
            Ok(ManagedUser::from(&user).into())
        })
        .await
    }

    /// Erase a user.
    async fn erase_user(&self, ctx: &Context<'_>, user_id: ID) -> Result<bool> {
        boxed(async move {
            let id = uuid(&user_id)?;
            account::erase_user(
                db_from_ctx(ctx),
                &auth_from_ctx(ctx),
                media_from_ctx(ctx),
                principal(ctx),
                id,
            )
            .await?;
            Ok(true)
        })
        .await
    }
}
