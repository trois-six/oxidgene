//! `source_repository` table entity: a source held at a repository under
//! one call number.

use sea_orm::entity::prelude::*;

use super::sea_enums::SourceMediaType;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "source_repository")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub source_id: Uuid,
    pub repository_id: Uuid,
    pub call_number: Option<String>,
    pub media_type: Option<SourceMediaType>,
    pub sort_order: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::source::Entity",
        from = "Column::SourceId",
        to = "super::source::Column::Id"
    )]
    Source,
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepositoryId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Source.def()
    }
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
