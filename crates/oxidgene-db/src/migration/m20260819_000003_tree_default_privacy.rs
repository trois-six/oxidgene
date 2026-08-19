//! Give the tree the setting that `privacy = default` has been deferring to.
//!
//! Every `person`, `family` and `media` defaults to `Privacy::Default`, whose
//! documented meaning is "follows the tree-level privacy settings" — and there
//! were none. The most common answer in the model pointed at something that did
//! not exist, so the honest reading of a default row was "undecided" rather than
//! "decided by the tree".
//!
//! One column, not a settings table: this is the only tree-wide preference so
//! far, and a table with one row and one column is a join for nothing. When a
//! second arrives it goes beside this one.
//!
//! `private` is the default default. A genealogy holds living people, and a
//! tree that has not been classified has not been cleared for publication —
//! so the value that applies before anyone has thought about it is the one
//! that withholds. Publishing is the deliberate act, not the accident.
//!
//! Still enforced by nothing; see `m20260819_000002_privacy_family_media`.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .add_column(
                        ColumnDef::new(Tree::DefaultPrivacy)
                            .string_len(10)
                            .not_null()
                            .default("private")
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Tree::Table)
                    .drop_column(Tree::DefaultPrivacy)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Tree {
    Table,
    DefaultPrivacy,
}
