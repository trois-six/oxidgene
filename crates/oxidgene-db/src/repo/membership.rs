//! Repository for tree memberships (`tree_membership`, docs/data-model.md
//! §6.2): who holds which role on a tree.
//!
//! A membership past its expiry grants nothing: every read of the access it
//! gives compares it with the time of the read, and the worker's maintenance
//! pass deletes it later ([`MembershipRepo::delete_expired`]).

use chrono::{DateTime, Utc};
use oxidgene_core::access::TreeRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Connection;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, Query};
use sea_orm::{
    ActiveModelTrait, Condition, ConnectionTrait, ExprTrait, IntoActiveModel, QueryFilter,
    QuerySelect, Set,
};
use uuid::Uuid;

use crate::entities::tree;
use crate::entities::tree_membership::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// One user's membership of one tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeMembership {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub user_id: Uuid,
    pub role: TreeRole,
    /// The member's own "Who am I?" person.
    pub self_person_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TreeMembership {
    /// Whether the membership still grants its role at `now`.
    pub fn is_live(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_none_or(|expires| expires > now)
    }
}

/// What [`MembershipRepo::update`] changes; `None` keeps a field, and a
/// doubled `Option` clears one with `Some(None)`.
#[derive(Debug, Clone, Default)]
pub struct MembershipChanges {
    pub role: Option<TreeRole>,
    pub self_person_id: Option<Option<Uuid>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// The condition of a membership not expired at `now`.
fn live_at(now: DateTime<Utc>) -> Condition {
    Condition::any()
        .add(Column::ExpiresAt.is_null())
        .add(Column::ExpiresAt.gt(now))
}

/// Repository for memberships.
pub struct MembershipRepo;

impl MembershipRepo {
    /// The role user `user_id` holds on tree `tree_id` at `now`: none when
    /// the tree is missing or deleted, the user no member, or the membership
    /// expired. One indexed read, the tree's liveness joined in.
    pub async fn live_role(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        user_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<TreeRole>, OxidGeneError> {
        let role: Option<String> = Entity::find()
            .select_only()
            .column(Column::Role)
            .inner_join(tree::Entity)
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::UserId.eq(user_id))
            .filter(tree::Column::DeletedAt.is_null())
            .filter(live_at(now))
            .into_tuple()
            .one(db)
            .await
            .map_err(db_err)?;
        Ok(role.as_deref().and_then(TreeRole::parse))
    }

    /// The roles user `user_id` holds at `now` on those of `tree_ids` they
    /// are a member of.
    pub async fn live_roles(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        tree_ids: &[Uuid],
        now: DateTime<Utc>,
    ) -> Result<std::collections::HashMap<Uuid, TreeRole>, OxidGeneError> {
        if tree_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows: Vec<(Uuid, String)> = Entity::find()
            .select_only()
            .column(Column::TreeId)
            .column(Column::Role)
            .filter(Column::UserId.eq(user_id))
            .filter(Column::TreeId.is_in(tree_ids.iter().copied()))
            .filter(live_at(now))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        Ok(rows
            .into_iter()
            .filter_map(|(tree_id, role)| Some((tree_id, TreeRole::parse(&role)?)))
            .collect())
    }

