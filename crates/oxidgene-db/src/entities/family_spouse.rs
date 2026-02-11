//! `family_spouse` table entity.

use sea_orm::entity::prelude::*;

use super::sea_enums::SpouseRole;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "family_spouse")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    pub person_id: Uuid,
    pub role: SpouseRole,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::family::Entity",
        from = "Column::FamilyId",
        to = "super::family::Column::Id"
    )]
    Family,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
}

impl Related<super::family::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Family.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
