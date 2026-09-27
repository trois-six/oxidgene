//! `record_version` table entity — one stored state of a versioned record.
//!
//! `snapshot` is an `oxidgene_core::history::RecordSnapshot` and `labels` a
//! list of `RecordLabel`, both as JSON.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "record_version")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub audit_entry_id: Uuid,
    pub record_type: String,
    pub record_id: Uuid,
    pub version: i32,
    pub deleted: bool,
    pub created_at: DateTimeUtc,
    pub snapshot: String,
    pub labels: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::audit_entry::Entity",
        from = "Column::AuditEntryId",
        to = "super::audit_entry::Column::Id"
    )]
    AuditEntry,
}

impl Related<super::audit_entry::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AuditEntry.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
