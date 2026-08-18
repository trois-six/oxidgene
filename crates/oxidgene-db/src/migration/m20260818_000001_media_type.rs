//! Say what a medium *is* — twice, because one field cannot answer both
//! questions.
//!
//! # Why two columns and not one
//!
//! GEDCOM has a field for this and its vocabulary is fixed:
//! `OBJE.FILE.FORM.TYPE` in 5.5.1, `FORM.MEDI` in 7.0, enumerating `PHOTO`,
//! `MANUSCRIPT`, `TOMBSTONE`, `FICHE`, `FILM`, `MAP`, `NEWSPAPER` and the
//! rest. Supporting it is not optional if an export is to be readable by
//! other genealogy software, and OxidGene has been emitting nothing at all —
//! `ged_io` parses and writes the tag, and we set it on neither side.
//!
//! But that vocabulary describes the *carrier*, not the record. A census
//! return, a marriage contract and a conscription register are all `MANUSCRIPT`
//! to GEDCOM, and to a genealogist they are three different things — which is
//! precisely the distinction Geneanet's own media types draw ("État civil",
//! "Archive notariée", "Recensement"). Collapsing them into the GEDCOM
//! enumeration on import would discard the classification the user made, and
//! widening the GEDCOM enumeration to hold them would produce files no other
//! program can read.
//!
//! So: `source_media_type` is GEDCOM's, exactly, and round-trips. Nothing
//! else is ever written there. `document_category` is ours, is nullable
//! because a photograph somebody uploaded needs no classification, and knows
//! which physical medium it implies — so choosing a category alone still
//! produces a correct GEDCOM export.
//!
//! `source_media_type` defaults to `other` rather than to `photo`: the table
//! holds scans and PDFs as readily as photographs, and a default that guessed
//! would mislabel every existing row instead of admitting it does not know.

use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // One ADD COLUMN per ALTER TABLE — SQLite accepts no more.
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(
                        ColumnDef::new(Media::SourceMediaType)
                            .string_len(20)
                            .not_null()
                            .default("other")
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Media::Table)
                    .add_column(string_null(Media::DocumentCategory))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [Media::DocumentCategory, Media::SourceMediaType] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Media::Table)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    SourceMediaType,
    DocumentCategory,
}
