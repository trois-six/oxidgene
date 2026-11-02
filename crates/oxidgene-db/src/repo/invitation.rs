//! Invitations by email (`tree_invitation`, docs/data-model.md §6.3): an
//! address invited to a tree with a role and an optional scope, until it is
//! accepted, declined, revoked or expires.

use chrono::{DateTime, Utc};
use oxidgene_core::access::TreeRole;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::Connection;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, Condition, ConnectionTrait, QueryFilter, QueryOrder, Set};
use uuid::Uuid;

use crate::entities::tree;
use crate::entities::tree_invitation::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, paginate};

/// One invitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeInvitation {
    pub id: Uuid,
    pub tree_id: Uuid,
    /// The invited address, lower-cased.
    pub email: String,
    pub role: TreeRole,
    pub scope_id: Option<Uuid>,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_sent_at: Option<DateTime<Utc>>,
    pub accepted_at: Option<DateTime<Utc>>,
    pub accepted_by: Option<Uuid>,
    pub declined_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl TreeInvitation {
    /// Whether the invitation still waits for an answer at `now`.
    pub fn is_pending(&self, now: DateTime<Utc>) -> bool {
        self.accepted_at.is_none()
            && self.declined_at.is_none()
            && self.revoked_at.is_none()
            && self.expires_at > now
    }
}

/// An invitation to store.
#[derive(Debug, Clone)]
pub struct NewInvitation {
    pub tree_id: Uuid,
    pub email: String,
    pub role: TreeRole,
    pub scope_id: Option<Uuid>,
    pub membership_expires_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

/// The condition of an invitation waiting for an answer at `now`.
fn pending_at(now: DateTime<Utc>) -> Condition {
    Condition::all()
        .add(Column::AcceptedAt.is_null())
        .add(Column::DeclinedAt.is_null())
        .add(Column::RevokedAt.is_null())
        .add(Column::ExpiresAt.gt(now))
}

fn into_domain(model: tree_invitation::Model) -> TreeInvitation {
    TreeInvitation {
        id: model.id,
        tree_id: model.tree_id,
        email: model.email,
        // Written only from an Editor or Guest role; anything else grants
        // the least.
        role: TreeRole::parse(&model.role).unwrap_or(TreeRole::Visitor),
        scope_id: model.scope_id,
        membership_expires_at: model.membership_expires_at,
        expires_at: model.expires_at,
        created_by: model.created_by,
        created_at: model.created_at,
        last_sent_at: model.last_sent_at,
        accepted_at: model.accepted_at,
        accepted_by: model.accepted_by,
        declined_at: model.declined_at,
        revoked_at: model.revoked_at,
    }
}

/// How an invitation ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationEnd {
    Accepted(Uuid),
    Declined,
    Revoked,
}

/// Reads and writes of the invitations.
pub struct InvitationRepo;

