//! In-app notifications (`notification`) and their email preferences
//! (`notification_preference`), docs/messaging.md §2 and §10: rows of one
//! user each, holding ids, never a name or an address.

use chrono::{DateTime, Utc};
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::types::{Connection, Edge, PageInfo};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ConnectionTrait, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::notification::{self, ActiveModel, Column, Entity};
use crate::entities::notification_preference as preference;
use crate::repo::db_err;
use crate::repo::pagination::{PaginationParams, encode_cursor};

/// The PostgreSQL notification channel a new notification is announced on,
/// with its user's id: the long polls of every server process wake on it
/// (docs/messaging.md §6).
pub const UPDATES_CHANNEL: &str = "oxidgene_updates";

/// One notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub kind: String,
    pub tree_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub count: i32,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

/// A notification to store; its id and time are given when it is.
#[derive(Debug, Clone)]
pub struct NewNotification {
    pub user_id: Uuid,
    pub kind: &'static str,
    pub tree_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
}

fn into_domain(model: notification::Model) -> Notification {
    Notification {
        id: model.id,
        user_id: model.user_id,
        kind: model.kind,
        tree_id: model.tree_id,
        actor_user_id: model.actor_user_id,
        subject_id: model.subject_id,
        count: model.count,
        created_at: model.created_at,
        read_at: model.read_at,
    }
}

/// Reads and writes of the notifications.
pub struct NotificationRepo;

impl NotificationRepo {
    /// Store `new`, and announce it on [`UPDATES_CHANNEL`] under
    /// PostgreSQL: within a transaction, the announcement leaves when the
    /// row is committed.
    pub async fn insert(
        db: &impl ConnectionTrait,
        new: NewNotification,
    ) -> Result<Notification, OxidGeneError> {
        let notification = ActiveModel {
            id: Set(Uuid::now_v7()),
            user_id: Set(new.user_id),
            kind: Set(new.kind.to_string()),
            tree_id: Set(new.tree_id),
            actor_user_id: Set(new.actor_user_id),
            subject_id: Set(new.subject_id),
            count: Set(1),
            created_at: Set(Utc::now()),
            read_at: Set(None),
        }
        .insert(db)
        .await
        .map(into_domain)
        .map_err(db_err)?;
        Self::announce(db, new.user_id).await?;
        Ok(notification)
    }

