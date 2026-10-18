//! `record_tag` table entity — one tag put on one record.
//!
//! Exactly one of the nine record columns is set on every row; the database
//! enforces it with a `CHECK`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "record_tag")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub tag_id: Uuid,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub place_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub citation_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
    pub media_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tag::Entity",
        from = "Column::TagId",
        to = "super::tag::Column::Id"
    )]
    Tag,
    #[sea_orm(
        belongs_to = "super::media::Entity",
        from = "Column::MediaId",
        to = "super::media::Column::Id"
    )]
    Media,
}

impl Related<super::tag::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tag.def()
    }
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
