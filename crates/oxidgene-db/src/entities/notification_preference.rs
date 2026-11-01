//! `notification_preference` table entity: how a user wants one kind of
//! notification by email (docs/messaging.md §2.3); a row per changed kind.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "notification_preference")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    /// A notification kind, or `all` for the switch turning all email off.
    #[sea_orm(primary_key, auto_increment = false)]
    pub kind: String,
    /// `off`, `immediate` or `daily`.
    pub email: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
