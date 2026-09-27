//! `audit_entry` table entity — one row per write to a tree.
//!
//! The enum-valued columns hold the snake_case names of
//! `oxidgene_core::history` enums, and `details` their `AuditDetails` as JSON.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "audit_entry")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub occurred_at: DateTimeUtc,
    pub category: String,
    pub action: String,
    pub entity: String,
    pub entity_id: Option<Uuid>,
    pub subject: Option<String>,
    pub subject_id: Option<Uuid>,
    pub label: Option<String>,
    pub details: Option<String>,
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
