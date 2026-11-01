//! The mail outbox (`mail_outbox`, docs/messaging.md §3.3): a mail is a row
//! written in the transaction of what causes it, which the worker claims
//! with a lease, sends through the SMTP relay and marks sent, retries after
//! a failure, or marks failed or dropped. Its parameters are ids; its
//! address goes with the row, seven days after it ended.

use chrono::{DateTime, Duration, Utc};
use oxidgene_core::error::OxidGeneError;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{Condition, ConnectionTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect, Set};
use uuid::Uuid;

use crate::entities::mail_outbox::{self, ActiveModel, Column, Entity};
use crate::repo::db_err;

/// The PostgreSQL notification channel a queued mail is announced on: the
/// mail senders of every process wake on it.
pub const MAIL_CHANNEL: &str = "oxidgene_mail";

/// Where a mail is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailStatus {
    /// Waiting to be sent, or to be tried again.
    Pending,
    Sent,
    /// Given up, with its error category.
    Failed,
    /// Not sent because it no longer applied: a revoked invitation, a
    /// notification already read.
    Dropped,
}

impl MailStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Failed => "failed",
            Self::Dropped => "dropped",
        }
    }
}

/// A queued mail, as the worker claims it.
#[derive(Clone, PartialEq, Eq)]
pub struct OutboxMail {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub to_address: String,
    pub kind: String,
    pub language: String,
    /// The ids it is rendered from, as JSON.
    pub params: String,
    pub status: String,
    /// How many times it was claimed, this time included.
    pub attempts: i32,
    pub next_attempt_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub error_category: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl std::fmt::Debug for OutboxMail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the address.
        f.debug_struct("OutboxMail")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("status", &self.status)
            .field("attempts", &self.attempts)
            .finish_non_exhaustive()
    }
}

/// A mail to queue.
#[derive(Clone)]
pub struct NewMail {
    pub user_id: Option<Uuid>,
    pub to_address: String,
    pub kind: &'static str,
    pub language: String,
    pub params: serde_json::Value,
    /// When it may be sent: now, or a digest's hour.
    pub due: DateTime<Utc>,
}

fn into_domain(model: mail_outbox::Model) -> OutboxMail {
    OutboxMail {
        id: model.id,
        user_id: model.user_id,
        to_address: model.to_address,
        kind: model.kind,
        language: model.language,
        params: model.params,
        status: model.status,
        attempts: model.attempts,
        next_attempt_at: model.next_attempt_at,
        sent_at: model.sent_at,
        error_category: model.error_category,
        created_at: model.created_at,
    }
}

/// A pending row a worker may take at `now`: due, and not leased to a
/// worker that is still sending it.
fn claimable(now: DateTime<Utc>) -> Condition {
    Condition::all()
        .add(Column::Status.eq(MailStatus::Pending.as_str()))
        .add(Column::NextAttemptAt.lte(now))
        .add(
            Condition::any()
                .add(Column::LeaseUntil.is_null())
                .add(Column::LeaseUntil.lt(now)),
        )
}

/// Reads and writes of the outbox.
pub struct MailOutboxRepo;

