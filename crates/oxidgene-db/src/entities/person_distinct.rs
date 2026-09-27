//! `person_distinct` table entity — two same-named persons confirmed to be
//! different people.
//!
//! One row per pair, with `person_id < other_person_id`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "person_distinct")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub person_id: Uuid,
    pub other_person_id: Uuid,
    pub created_at: DateTimeUtc,
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
