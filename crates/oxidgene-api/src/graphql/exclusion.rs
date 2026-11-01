//! GraphQL twins of the exclusions from sharing (docs/privacy.md §6), in the
//! web build only: they call the exclusion service as REST does.

use async_graphql::{Context, Enum, ID, InputObject, Object, Result, SimpleObject};
use chrono::{DateTime, Utc};
use oxidgene_core::enums::ExclusionKind;

use super::scope::{boxed, live_tree, uuid, viewer_from_ctx};
use super::types::db_from_ctx;
use crate::service::exclusion::{self, ExclusionView, NewExclusion};

/// What an exclusion covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
pub enum GqlExclusionKind {
    Person,
    Ancestors,
    Descendants,
}

impl From<ExclusionKind> for GqlExclusionKind {
    fn from(kind: ExclusionKind) -> Self {
        match kind {
            ExclusionKind::Person => Self::Person,
            ExclusionKind::Ancestors => Self::Ancestors,
            ExclusionKind::Descendants => Self::Descendants,
        }
    }
}

impl From<GqlExclusionKind> for ExclusionKind {
    fn from(kind: GqlExclusionKind) -> Self {
        match kind {
            GqlExclusionKind::Person => Self::Person,
            GqlExclusionKind::Ancestors => Self::Ancestors,
            GqlExclusionKind::Descendants => Self::Descendants,
        }
    }
}

/// An exclusion from sharing, with its root and the persons it covers:
/// REST `GET /trees/{tree_id}/exclusions`.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlExclusion {
    pub id: ID,
    pub kind: GqlExclusionKind,
    pub person_id: ID,
    pub depth: Option<i32>,
    pub include_root: bool,
    pub include_spouses: bool,
    pub created_by: Option<ID>,
    pub created_at: DateTime<Utc>,
    pub person_name: Option<String>,
    pub person_deleted: bool,
    pub person_count: u64,
}

impl From<ExclusionView> for GqlExclusion {
    fn from(view: ExclusionView) -> Self {
        Self {
            id: ID(view.id.to_string()),
            kind: view.kind.into(),
            person_id: ID(view.person_id.to_string()),
            depth: view.depth,
            include_root: view.include_root,
            include_spouses: view.include_spouses,
            created_by: view.created_by.map(|id| ID(id.to_string())),
            created_at: view.created_at,
            person_name: view.person_name,
            person_deleted: view.person_deleted,
            person_count: view.person_count,
        }
    }
}

/// An exclusion to create: REST `POST /trees/{tree_id}/exclusions`.
#[derive(Debug, InputObject)]
pub struct CreateExclusionInput {
    pub kind: GqlExclusionKind,
    pub person_id: ID,
    pub depth: Option<i32>,
    pub include_root: Option<bool>,
    pub include_spouses: Option<bool>,
}

/// The exclusion queries.
#[derive(Default)]
pub struct ExclusionQuery;

#[Object]
impl ExclusionQuery {
    /// The exclusions from sharing of a tree, oldest first.
    async fn exclusions(&self, ctx: &Context<'_>, tree_id: ID) -> Result<Vec<GqlExclusion>> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let list = exclusion::list_exclusions(db_from_ctx(ctx), tree_id).await?;
            Ok(list.into_iter().map(Into::into).collect())
        })
        .await
    }
}

/// The exclusion mutations.
#[derive(Default)]
pub struct ExclusionMutation;

#[Object]
impl ExclusionMutation {
    /// Exclude a person, their ancestors or their descendants from sharing.
    async fn create_exclusion(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        input: CreateExclusionInput,
    ) -> Result<GqlExclusion> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            let new = NewExclusion {
                kind: input.kind.into(),
                person_id: uuid(&input.person_id)?,
                depth: input.depth,
                include_root: input.include_root,
                include_spouses: input.include_spouses,
            };
            let created =
                exclusion::create_exclusion(viewer_from_ctx(ctx), db_from_ctx(ctx), tree_id, new)
                    .await?;
            Ok(created.into())
        })
        .await
    }

    /// Remove an exclusion from sharing; true once removed.
    async fn delete_exclusion(
        &self,
        ctx: &Context<'_>,
        tree_id: ID,
        exclusion_id: ID,
    ) -> Result<bool> {
        boxed(async move {
            let tree_id = live_tree(ctx, &tree_id).await?;
            exclusion::delete_exclusion(
                viewer_from_ctx(ctx),
                db_from_ctx(ctx),
                tree_id,
                uuid(&exclusion_id)?,
            )
            .await?;
            Ok(true)
        })
        .await
    }
}
