//! GraphQL twins of sharing (docs/authorization.md §6, §10): sharing with
//! users and invitations by email, share links and the preview of an
//! access scope, calling the same services as REST with the request's
//! viewer. Web build only, and absent from the `none` mode.

use async_graphql::{Context, Enum, ID, InputObject, MaybeUndefined, Object, Result, SimpleObject};
use chrono::{DateTime, Utc};
use oxidgene_core::OxidGeneError;
use oxidgene_core::access::TreeRole;
use oxidgene_db::repo::PaginationParams;

use super::account::auth_from_ctx;
use super::inputs::{AccessScopeInput, GqlScopeKind, definition};
use super::member::GqlTreeMember;
use super::scope::{boxed, live_tree, uuid, viewer_from_ctx};
use super::types::{db_from_ctx, profiles_from_ctx, reader_from_ctx};
use crate::service::access_scope::{self, ScopePreview, ScopePreviewRequest, ScopeView};
use crate::service::share_link::{
    self, CreatedShareLink, NewShareLinkRequest, ShareLinkPatch, ShareLinkView,
};
use crate::service::sharing::{
    self, Delivery, InvitationView, MyInvitation, ShareCandidate, ShareRequest,
};

fn id(uuid: uuid::Uuid) -> ID {
    ID(uuid.to_string())
}

fn params(first: Option<i32>, after: Option<String>) -> PaginationParams {
    PaginationParams {
        first: first.map_or(25, |first| u64::try_from(first).unwrap_or(0)),
        after,
    }
}

fn maybe<T>(value: MaybeUndefined<T>) -> Option<Option<T>> {
    match value {
        MaybeUndefined::Undefined => None,
        MaybeUndefined::Null => Some(None),
        MaybeUndefined::Value(value) => Some(Some(value)),
    }
}

/// The role a share grants, `editor` or `guest`.
pub(crate) fn shared_role(role: &str) -> Result<TreeRole> {
    TreeRole::parse(role).ok_or_else(|| OxidGeneError::Validation("unknown role".into()).into())
}

/// A scope as it is shown: REST's `scope` object.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccessScope {
    pub id: ID,
    pub kind: GqlScopeKind,
    pub person_id: ID,
    pub depth: Option<i32>,
    pub include_self: bool,
    pub include_spouses: bool,
    pub collateral: bool,
    pub person_name: Option<String>,
    pub person_deleted: bool,
}

impl From<ScopeView> for GqlAccessScope {
    fn from(view: ScopeView) -> Self {
        Self {
            id: id(view.id),
            kind: view.definition.kind.into(),
            person_id: id(view.definition.person_id),
            depth: view
                .definition
                .depth
                .and_then(|depth| i32::try_from(depth).ok()),
            include_self: view.definition.include_self,
            include_spouses: view.definition.include_spouses,
            collateral: view.definition.collateral,
            person_name: view.person_name,
            person_deleted: view.person_deleted,
        }
    }
}

/// How an invitation reaches its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlDelivery {
    Email,
    Manual,
}

impl From<Delivery> for GqlDelivery {
    fn from(delivery: Delivery) -> Self {
        match delivery {
            Delivery::Email => Self::Email,
            Delivery::Manual => Self::Manual,
        }
    }
}

/// A pending invitation: REST `GET /trees/{tree_id}/invitations`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeInvitation {
    pub id: ID,
    pub email: String,
    pub role: String,
    pub scope: Option<GqlAccessScope>,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub last_sent_at: Option<DateTime<Utc>>,
    pub link: String,
    pub delivery: GqlDelivery,
}

impl From<InvitationView> for GqlTreeInvitation {
    fn from(view: InvitationView) -> Self {
        Self {
            id: id(view.id),
            email: view.email,
            role: view.role.as_str().to_string(),
            scope: view.scope.map(Into::into),
            membership_expires_at: view.membership_expires_at,
            expires_at: view.expires_at,
            created_at: view.created_at,
            last_sent_at: view.last_sent_at,
            link: view.link,
            delivery: view.delivery.into(),
        }
    }
}

connection!(
    GqlTreeInvitationEdge,
    GqlTreeInvitationConnection,
    GqlTreeInvitation,
    InvitationView
);

/// An invitation the signed-in user may accept: REST `GET /me/invitations`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlMyInvitation {
    pub id: ID,
    pub tree_id: ID,
    pub tree_name: String,
    pub inviter_name: Option<String>,
    pub role: String,
    pub scoped: bool,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<MyInvitation> for GqlMyInvitation {
    fn from(view: MyInvitation) -> Self {
        Self {
            id: id(view.id),
            tree_id: id(view.tree_id),
            tree_name: view.tree_name,
            inviter_name: view.inviter_name,
            role: view.role.as_str().to_string(),
            scoped: view.scoped,
            membership_expires_at: view.membership_expires_at,
            expires_at: view.expires_at,
            created_at: view.created_at,
        }
    }
}

