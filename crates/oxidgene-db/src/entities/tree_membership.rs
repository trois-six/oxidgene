//! `tree_membership` table entity: a user's role on a tree
//! (docs/data-model.md §6.2). Only the web build writes it; it stays empty
//! on the desktop.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "tree_membership")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub user_id: Uuid,
    /// `owner`, `editor` or `guest`; one `owner` per tree.
    pub role: String,
    /// The access scope the member is limited to (docs/authorization.md
    /// §6.5); none for the whole tree.
    pub scope_id: Option<Uuid>,
    /// The member's own "Who am I?" person.
    pub self_person_id: Option<Uuid>,
    /// Past it, the membership grants nothing.
    pub expires_at: Option<DateTimeUtc>,
    /// The user who shared the tree.
    pub created_by: Option<Uuid>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tree::Entity",
        from = "Column::TreeId",
        to = "super::tree::Column::Id"
    )]
    Tree,
    #[sea_orm(
        belongs_to = "super::app_user::Entity",
        from = "Column::UserId",
        to = "super::app_user::Column::Id"
    )]
    User,
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
