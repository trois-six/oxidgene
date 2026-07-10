//! `event` table entity.

use sea_orm::entity::prelude::*;

use super::sea_enums::{Calendar, DateQualifier, EventType};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "event")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<Date>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub cause: Option<String>,
    pub place_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub deleted_at: Option<DateTimeUtc>,
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
        belongs_to = "super::place::Entity",
        from = "Column::PlaceId",
        to = "super::place::Column::Id"
    )]
    Place,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::family::Entity",
        from = "Column::FamilyId",
        to = "super::family::Column::Id"
    )]
    Family,
    #[sea_orm(has_many = "super::citation::Entity")]
    Citation,
    #[sea_orm(has_many = "super::media_link::Entity")]
    MediaLink,
    #[sea_orm(has_many = "super::note::Entity")]
    Note,
    #[sea_orm(has_many = "super::event_witness::Entity")]
    EventWitness,
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl Related<super::place::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Place.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::family::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Family.def()
    }
}

impl Related<super::citation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Citation.def()
    }
}

impl Related<super::media_link::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaLink.def()
    }
}

impl Related<super::note::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Note.def()
    }
}

impl Related<super::event_witness::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::EventWitness.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