/// One of the people the owner already shares with.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlShareCandidate {
    pub user_id: ID,
    pub display_name: Option<String>,
}

impl From<ShareCandidate> for GqlShareCandidate {
    fn from(candidate: ShareCandidate) -> Self {
        Self {
            user_id: id(candidate.user_id),
            display_name: candidate.display_name,
        }
    }
}

/// What sharing made: REST `POST /trees/{tree_id}/members`, a membership
/// (`201`) or an invitation (`202`).
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlShareOutcome {
    pub member: Option<GqlTreeMember>,
    pub invitation: Option<GqlTreeInvitation>,
}

/// A share: REST `POST /trees/{tree_id}/members`.
#[derive(Debug, InputObject)]
pub struct ShareTreeInput {
    pub email: Option<String>,
    pub username: Option<String>,
    pub user_id: Option<ID>,
    /// `editor` or `guest`.
    pub role: String,
    pub scope: Option<AccessScopeInput>,
    pub expires_at: Option<DateTime<Utc>>,
    pub invitation_expires_at: Option<DateTime<Utc>>,
}

/// A share link: REST `GET /trees/{tree_id}/share-links`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlShareLink {
    pub id: ID,
    pub label: Option<String>,
    pub scope: Option<GqlAccessScope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub expired: bool,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

impl From<ShareLinkView> for GqlShareLink {
    fn from(view: ShareLinkView) -> Self {
        Self {
            id: id(view.id),
            label: view.label,
            scope: view.scope.map(Into::into),
            expires_at: view.expires_at,
            expired: view.expired,
            created_at: view.created_at,
            last_used_at: view.last_used_at,
            use_count: view.use_count,
        }
    }
}

connection!(
    GqlShareLinkEdge,
    GqlShareLinkConnection,
    GqlShareLink,
    ShareLinkView
);

/// A link just created, with its URL, shown once.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCreatedShareLink {
    pub link: GqlShareLink,
    pub url: String,
}

impl From<CreatedShareLink> for GqlCreatedShareLink {
    fn from(created: CreatedShareLink) -> Self {
        Self {
            link: created.link.into(),
            url: created.url,
        }
    }
}

/// A link to create: REST `POST /trees/{tree_id}/share-links`.
#[derive(Debug, InputObject)]
pub struct CreateShareLinkInput {
    pub label: Option<String>,
    pub scope: Option<AccessScopeInput>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// A link update: REST `PUT /trees/{tree_id}/share-links/{link_id}`;
/// `null` clears a field, an absent one is kept.
#[derive(Debug, InputObject)]
pub struct UpdateShareLinkInput {
    pub label: MaybeUndefined<String>,
    pub scope: MaybeUndefined<AccessScopeInput>,
    pub expires_at: MaybeUndefined<DateTime<Utc>>,
}

/// What a scope reaches: REST `POST /trees/{tree_id}/access-scopes/preview`.
#[derive(Debug, Clone, Copy, SimpleObject)]
pub struct GqlScopePreview {
    pub person_count: u64,
    pub visitor_count: u64,
    pub excluded_count: u64,
}

impl From<ScopePreview> for GqlScopePreview {
    fn from(preview: ScopePreview) -> Self {
        Self {
            person_count: preview.person_count,
            visitor_count: preview.visitor_count,
            excluded_count: preview.excluded_count,
        }
    }
}

/// The sharing queries.
#[derive(Default)]
pub struct SharingQuery;

#[Object]
impl SharingQuery {
    /// The pending invitations of the signed-in user's verified address.
    async fn my_invitations(&self, ctx: &Context<'_>) -> Result<Vec<GqlMyInvitation>> {
        boxed(async move {
            auth_from_ctx(ctx).sharing_env()?;
            let list = sharing::my_invitations(viewer_from_ctx(ctx), reader_from_ctx(ctx)).await?;
            Ok(list.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// The people the tree's owner already shares with, at most 50.
    async fn share_candidates(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Vec<GqlShareCandidate>> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            let list =
                sharing::share_candidates(viewer_from_ctx(ctx), reader_from_ctx(ctx), tree_id)
                    .await?;
            Ok(list.into_iter().map(Into::into).collect())
        })
        .await
    }

    /// The pending invitations of a tree, oldest first.
    async fn tree_invitations(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlTreeInvitationConnection> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let env = auth_from_ctx(ctx).sharing_env()?;
            let page = sharing::list_invitations(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                &env,
                tree_id,
                &params(first, after),
            )
            .await?;
            Ok(page.into())
        })
        .await
    }

