//! `access_scope` table entity: the persons a share is limited to
//! (docs/data-model.md §6.5). Only the web build writes it.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "access_scope")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    /// The `ScopeDefinition`, as JSON.
    #[sea_orm(column_type = "Text")]
    pub definition: String,
    /// The tree's ancestry generation `access_scope_person` was built from;
    /// none before the first build.
    pub built_generation: Option<i64>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
