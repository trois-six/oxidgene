//! GraphQL twins of the memberships, ownership and access log operations
//! of the web build (docs/authorization.md §5, §9.2, §10), calling the same
//! services as REST with the request's viewer.

use async_graphql::{Context, ID, InputObject, MaybeUndefined, Object, Result, SimpleObject};
use chrono::{DateTime, Utc};
use oxidgene_core::access::TreeRole;
use oxidgene_db::repo::{AccessEvent, PaginationParams};

use super::scope::{boxed, live_tree, uuid, viewer_from_ctx};
use super::types::{db_from_ctx, reader_from_ctx};
use crate::service::access_log;
use crate::service::membership::{self, AdminTree, MemberPatch, MemberView};

fn id(uuid: uuid::Uuid) -> ID {
    ID(uuid.to_string())
}

fn params(first: Option<i32>, after: Option<String>) -> PaginationParams {
    PaginationParams {
        first: first.map_or(25, |first| u64::try_from(first).unwrap_or(0)),
        after,
    }
}

/// A member of a tree: REST `GET /trees/{tree_id}/members`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeMember {
    pub user_id: ID,
    pub display_name: Option<String>,
    /// `owner`, `editor` or `guest`.
    pub role: String,
    /// The part of the tree the member is limited to; none for the whole
    /// tree.
    pub scope: Option<super::sharing::GqlAccessScope>,
    pub self_person_id: Option<ID>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<MemberView> for GqlTreeMember {
    fn from(member: MemberView) -> Self {
        Self {
            user_id: id(member.user_id),
            display_name: member.display_name,
            role: member.role.as_str().to_string(),
            scope: member.scope.map(Into::into),
            self_person_id: member.self_person_id.map(id),
            expires_at: member.expires_at,
            created_at: member.created_at,
            updated_at: member.updated_at,
        }
    }
}

connection!(
    GqlTreeMemberEdge,
    GqlTreeMemberConnection,
    GqlTreeMember,
    MemberView
);

/// A tree as an administrator sees it: REST `GET /admin/trees`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAdminTree {
    pub id: ID,
    pub name: String,
    pub owner_id: Option<ID>,
    pub owner_name: Option<String>,
    pub person_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<AdminTree> for GqlAdminTree {
    fn from(tree: AdminTree) -> Self {
        Self {
            id: id(tree.id),
            name: tree.name,
            owner_id: tree.owner_id.map(id),
            owner_name: tree.owner_name,
            person_count: tree.person_count,
            created_at: tree.created_at,
            updated_at: tree.updated_at,
        }
    }
}

connection!(
    GqlAdminTreeEdge,
    GqlAdminTreeConnection,
    GqlAdminTree,
    AdminTree
);

/// One event of the access log: REST `GET /trees/{tree_id}/access-log`
/// and `GET /admin/access-log`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAccessEvent {
    pub id: ID,
    pub occurred_at: DateTime<Utc>,
    pub kind: String,
    pub principal_kind: String,
    pub user_id: Option<ID>,
    pub api_client_id: Option<ID>,
    pub share_link_id: Option<ID>,
    pub client_id: Option<String>,
    pub tree_id: Option<ID>,
    pub route: Option<String>,
    pub permission: Option<String>,
    pub outcome: String,
    pub request_id: Option<String>,
}

impl From<AccessEvent> for GqlAccessEvent {
    fn from(event: AccessEvent) -> Self {
        Self {
            id: id(event.id),
            occurred_at: event.occurred_at,
            kind: event.kind,
            principal_kind: event.principal_kind,
            user_id: event.user_id.map(id),
            api_client_id: event.api_client_id.map(id),
            share_link_id: event.share_link_id.map(id),
            client_id: event.client_id,
            tree_id: event.tree_id.map(id),
            route: event.route,
            permission: event.permission,
            outcome: event.outcome,
            request_id: event.request_id,
        }
    }
}

connection!(
    GqlAccessEventEdge,
    GqlAccessEventConnection,
    GqlAccessEvent,
    AccessEvent
);

