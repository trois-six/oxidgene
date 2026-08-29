//! `background_job` table entity.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "background_job")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub active_tree_id: Option<Uuid>,
    pub kind: String,
    pub format: String,
    pub status: String,
    pub phase: String,
    pub source_key: Option<String>,
    pub artifact_key: Option<String>,
    pub original_filename: Option<String>,
    pub merge_occupations: bool,
    pub merge_names: bool,
    pub done: i64,
    pub total: i64,
    pub attempt: i32,
    pub lease_owner: Option<String>,
    pub lease_until: Option<DateTimeUtc>,
    pub cancel_requested: bool,
    pub result_json: Option<String>,
    pub error_code: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub started_at: Option<DateTimeUtc>,
    pub finished_at: Option<DateTimeUtc>,
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
        belongs_to = "super::tree::Entity",
        from = "Column::ActiveTreeId",
        to = "super::tree::Column::Id"
    )]
    ActiveTree,
}

impl ActiveModelBehavior for ActiveModel {}