    /// Announce on [`UPDATES_CHANNEL`] that `user_id` has something new,
    /// under PostgreSQL; nothing elsewhere, where one process serves all.
    pub async fn announce(db: &impl ConnectionTrait, user_id: Uuid) -> Result<(), OxidGeneError> {
        if db.get_database_backend() != sea_orm::DatabaseBackend::Postgres {
            return Ok(());
        }
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pg_notify($1, $2)",
            [UPDATES_CHANNEL.into(), user_id.to_string().into()],
        ))
        .await
        .map(drop)
        .map_err(db_err)
    }

    /// The notifications of `user_id`, newest first, by cursor.
    pub async fn list(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<Notification>, OxidGeneError> {
        let limit = params.clamped_first();
        let mut query = Entity::find().filter(Column::UserId.eq(user_id));
        let total_count = PaginatorTrait::count(query.clone(), db)
            .await
            .map_err(db_err)?;
        if let Some(after) = params.decode_cursor()? {
            query = query.filter(Column::Id.lt(after));
        }
        let mut rows = query
            .order_by(Column::Id, Order::Desc)
            .limit(limit + 1)
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        let edges: Vec<Edge<Notification>> = rows
            .into_iter()
            .map(|row| Edge {
                cursor: encode_cursor(&row.id),
                node: into_domain(row),
            })
            .collect();
        let end_cursor = edges.last().map(|edge| edge.cursor.clone());
        Ok(Connection {
            edges,
            page_info: PageInfo {
                has_next_page,
                end_cursor,
            },
            total_count: i64::try_from(total_count).unwrap_or(i64::MAX),
        })
    }

    /// How many notifications of `user_id` are unread.
    pub async fn unread_count(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<u64, OxidGeneError> {
        Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::ReadAt.is_null())
            .count(db)
            .await
            .map_err(db_err)
    }

    /// The id of the newest notification of `user_id`, if any.
    pub async fn newest(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<Option<Uuid>, OxidGeneError> {
        Entity::find()
            .select_only()
            .column(Column::Id)
            .filter(Column::UserId.eq(user_id))
            .order_by(Column::Id, Order::Desc)
            .limit(1)
            .into_tuple()
            .one(db)
            .await
            .map_err(db_err)
    }

    /// The notifications of `user_id` newer than `after`, oldest first, at
    /// most `limit`.
    pub async fn after(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        after: Uuid,
        limit: u64,
    ) -> Result<Vec<Notification>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Id.gt(after))
            .order_by(Column::Id, Order::Asc)
            .limit(limit)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// The unread notifications of `user_id` of one of `kinds` created at
    /// `since` or later, oldest first: what a daily digest gathers.
    pub async fn unread_since(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        kinds: &[&str],
        since: DateTime<Utc>,
    ) -> Result<Vec<Notification>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::ReadAt.is_null())
            .filter(Column::CreatedAt.gte(since))
            .filter(Column::Kind.is_in(kinds.iter().copied()))
            .order_by(Column::Id, Order::Asc)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(into_domain)
            .collect())
    }

    /// Notification `id` of `user_id`, if it is theirs.
    pub async fn get(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Notification>, OxidGeneError> {
        Ok(Entity::find_by_id(id)
            .filter(Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// Mark the unread notifications `ids` of `user_id` read at `now` — all
    /// of them for `None`; how many were.
    pub async fn mark_read(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        ids: Option<&[Uuid]>,
        now: DateTime<Utc>,
    ) -> Result<u64, OxidGeneError> {
        let mut update = Entity::update_many()
            .col_expr(Column::ReadAt, Expr::value(Some(now)))
            .filter(Column::UserId.eq(user_id))
            .filter(Column::ReadAt.is_null());
        if let Some(ids) = ids {
            update = update.filter(Column::Id.is_in(ids.iter().copied()));
        }
        Ok(update.exec(db).await.map_err(db_err)?.rows_affected)
    }

    /// Delete the notifications created before `cutoff`.
    pub async fn delete_before(
        db: &impl ConnectionTrait,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, OxidGeneError> {
        Ok(Entity::delete_many()
            .filter(Column::CreatedAt.lt(cutoff))
            .exec(db)
            .await
            .map_err(db_err)?
            .rows_affected)
    }

    /// Delete every notification and preference of `user_id`: their
    /// erasure.
    pub async fn delete_of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<(), OxidGeneError> {
        Entity::delete_many()
            .filter(Column::UserId.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        preference::Entity::delete_many()
            .filter(preference::Column::UserId.eq(user_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// The email preferences `user_id` changed, as `(kind, email)`.
    pub async fn preferences(
        db: &impl ConnectionTrait,
        user_id: Uuid,
    ) -> Result<Vec<(String, String)>, OxidGeneError> {
        Ok(preference::Entity::find()
            .filter(preference::Column::UserId.eq(user_id))
            .order_by(preference::Column::Kind, Order::Asc)
            .all(db)
            .await
            .map_err(db_err)?
            .into_iter()
            .map(|row| (row.kind, row.email))
            .collect())
    }

    /// Set the email preference of `user_id` for `kind`.
    pub async fn set_preference(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        kind: &str,
        email: &str,
    ) -> Result<(), OxidGeneError> {
        preference::Entity::insert(preference::ActiveModel {
            user_id: Set(user_id),
            kind: Set(kind.to_string()),
            email: Set(email.to_string()),
        })
        .on_conflict(
            OnConflict::columns([preference::Column::UserId, preference::Column::Kind])
                .update_column(preference::Column::Email)
                .to_owned(),
        )
        .exec(db)
        .await
        .map(drop)
        .map_err(db_err)
    }
}
