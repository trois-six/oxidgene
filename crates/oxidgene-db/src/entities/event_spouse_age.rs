//! `event_spouse_age` table entity: the age a family event's record gives
//! for one of the family's spouses (GEDCOM `HUSB.AGE` / `WIFE.AGE`).

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "event_spouse_age")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub event_id: Uuid,
    /// The spouse's membership of the event's family — not the person, so
    /// the age follows the membership through a merge.
    pub family_spouse_id: Uuid,
    /// Canonical GEDCOM age (`34y`, `< 1y 6m`, `CHILD`).
    pub age: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::event::Entity",
        from = "Column::EventId",
        to = "super::event::Column::Id"
    )]
    Event,
    #[sea_orm(
        belongs_to = "super::family_spouse::Entity",
        from = "Column::FamilySpouseId",
        to = "super::family_spouse::Column::Id"
    )]
    FamilySpouse,
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::family_spouse::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FamilySpouse.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