impl MailOutboxRepo {
    /// Queue `mail`, and announce it on [`MAIL_CHANNEL`] under PostgreSQL:
    /// within a transaction, the announcement leaves when the row is
    /// committed.
    pub async fn enqueue(db: &impl ConnectionTrait, mail: NewMail) -> Result<Uuid, OxidGeneError> {
        let id = Uuid::now_v7();
        ActiveModel {
            id: Set(id),
            user_id: Set(mail.user_id),
            to_address: Set(mail.to_address),
            kind: Set(mail.kind.to_string()),
            language: Set(mail.language),
            params: Set(mail.params.to_string()),
            status: Set(MailStatus::Pending.as_str().to_string()),
            attempts: Set(0),
            next_attempt_at: Set(mail.due),
            lease_until: Set(None),
            sent_at: Set(None),
            error_category: Set(None),
            created_at: Set(Utc::now()),
        }
        .insert(db)
        .await
        .map_err(db_err)?;
        if db.get_database_backend() == sea_orm::DatabaseBackend::Postgres {
            db.execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT pg_notify($1, '')",
                [MAIL_CHANNEL.into()],
            ))
            .await
            .map_err(db_err)?;
        }
        Ok(id)
    }

    /// Mail `id`, whatever its state.
    pub async fn get(
        db: &impl ConnectionTrait,
        id: Uuid,
    ) -> Result<Option<OutboxMail>, OxidGeneError> {
        Ok(Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// Claim the mail due longest at `now` for `lease`, counting the
    /// attempt; `None` when nothing is due, or another worker took it
    /// first.
    pub async fn claim_next(
        db: &impl ConnectionTrait,
        now: DateTime<Utc>,
        lease: Duration,
    ) -> Result<Option<OutboxMail>, OxidGeneError> {
        let Some(candidate) = Entity::find()
            .filter(claimable(now))
            .order_by_asc(Column::NextAttemptAt)
            .one(db)
            .await
            .map_err(db_err)?
        else {
            return Ok(None);
        };
        let updated = Entity::update_many()
            .col_expr(Column::LeaseUntil, Expr::value(Some(now + lease)))
            .col_expr(Column::Attempts, Expr::col(Column::Attempts).add(1))
            .filter(Column::Id.eq(candidate.id))
            .filter(claimable(now))
            .exec(db)
            .await
            .map_err(db_err)?;
        if updated.rows_affected == 0 {
            return Ok(None);
        }
        Self::get(db, candidate.id).await
    }

    /// When the next pending mail falls due: the earliest attempt of those
    /// waiting, and the earliest lease of those being sent, which a stopped
    /// worker leaves to run out.
    pub async fn next_due(
        db: &impl ConnectionTrait,
    ) -> Result<Option<DateTime<Utc>>, OxidGeneError> {
        let waiting: Option<DateTime<Utc>> = Entity::find()
            .select_only()
            .column(Column::NextAttemptAt)
            .filter(Column::Status.eq(MailStatus::Pending.as_str()))
            .filter(Column::LeaseUntil.is_null())
            .order_by_asc(Column::NextAttemptAt)
            .limit(1)
            .into_tuple()
            .one(db)
            .await
            .map_err(db_err)?;
        let leased: Option<Option<DateTime<Utc>>> = Entity::find()
            .select_only()
            .column(Column::LeaseUntil)
            .filter(Column::Status.eq(MailStatus::Pending.as_str()))
            .filter(Column::LeaseUntil.is_not_null())
            .order_by_asc(Column::LeaseUntil)
            .limit(1)
            .into_tuple()
            .one(db)
            .await
            .map_err(db_err)?;
        Ok(match (waiting, leased.flatten()) {
            (Some(a), Some(b)) => Some(Ord::min(a, b)),
            (a, b) => a.or(b),
        })
    }

    /// Mark mail `id` sent at `now`.
    pub async fn mark_sent(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), OxidGeneError> {
        Self::finish(db, id, MailStatus::Sent, now, None).await
    }

    /// Give mail `id` up at `now`, with `category`.
    pub async fn mark_failed(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
        category: &str,
    ) -> Result<(), OxidGeneError> {
        Self::finish(db, id, MailStatus::Failed, now, Some(category)).await
    }

    /// Mark mail `id`, which no longer applies, dropped at `now`, with why.
    pub async fn mark_dropped(
        db: &impl ConnectionTrait,
        id: Uuid,
        now: DateTime<Utc>,
        reason: &str,
    ) -> Result<(), OxidGeneError> {
        Self::finish(db, id, MailStatus::Dropped, now, Some(reason)).await
    }

    async fn finish(
        db: &impl ConnectionTrait,
        id: Uuid,
        status: MailStatus,
        now: DateTime<Utc>,
        category: Option<&str>,
    ) -> Result<(), OxidGeneError> {
        let sent_at = (status == MailStatus::Sent).then_some(now);
        Entity::update_many()
            .col_expr(Column::Status, Expr::value(status.as_str()))
            .col_expr(Column::NextAttemptAt, Expr::value(now))
            .col_expr(Column::LeaseUntil, Expr::value(None::<DateTime<Utc>>))
            .col_expr(Column::SentAt, Expr::value(sent_at))
            .col_expr(Column::ErrorCategory, Expr::value(category))
            .filter(Column::Id.eq(id))
            .exec(db)
            .await
            .map(drop)
            .map_err(db_err)
    }

    /// Try mail `id` again at `at`, after a failure of `category`.
    pub async fn retry_at(
        db: &impl ConnectionTrait,
        id: Uuid,
        at: DateTime<Utc>,
        category: &str,
    ) -> Result<(), OxidGeneError> {
        Entity::update_many()
            .col_expr(Column::NextAttemptAt, Expr::value(at))
            .col_expr(Column::LeaseUntil, Expr::value(None::<DateTime<Utc>>))
            .col_expr(Column::ErrorCategory, Expr::value(category))
            .filter(Column::Id.eq(id))
            .filter(Column::Status.eq(MailStatus::Pending.as_str()))
            .exec(db)
            .await
            .map(drop)
            .map_err(db_err)
    }

    /// The pending mail of `kind` to `user_id`, if any: the daily digest
    /// a new notification joins.
    pub async fn pending_of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        kind: &str,
    ) -> Result<Option<OutboxMail>, OxidGeneError> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Kind.eq(kind))
            .filter(Column::Status.eq(MailStatus::Pending.as_str()))
            .one(db)
            .await
            .map_err(db_err)?
            .map(into_domain))
    }

    /// Delete the mail that ended — sent, failed or dropped — before
    /// `cutoff`, and its address with it.
    pub async fn delete_ended_before(
        db: &impl ConnectionTrait,
        cutoff: DateTime<Utc>,
    ) -> Result<u64, OxidGeneError> {
        Ok(Entity::delete_many()
            .filter(Column::Status.ne(MailStatus::Pending.as_str()))
            .filter(Column::NextAttemptAt.lt(cutoff))
            .exec(db)
            .await
            .map_err(db_err)?
            .rows_affected)
    }

    /// Delete the mail queued for `user_id` or addressed to `email`: their
    /// erasure.
    pub async fn delete_of_user(
        db: &impl ConnectionTrait,
        user_id: Uuid,
        email: Option<&str>,
    ) -> Result<(), OxidGeneError> {
        let mut whose = Condition::any().add(Column::UserId.eq(user_id));
        if let Some(email) = email {
            whose = whose.add(Column::ToAddress.eq(email));
        }
        Entity::delete_many()
            .filter(whose)
            .exec(db)
            .await
            .map(drop)
            .map_err(db_err)
    }
}
