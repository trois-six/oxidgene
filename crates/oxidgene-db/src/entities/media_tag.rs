//! Tags attached to a media item.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media_tag")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub media_id: Uuid,
    /// Case-folded key that makes concurrent creation idempotent.
    #[sea_orm(primary_key, auto_increment = false)]
    pub normalized_tag: String,
    /// The spelling entered by the person who first created the tag.
    pub tag: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::media::Entity",
        from = "Column::MediaId",
        to = "super::media::Column::Id"
    )]
    Media,
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
