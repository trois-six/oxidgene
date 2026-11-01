//! `mail_outbox` table entity: a mail queued for the SMTP relay
//! (docs/messaging.md §3.3, §10). Only the web build writes it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "mail_outbox")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub to_address: String,
    /// `invitation`, a notification kind, or `digest`.
    pub kind: String,
    /// The interface language the mail is written in.
    pub language: String,
    /// The ids the mail is rendered from, as JSON.
    pub params: String,
    /// `pending`, `sent`, `failed` or `dropped`.
    pub status: String,
    pub attempts: i32,
    /// When a pending row is due; when a finished one ended.
    pub next_attempt_at: DateTimeUtc,
    pub lease_until: Option<DateTimeUtc>,
    pub sent_at: Option<DateTimeUtc>,
    pub error_category: Option<String>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
