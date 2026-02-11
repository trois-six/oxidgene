//! `person_ancestry` closure table entity.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "person_ancestry")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub ancestor_id: Uuid,
    pub descendant_id: Uuid,
    pub depth: i32,
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
        belongs_to = "super::person::Entity",
        from = "Column::AncestorId",
        to = "super::person::Column::Id"
    )]
    Ancestor,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::DescendantId",
        to = "super::person::Column::Id"
    )]
    Descendant,
}

// Note: We cannot implement Related<super::person::Entity> twice because SeaORM
// requires a single `Related` impl per target entity. Use the Relation enum
// directly when building queries that need ancestor vs. descendant distinction.

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