impl InvitationRepo {
    /// Store `new`, sent now.
    pub async fn create(
        db: &impl ConnectionTrait,
        new: NewInvitation,
    ) -> Result<TreeInvitation, OxidGeneError> {
        let now = Utc::now();
        ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(new.tree_id),
            email: Set(new.email),
            role: Set(new.role.as_str().to_string()),
            scope_id: Set(new.scope_id),
            membership_expires_at: Set(new.membership_expires_at),
            expires_at: Set(new.expires_at),
            created_by: Set(new.created_by),
            created_at: Set(now),
            last_sent_at: Set(Some(now)),
            accepted_at: Set(None),
            accepted_by: Set(None),
            declined_at: Set(None),
            revoked_at: Set(None),
        }
        .insert(db)
        .await
        .map(into_domain)
        .map_err(db_err)
    }

    /// Invitation `id`, whatever its state.
    pub async fn get(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Option<TreeInvitation>, OxidGeneError> {
        Ok(Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The pending invitation of `email` to `tree_id`, if there is one.
    pub async fn pending_for(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        email: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<TreeInvitation>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(Column::Email.eq(email))
            .filter(pending_at(now))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// The pending invitations of tree `tree_id`, by cursor, oldest first.
    pub async fn list_pending(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        now: DateTime<Utc>,
        params: &PaginationParams,
    ) -> Result<Connection<TreeInvitation>, OxidGeneError> {
        let query = Entity::find()
            .filter(Column::TreeId.eq(tree_id))
            .filter(pending_at(now));
        paginate(db, query, Column::Id, params, |model| {
            (model.id, into_domain(model))
        })
        .await
    }

    /// The pending invitations of address `email` to live trees, oldest
    /// first: what a signed-in user with that verified address sees.
    pub async fn pending_of_email(
        db: &impl ConnectionTrait,
        email: &str,
        now: DateTime<Utc>,
    ) -> Result<Vec<TreeInvitation>, OxidGeneError> {
        Ok(Entity::find()
            .inner_join(tree::Entity)
            .filter(Column::Email.eq(email))
            .filter(tree::Column::DeletedAt.is_null())
            .filter(pending_at(now))
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// Whether address `email` has a pending invitation to a live tree: the
    /// signup gate's question (docs/authentication.md §6.2).
    pub async fn has_pending(
        db: &impl ConnectionTrait,
        email: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, OxidGeneError> {
        Ok(!Self::pending_of_email(db, email, now).await?.is_empty())
    }

    /// Every invitation addressed to `email`, whatever its state: the
    /// user's account data.
    pub async fn of_email(
        db: &impl ConnectionTrait,
        email: &str,
    ) -> Result<Vec<TreeInvitation>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::Email.eq(email))
            .order_by_asc(Column::Id)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// Mark invitation `id` sent again at `now`, waiting until `expires_at`;
    /// `not_found` once it is no longer pending (see [`Self::end`]).
    pub async fn resent(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<TreeInvitation, OxidGeneError> {
        let update = Entity::update_many()
            .col_expr(Column::LastSentAt, Expr::value(Some(now)))
            .col_expr(Column::ExpiresAt, Expr::value(expires_at));
        Self::while_pending(db, id, update, now).await
    }

    /// End invitation `id` as `end` says, at `now`; `not_found` once it is
    /// no longer pending.
    ///
    /// The update is itself the check: it applies only while no answer,
    /// revocation or expiry ended the invitation, so of a revocation and an
    /// acceptance racing (different columns, which would not block each
    /// other), the second finds it ended and fails, and its transaction —
    /// the membership an acceptance creates with it — rolls back.
    pub async fn end(
        db: &impl ConnectionTrait,
        id: Uuid,
        end: InvitationEnd,
        now: DateTime<Utc>,
    ) -> Result<TreeInvitation, OxidGeneError> {
        let update = Entity::update_many();
        let update = match end {
            InvitationEnd::Accepted(user_id) => update
                .col_expr(Column::AcceptedAt, Expr::value(Some(now)))
                .col_expr(Column::AcceptedBy, Expr::value(Some(user_id))),
            InvitationEnd::Declined => update.col_expr(Column::DeclinedAt, Expr::value(Some(now))),
            InvitationEnd::Revoked => update.col_expr(Column::RevokedAt, Expr::value(Some(now))),
        };
        Self::while_pending(db, id, update, now).await
    }

    /// Apply `update` to invitation `id` if it is pending at `now`, and
    /// read it back; `not_found` when it no longer is, as a read of a
    /// pending invitation answers.
    async fn while_pending(
        db: &impl ConnectionTrait,
        id: Uuid,
        update: sea_orm::UpdateMany<Entity>,
        now: DateTime<Utc>,
    ) -> Result<TreeInvitation, OxidGeneError> {
        let result = update
            .filter(Column::Id.eq(id))
            .filter(pending_at(now))
            .exec(db)
            .await
            .map_err(db_err)?;
        if result.rows_affected == 0 {
            return Err(OxidGeneError::NotFound {
                entity: "Invitation",
                id,
            });
        }
        Self::model(db, id).await.map(into_domain)
    }

    /// Delete the invitations no longer pending at `now` — expired,
    /// answered or revoked — returning how many: the worker's maintenance
    /// pass (docs/authorization.md §6.2).
    pub async fn delete_ended(
        db: &impl ConnectionTrait,
        now: DateTime<Utc>,
    ) -> Result<u64, OxidGeneError> {
        let result = Entity::delete_many()
            .filter(
                Condition::any()
                    .add(Column::ExpiresAt.lte(now))
                    .add(Column::AcceptedAt.is_not_null())
                    .add(Column::DeclinedAt.is_not_null())
                    .add(Column::RevokedAt.is_not_null()),
            )
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(result.rows_affected)
    }

    /// Forget user `user_id` as the author or the acceptor of invitations,
    /// and delete those addressed to `email`: their erasure.
    pub async fn forget_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        email: Option<&str>,
    ) -> Result<(), OxidGeneError> {
        if let Some(email) = email {
            Entity::delete_many()
                .filter(Column::Email.eq(email))
                .exec(db)
                .await
                .map_err(db_err)?;
        }
        Entity::update_many()
            .col_expr(
                Column::CreatedBy,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(Column::CreatedBy.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Entity::update_many()
            .col_expr(
                Column::AcceptedBy,
                sea_orm::sea_query::Expr::value(Option::<Uuid>::None),
            )
            .filter(Column::AcceptedBy.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    async fn model(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<tree_invitation::Model, OxidGeneError> {
        Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "Invitation",
                id,
            })
    }
}
