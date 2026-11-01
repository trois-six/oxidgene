//! `notification` table entity: an in-app notification of one user
//! (docs/messaging.md §2, §10). Ids only; only the web build writes it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "notification")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub user_id: Uuid,
    /// `tree_shared`, `membership_changed`… (docs/messaging.md §2.1).
    pub kind: String,
    pub tree_id: Option<Uuid>,
    pub actor_user_id: Option<Uuid>,
    /// What else it is about: a job.
    pub subject_id: Option<Uuid>,
    pub count: i32,
    pub created_at: DateTimeUtc,
    pub read_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
