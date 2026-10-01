//! `record_version` table entity — one past state of a versioned record,
//! stored by the write that replaced it.
//!
//! `snapshot` is an `oxidgene_core::history::RecordSnapshot` and `labels` a
//! list of `RecordLabel`, both as JSON. Both are null for a deleted state, and
//! for the state a soft deletion replaced, which the soft-deleted row still
//! holds; see `docs/data-model.md` §5.2.

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
    pub snapshot: Option<String>,
    pub labels: Option<String>,
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
