//! `media` table entity.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub storage_key: Option<String>,
    pub sha256: Option<String>,
    pub thumbnail_key: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub page_count: i32,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub date_value: Option<String>,
    pub date_sort: Option<Date>,
    pub place_id: Option<Uuid>,
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
    #[sea_orm(has_many = "super::media_link::Entity")]
    MediaLink,
    #[sea_orm(has_many = "super::vignette::Entity")]
    Vignette,
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

impl Related<super::media_link::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaLink.def()
    }
}

impl Related<super::vignette::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vignette.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