    /// The share links of a tree not revoked, oldest first.
    async fn share_links(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlShareLinkConnection> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            let page =
                share_link::list(reader_from_ctx(ctx), tree_id, &params(first, after)).await?;
            Ok(page.into())
        })
        .await
    }

    /// The scope a tree's Visitors are limited to; `null` for the whole
    /// tree.
    async fn visitor_scope(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
    ) -> Result<Option<GqlAccessScope>> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            let scope = access_scope::visitor_scope(reader_from_ctx(ctx), tree_id).await?;
            Ok(scope.map(Into::into))
        })
        .await
    }

    /// How many persons a scope reaches — the whole tree without one — and
    /// how many of them a Visitor sees.
    async fn access_scope_preview(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        scope: Option<AccessScopeInput>,
    ) -> Result<GqlScopePreview> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            let request = ScopePreviewRequest {
                scope: definition(scope)?,
            };
            let preview =
                access_scope::preview(db_from_ctx(ctx), profiles_from_ctx(ctx), tree_id, &request)
                    .await?;
            Ok(preview.into())
        })
        .await
    }
}

/// The sharing mutations.
#[derive(Default)]
pub struct SharingMutation;

#[Object]
impl SharingMutation {
    /// Share a tree with a user, by exact email, exact username or among
    /// the people the owner already shares with; an email no account has
    /// becomes an invitation.
    async fn share_tree(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: ShareTreeInput,
    ) -> Result<GqlShareOutcome> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let env = auth_from_ctx(ctx).sharing_env()?;
            let request = ShareRequest {
                email: input.email,
                username: input.username,
                user_id: input.user_id.as_ref().map(uuid).transpose()?,
                role: shared_role(&input.role)?,
                scope: definition(input.scope)?,
                expires_at: input.expires_at,
                invitation_expires_at: input.invitation_expires_at,
            };
            let outcome = sharing::share(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                &env,
                tree_id,
                request,
            )
            .await?;
            Ok(GqlShareOutcome {
                member: outcome.member.map(Into::into),
                invitation: outcome.invitation.map(Into::into),
            })
        })
        .await
    }

    /// Send a pending invitation again.
    async fn resend_tree_invitation(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        invitation_id: ID,
    ) -> Result<GqlTreeInvitation> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let env = auth_from_ctx(ctx).sharing_env()?;
            let invitation = sharing::resend_invitation(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                &env,
                tree_id,
                uuid(&invitation_id)?,
            )
            .await?;
            Ok(invitation.into())
        })
        .await
    }

    /// Revoke a pending invitation; true once revoked.
    async fn revoke_tree_invitation(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        invitation_id: ID,
    ) -> Result<bool> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            sharing::revoke_invitation(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&invitation_id)?,
            )
            .await?;
            Ok(true)
        })
        .await
    }

    /// Accept an invitation addressed to the signed-in user's verified
    /// address: it becomes their membership.
    async fn accept_invitation(
        &self,
        ctx: &Context<'_>,
        invitation_id: ID,
    ) -> Result<GqlTreeMember> {
        boxed(async move {
            let env = auth_from_ctx(ctx).sharing_env()?;
            let member = sharing::accept_invitation(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                env.mailing(),
                uuid(&invitation_id)?,
            )
            .await?;
            Ok(member.into())
        })
        .await
    }

    /// Decline an invitation; true once declined.
    async fn decline_invitation(&self, ctx: &Context<'_>, invitation_id: ID) -> Result<bool> {
        boxed(async move {
            auth_from_ctx(ctx).sharing_env()?;
            sharing::decline_invitation(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                uuid(&invitation_id)?,
            )
            .await?;
            Ok(true)
        })
        .await
    }

    /// Create a share link; its URL is returned once.
    async fn create_share_link(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateShareLinkInput,
    ) -> Result<GqlCreatedShareLink> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let env = auth_from_ctx(ctx).sharing_env()?;
            let request = NewShareLinkRequest {
                label: input.label,
                scope: definition(input.scope)?,
                expires_at: input.expires_at,
            };
            let created = share_link::create(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                &env,
                tree_id,
                request,
            )
            .await?;
            Ok(created.into())
        })
        .await
    }

    /// Change a share link's label, scope or expiry.
    async fn update_share_link(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        link_id: ID,
        input: UpdateShareLinkInput,
    ) -> Result<GqlShareLink> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            let scope = match maybe(input.scope) {
                Some(Some(scope)) => Some(Some(scope.definition()?)),
                Some(None) => Some(None),
                None => None,
            };
            let patch = ShareLinkPatch {
                label: maybe(input.label),
                scope,
                expires_at: maybe(input.expires_at),
            };
            let link = share_link::update(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&link_id)?,
                patch,
            )
            .await?;
            Ok(link.into())
        })
        .await
    }

    /// Revoke a share link: every grant made from it ends; true once
    /// revoked.
    async fn revoke_share_link(&self, ctx: &Context<'_>, tree_id: ID, link_id: ID) -> Result<bool> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            auth_from_ctx(ctx).sharing_env()?;
            share_link::revoke(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&link_id)?,
            )
            .await?;
            Ok(true)
        })
        .await
    }
}
