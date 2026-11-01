//! Share links (`share_link`, docs/data-model.md §6.4): a token giving
//! Visitor access to a tree, of which only the SHA-256 is stored, with its
//! label, scope, expiry and use count.

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Connection;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, ExprTrait};
use sea_orm::{ActiveModelTrait, Condition, ConnectionTrait, IntoActiveModel, QueryFilter, Set};
use uuid::Uuid;

use crate::entities::share_link::{self, ActiveModel, Column, Entity};
use crate::entities::tree;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// One share link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareLink {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub label: Option<String>,
    pub scope_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub use_count: i64,
}

impl ShareLink {
    /// Whether the link still grants access at `now`.
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|expires| expires > now)
    }
}

/// A share link to store.
#[derive(Debug, Clone)]
pub struct NewShareLink {
    pub tree_id: Uuid,
    /// The SHA-256 of the token, hexadecimal.
    pub token_hash: String,
    pub label: Option<String>,
    pub scope_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
}

/// What [`ShareLinkRepo::update`] changes; `None` keeps a field, and a
/// doubled `Option` clears one with `Some(None)`.
#[derive(Debug, Clone, Default)]
pub struct ShareLinkChanges {
    pub label: Option<Option<String>>,
    pub scope_id: Option<Option<Uuid>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

fn into_domain(model: share_link::Model) -> ShareLink {
    ShareLink {
        id: model.id,
        tree_id: model.tree_id,
        label: model.label,
        scope_id: model.scope_id,
        expires_at: model.expires_at,
        created_by: model.created_by,
        created_at: model.created_at,
        revoked_at: model.revoked_at,
        last_used_at: model.last_used_at,
        use_count: model.use_count,
    }
}

/// The condition of a link not revoked nor expired at `now`.
fn live_at(now: DateTime<Utc>) -> Condition {
    Condition::all().add(Column::RevokedAt.is_null()).add(
        Condition::any()
            .add(Column::ExpiresAt.is_null())
            .add(Column::ExpiresAt.gt(now)),
    )
}

/// Reads and writes of the share links.
pub struct ShareLinkRepo;

impl ShareLinkRepo {
    /// Store `new`.
    pub async fn create(
        db: &impl ConnectionTrait,
        new: NewShareLink,
    ) -> Result<ShareLink, OxidGeneError> {
        ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(new.tree_id),
            token_hash: Set(new.token_hash),
            label: Set(new.label),
            scope_id: Set(new.scope_id),
            expires_at: Set(new.expires_at),
            created_by: Set(new.created_by),
            created_at: Set(Utc::now()),
            revoked_at: Set(None),
            last_used_at: Set(None),
            use_count: Set(0),
        }
        .insert(db)
        .await
        .map(into_domain)
        .map_err(db_err)
    }

    /// Link `id` of tree `tree_id`, revoked or not; `NotFound` otherwise.
    pub async fn get(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
    ) -> Result<ShareLink, OxidGeneError> {
        Entity::find_by_id(id)
            .filter(Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain)
            .ok_or(OxidGeneError::NotFound {
                entity: "ShareLink",
                id,
            })
    }

    /// The live link whose token has digest `token_hash`, on a live tree.
    pub async fn live_by_token(
        db: &impl ConnectionTrait,
        token_hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<ShareLink>, OxidGeneError> {
        Ok(Entity::find()
            .inner_join(tree::Entity)
            .filter(Column::TokenHash.eq(token_hash))
            .filter(tree::Column::DeletedAt.is_null())
            .filter(live_at(now))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The live links among `ids` that open tree `tree_id` at `now`: the
    /// grants a session holds for it. Revoking a link ends its grants here,
    /// at the next request.
    pub async fn live_of_tree(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<Vec<ShareLink>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(Entity::find()
            .filter(Column::Id.is_in(ids.iter().copied()))
            .filter(Column::TreeId.eq(tree_id))
            .filter(live_at(now))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// The live links among `ids`, on live trees: every tree a session's
    /// grants open.
    pub async fn live_among(
        db: &impl ConnectionTrait,
        ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<Vec<ShareLink>, OxidGeneError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(Entity::find()
            .inner_join(tree::Entity)
            .filter(Column::Id.is_in(ids.iter().copied()))
            .filter(tree::Column::DeletedAt.is_null())
            .filter(live_at(now))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// The links of tree `tree_id` not revoked, by cursor, oldest first.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<ShareLink>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::RevokedAt.is_null());
        paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
    }

    /// Change the fields `changes` names on link `id` of tree `tree_id`.
    pub async fn update(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
        changes: ShareLinkChanges,
    ) -> Result<ShareLink, OxidGeneError> {
        let model = Entity::find_by_id(id)
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::RevokedAt.is_null())
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "ShareLink",
                id,
            })?;
        let mut active = model.into_active_model();
        if let Some(label) = changes.label {
            active.label = Set(label);
        }
        if let Some(scope) = changes.scope_id {
            active.scope_id = Set(scope);
        }
        if let Some(expires_at) = changes.expires_at {
            active.expires_at = Set(expires_at);
        }
        active.update(db).await.map(into_domain).map_err(db_err)
    }

    /// Revoke link `id` of tree `tree_id` at `now`; `NotFound` for a link
    /// of another tree or one already revoked.
    pub async fn revoke(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<ShareLink, OxidGeneError> {
        let model = Entity::find_by_id(id)
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::RevokedAt.is_null())
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "ShareLink",
                id,
            })?;
        let mut active = model.into_active_model();
        active.revoked_at = Set(Some(now));
        active.update(db).await.map(into_domain).map_err(db_err)
    }

    /// Count one use of link `id` at `now`.
    pub async fn record_use(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), OxidGeneError> {
        Entity::update_many()
            .col_expr(Column::UseCount, Expr::col(Column::UseCount).add(1))
            .col_expr(Column::LastUsedAt, Expr::value(now))
            .filter(Column::Id.eq(id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Forget user `user_id` as the author of links: their erasure. The
    /// links stay the tree's.
    pub async fn forget_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::update_many()
            .col_expr(Column::CreatedBy, Expr::value(Option::<Uuid>::None))
            .filter(Column::CreatedBy.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }
}
