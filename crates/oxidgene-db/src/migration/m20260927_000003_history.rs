//! Record every write to a tree, and keep the successive states of its
//! persons, places, sources, and settings.
//!
//! `audit_entry` holds one row per write: what was done, to which kind of row,
//! about which record, and the record's display label at the time. It is the
//! tree's audit log, read newest first, optionally filtered by category — the
//! two indexes lead with `tree_id` for exactly those reads.
//!
//! `record_version` holds the state a write left a versioned record in, as
//! JSON, numbered per record. The unique index on the record and its number is
//! both how a record's history is read and what keeps two writers from
//! numbering the same version twice.
//!
//! Both tables cascade from `tree`, and versions from their audit entry, so the
//! purge of a tree takes its history with it. Neither references the persons
//! or places it describes: a history has to outlive what it records.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AuditEntry::Table)
                    .if_not_exists()
                    .col(uuid(AuditEntry::Id).primary_key())
                    .col(uuid(AuditEntry::TreeId))
                    .col(timestamp_with_time_zone(AuditEntry::OccurredAt))
                    .col(string_len(AuditEntry::Category, 16))
                    .col(string_len(AuditEntry::Action, 16))
                    .col(string_len(AuditEntry::Entity, 32))
                    .col(uuid_null(AuditEntry::EntityId))
                    .col(string_len_null(AuditEntry::Subject, 16))
                    .col(uuid_null(AuditEntry::SubjectId))
                    .col(text_null(AuditEntry::Label))
                    .col(text_null(AuditEntry::Details))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_audit_entry_tree")
                            .from(AuditEntry::Table, AuditEntry::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_entry_tree")
                    .table(AuditEntry::Table)
                    .col(AuditEntry::TreeId)
                    .col(AuditEntry::Id)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_entry_tree_category")
                    .table(AuditEntry::Table)
                    .col(AuditEntry::TreeId)
                    .col(AuditEntry::Category)
                    .col(AuditEntry::Id)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(RecordVersion::Table)
                    .if_not_exists()
                    .col(uuid(RecordVersion::Id).primary_key())
                    .col(uuid(RecordVersion::TreeId))
                    .col(uuid(RecordVersion::AuditEntryId))
                    .col(string_len(RecordVersion::RecordType, 16))
                    .col(uuid(RecordVersion::RecordId))
                    .col(integer(RecordVersion::Version))
                    .col(boolean(RecordVersion::Deleted).default(false))
                    .col(timestamp_with_time_zone(RecordVersion::CreatedAt))
                    .col(text(RecordVersion::Snapshot))
                    .col(text(RecordVersion::Labels))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_record_version_tree")
                            .from(RecordVersion::Table, RecordVersion::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_record_version_audit_entry")
                            .from(RecordVersion::Table, RecordVersion::AuditEntryId)
                            .to(AuditEntry::Table, AuditEntry::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_record_version_record")
                    .table(RecordVersion::Table)
                    .col(RecordVersion::RecordType)
                    .col(RecordVersion::RecordId)
                    .col(RecordVersion::Version)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_record_version_audit_entry")
                    .table(RecordVersion::Table)
                    .col(RecordVersion::AuditEntryId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_record_version_tree")
                    .table(RecordVersion::Table)
                    .col(RecordVersion::TreeId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(RecordVersion::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(AuditEntry::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum AuditEntry {
    Table,
    Id,
    TreeId,
    OccurredAt,
    Category,
    Action,
    Entity,
    EntityId,
    Subject,
    SubjectId,
    Label,
    Details,
}

#[derive(DeriveIden)]
enum RecordVersion {
    Table,
    Id,
    TreeId,
    AuditEntryId,
    RecordType,
    RecordId,
    Version,
    Deleted,
    CreatedAt,
    Snapshot,
    Labels,
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
}
