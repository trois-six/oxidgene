//! `vignette` table entity — a crop rectangle on a stored media file.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "vignette")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub media_id: Uuid,
    pub page: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub title: Option<String>,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::media::Entity",
        from = "Column::MediaId",
        to = "super::media::Column::Id"
    )]
    Media,
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::event::Entity",
        from = "Column::EventId",
        to = "super::event::Column::Id"
    )]
    Event,
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
