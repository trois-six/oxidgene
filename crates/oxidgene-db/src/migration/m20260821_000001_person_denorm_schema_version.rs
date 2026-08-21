//! Let a stored projection say which build wrote it.
//!
//! # Why
//!
//! `person_denorm.payload` is a JSON `PersonProfile`, and every field added to
//! that shape carries `#[serde(default)]` so the rows already in the table keep
//! deserializing. That is the right call for compatibility and precisely the
//! wrong one for visibility: a payload written before a field existed comes
//! back with the default and looks *complete*. Nothing anywhere can tell "this
//! person genuinely has no date qualifier" from "this row predates qualifiers".
//!
//! That is not hypothetical. Adding `date_qualifier` shipped a feature that was
//! invisible on every existing install — the cards went on drawing bare years,
//! and the only cure was knowing to re-import. Whoever adds the next projection
//! field would have walked into the same trap.
//!
//! # The column, not a field in the payload
//!
//! The version is metadata *about* the row, not part of the profile, and it has
//! to be queryable: "does this tree hold any stale projection" must be one
//! indexable comparison, the same statement on SQLite and PostgreSQL. Inside
//! the JSON it would need each backend's own JSON functions, and counting stale
//! rows would mean decoding every payload in the tree to answer a question
//! asked on every read path.
//!
//! # Why the default is 0
//!
//! Every existing row predates versioning, so 0 means "older than the first
//! version anyone declared" and sorts below it. Nothing needs backfilling: the
//! rows are stale, 0 says so, and the ordinary rebuild-on-missing paths pick
//! them up. Rebuilding here instead would make the migration re-derive every
//! projection in the database, which is slow, needs the whole builder inside a
//! migration, and would redo work the first read does lazily anyway.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(PersonDenorm::Table)
                    .add_column(
                        ColumnDef::new(PersonDenorm::SchemaVersion)
                            .integer()
                            .not_null()
                            .default(0)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

        // Reads filter on (tree_id, schema_version) to skip stale rows, and
        // `count_current` asks that question on the way into every pedigree
        // and every tree-wide profile read.
        manager
            .create_index(
                Index::create()
                    .name("idx_person_denorm_tree_schema_version")
                    .table(PersonDenorm::Table)
                    .col(PersonDenorm::TreeId)
                    .col(PersonDenorm::SchemaVersion)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_person_denorm_tree_schema_version")
                    .table(PersonDenorm::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(PersonDenorm::Table)
                    .drop_column(PersonDenorm::SchemaVersion)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum PersonDenorm {
    Table,
    TreeId,
    SchemaVersion,
}
