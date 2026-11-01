//! `share_link` table entity: a link giving Visitor access to a tree
//! (docs/data-model.md §6.4). Only the token's SHA-256 is stored. Only the
//! web build writes it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "share_link")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    /// The SHA-256 of the token, hexadecimal.
    pub token_hash: String,
    pub label: Option<String>,
    pub scope_id: Option<Uuid>,
    pub expires_at: Option<DateTimeUtc>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTimeUtc,
    pub revoked_at: Option<DateTimeUtc>,
    pub last_used_at: Option<DateTimeUtc>,
    pub use_count: i64,
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
