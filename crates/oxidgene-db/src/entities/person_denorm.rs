//! `person_denorm` materialized person-projection table entity.
//!
//! `payload` holds the JSON-serialized
//! [`oxidgene_core::projection::PersonProfile`]; see
//! `crate::repo::PersonDenormRepo` for the (de)serializing accessors.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "person_denorm")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub person_id: Uuid,
    pub tree_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub payload: String,
    /// Which build's [`PersonProfile`](oxidgene_core::projection::PersonProfile)
    /// shape `payload` holds. Rows below
    /// `oxidgene_core::projection::PROJECTION_SCHEMA_VERSION` read as absent
    /// and are rebuilt; see `crate::repo::PersonDenormRepo`.
    pub schema_version: i32,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(
        belongs_to = "super::tree::Entity",
        from = "Column::TreeId",
        to = "super::tree::Column::Id"
    )]
    Tree,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
