//! `family` table entity.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "family")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
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
    #[sea_orm(has_many = "super::family_spouse::Entity")]
    FamilySpouse,
    #[sea_orm(has_many = "super::family_child::Entity")]
    FamilyChild,
    #[sea_orm(has_many = "super::event::Entity")]
    Event,
    #[sea_orm(has_many = "super::citation::Entity")]
    Citation,
    #[sea_orm(has_many = "super::media_link::Entity")]
    MediaLink,
    #[sea_orm(has_many = "super::note::Entity")]
    Note,
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl Related<super::family_spouse::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FamilySpouse.def()
    }
}

impl Related<super::family_child::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FamilyChild.def()
    }
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
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

impl ActiveModelBehavior for ActiveModel {}
