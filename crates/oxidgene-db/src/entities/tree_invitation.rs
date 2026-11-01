//! `tree_invitation` table entity: an invitation by email
//! (docs/data-model.md §6.3). Only the web build writes it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "tree_invitation")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    /// The invited address, lower-cased.
    pub email: String,
    /// `editor` or `guest`.
    pub role: String,
    pub scope_id: Option<Uuid>,
    /// The expiry the membership gets once accepted.
    pub membership_expires_at: Option<DateTimeUtc>,
    pub expires_at: DateTimeUtc,
    pub created_by: Option<Uuid>,
    pub created_at: DateTimeUtc,
    pub last_sent_at: Option<DateTimeUtc>,
    pub accepted_at: Option<DateTimeUtc>,
    pub accepted_by: Option<Uuid>,
    pub declined_at: Option<DateTimeUtc>,
    pub revoked_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tree::Entity",
        from = "Column::TreeId",
        to = "super::tree::Column::Id"
    )]
    Tree,
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