/// REST `PUT /trees/{tree_id}/members/{user_id}`: `null` clears an optional
/// field, an absent one is kept.
#[derive(Debug, InputObject)]
pub struct UpdateTreeMemberInput {
    /// `editor` or `guest`.
    pub role: Option<String>,
    /// The part of the tree the member is limited to; `null` for the whole
    /// tree.
    pub scope: MaybeUndefined<super::inputs::AccessScopeInput>,
    pub expires_at: MaybeUndefined<DateTime<Utc>>,
    pub self_person_id: MaybeUndefined<ID>,
}

fn maybe<T>(value: MaybeUndefined<T>) -> Option<Option<T>> {
    match value {
        MaybeUndefined::Undefined => None,
        MaybeUndefined::Null => Some(None),
        MaybeUndefined::Value(value) => Some(Some(value)),
    }
}

impl UpdateTreeMemberInput {
    fn into_patch(self) -> Result<MemberPatch> {
        let role = self
            .role
            .map(|role| {
                TreeRole::parse(&role)
                    .ok_or_else(|| oxidgene_core::OxidGeneError::Validation("unknown role".into()))
            })
            .transpose()?;
        let self_person_id = match maybe(self.self_person_id) {
            Some(Some(person)) => Some(Some(uuid(&person)?)),
            Some(None) => Some(None),
            None => None,
        };
        let scope = match maybe(self.scope) {
            Some(Some(scope)) => Some(Some(scope.definition()?)),
            Some(None) => Some(None),
            None => None,
        };
        Ok(MemberPatch {
            role,
            scope,
            expires_at: maybe(self.expires_at),
            self_person_id,
        })
    }
}

/// The membership queries.
#[derive(Default)]
pub struct MemberQuery;

#[Object]
impl MemberQuery {
    /// The members of a tree: all of them for its owner, the viewer's own
    /// membership for anyone else.
    async fn tree_members(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlTreeMemberConnection> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let members = membership::list_members(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                tree_id,
                &params(first, after),
            )
            .await?;
            Ok(members.into())
        })
        .await
    }

    /// The access events of a tree, newest first, for its owner.
    async fn tree_access_events(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlAccessEventConnection> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let events = access_log::tree_events(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                tree_id,
                &params(first, after),
            )
            .await?;
            Ok(events.into())
        })
        .await
    }

    /// Every tree of the instance with its owner and size, for an
    /// administrator: metadata only.
    async fn admin_trees(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlAdminTreeConnection> {
        boxed(async move {
            let trees = membership::admin_trees(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                &params(first, after),
            )
            .await?;
            Ok(trees.into())
        })
        .await
    }

    /// The instance's access events, newest first, for an administrator.
    async fn access_events(
        &self,
        ctx: &Context<'_>,
        first: Option<i32>,
        after: Option<String>,
    ) -> Result<GqlAccessEventConnection> {
        boxed(async move {
            let events = access_log::instance_events(
                viewer_from_ctx(ctx),
                reader_from_ctx(ctx),
                &params(first, after),
            )
            .await?;
            Ok(events.into())
        })
        .await
    }
}

/// The membership mutations.
#[derive(Default)]
pub struct MemberMutation;

#[Object]
impl MemberMutation {
    /// Change a member's role or expiry (the owner), or one's own "Who am
    /// I?" person.
    async fn update_tree_member(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        user_id: ID,
        input: UpdateTreeMemberInput,
    ) -> Result<GqlTreeMember> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let member = membership::update_member(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&user_id)?,
                input.into_patch()?,
            )
            .await?;
            Ok(member.into())
        })
        .await
    }

    /// End a membership: the owner removes a member, a member leaves.
    async fn remove_tree_member(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        user_id: ID,
    ) -> Result<bool> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            membership::remove_member(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&user_id)?,
            )
            .await?;
            Ok(true)
        })
        .await
    }

    /// Hand the tree to one of its members, the owner becoming an editor.
    async fn transfer_tree_ownership(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        user_id: ID,
    ) -> Result<GqlTreeMember> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let owner = membership::transfer_ownership(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&user_id)?,
            )
            .await?;
            Ok(owner.into())
        })
        .await
    }

    /// Give an ownerless tree an owner: an administrator's act.
    async fn assign_tree_owner(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        user_id: ID,
    ) -> Result<GqlTreeMember> {
        boxed(async move {
            let owner = membership::assign_owner(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                uuid(&tree_id)?,
                uuid(&user_id)?,
            )
            .await?;
            Ok(owner.into())
        })
        .await
    }
}