    /// The membership of `user_id` on `tree_id`, expired or not.
    pub async fn get(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<TreeMembership>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// Every membership of user `user_id`, expired or not: their account
    /// data.
    pub async fn of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<Vec<TreeMembership>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// The owner of tree `tree_id`, if it has one.
    pub async fn owner(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Option<TreeMembership>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::Role.eq(TreeRole::Owner.as_str()))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The members of tree `tree_id`, by cursor in the order they joined.
    pub async fn list(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<TreeMembership>, OxidGeneError> {
        let query = Entity::find().filter(Column::TreeId.eq(tree_id));
        paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
    }

    /// The owners of trees `tree_ids`, by tree.
    pub async fn owners(
        db: &impl ConnectionTrait,
        tree_ids: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, Uuid>, OxidGeneError> {
        if tree_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        Ok(Entity::find()
            .filter(Column::TreeId.is_in(tree_ids.iter().copied()))
            .filter(Column::Role.eq(TreeRole::Owner.as_str()))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|model| (model.tree_id, model.user_id))
            .collect())
    }

    /// The live trees user `user_id` owns.
    pub async fn owned_trees(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        Entity::find()
            .select_only()
            .column(Column::TreeId)
            .inner_join(tree::Entity)
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Role.eq(TreeRole::Owner.as_str()))
            .filter(tree::Column::DeletedAt.is_null())
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// The condition keeping the trees user `user_id` is a member of at
    /// `now`, for a query on `tree`.
    pub fn trees_of(user_id: Uuid, now: DateTime<Utc>) -> Condition {
        Condition::all().add(
            tree::Column::Id.in_subquery(
                Query::select()
                    .column(Column::TreeId)
                    .from(Entity)
                    .and_where(Expr::col(Column::UserId).eq(user_id))
                    .cond_where(live_at(now))
                    .to_owned(),
            ),
        )
    }

    /// Make user `user_id` a member of tree `tree_id` with `role`.
    pub async fn create(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        user_id: Uuid,
        role: TreeRole,
        created_by: Option<Uuid>,
    ) -> Result<TreeMembership, OxidGeneError> {
        let now = Utc::now();
        ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(tree_id),
            user_id: Set(user_id),
            role: Set(role.as_str().to_string()),
            self_person_id: Set(None),
            expires_at: Set(None),
            created_by: Set(created_by),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(db)
        .await
        .map(into_domain)
        .map_err(|error| match error.sql_err() {
            // One membership per user and tree, one owner per tree.
            Some(sea_orm::SqlErr::UniqueConstraintViolation(_)) => {
                OxidGeneError::Conflict("the user already holds a role on the tree".into())
            }
            _ => db_err(error),
        })
    }

    /// Change the fields `changes` names.
    pub async fn update(
        db: &impl ConnectionTrait,
        membership_id: Uuid,
        changes: MembershipChanges,
    ) -> Result<TreeMembership, OxidGeneError> {
        let model = Entity::find_by_id(membership_id)
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Membership",
                id: membership_id,
            })?;
        let mut active = model.into_active_model();
        if let Some(role) = changes.role {
            active.role = Set(role.as_str().to_string());
        }
        if let Some(person) = changes.self_person_id {
            active.self_person_id = Set(person);
        }
        if let Some(expires_at) = changes.expires_at {
            active.expires_at = Set(expires_at);
        }
        active.updated_at = Set(Utc::now());
        active.update(db).await.map(into_domain).map_err(db_err)
    }

    /// End user `user_id`'s membership of tree `tree_id`.
    pub async fn delete(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let result = Entity::delete_many()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::UserId.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected > 0)
    }

    /// End every membership of user `user_id`: their erasure.
    pub async fn delete_of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::delete_many()
            .filter(Column::UserId.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Delete the memberships expired at `now`, returning them.
    pub async fn delete_expired(
        db: &impl ConnectionTrait,
        now: DateTime<Utc>,
    ) -> Result<Vec<TreeMembership>, OxidGeneError> {
        let expired: Vec<TreeMembership> = Entity::find()
            .filter(Column::ExpiresAt.lte(now))
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect();
        if !expired.is_empty() {
            Entity::delete_many()
                .filter(Column::Id.is_in(expired.iter().map(|m| m.id)))
                .exec(db)
                .await
                .map_err(db_err)?;
        }
        Ok(expired)
    }
}

fn into_domain(model: tree_membership::Model) -> TreeMembership {
    TreeMembership {
        id: model.id,
        tree_id: model.tree_id,
        user_id: model.user_id,
        // Written only from a `TreeRole`; anything else grants the least.
        role: TreeRole::parse(&model.role).unwrap_or(TreeRole::Visitor),
        self_person_id: model.self_person_id,
        expires_at: model.expires_at,
        created_by: model.created_by,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}
