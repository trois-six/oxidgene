//! Sprint F.3 — a multi-page document assembled from images.
//!
//! # Why a page is a media, not a row of its own
//!
//! Registers, notarial acts and probate files arrive as a folder of scans, one
//! image per page, and F.1's `page_count` could not describe that: it counts
//! pages *inside* one file (a PDF, a TIFF), which is a different thing from a
//! document someone assembles out of eight JPEGs.
//!
//! A page has bytes, a thumbnail, dimensions, a content hash and crops drawn on
//! it — every property `media` already models. Giving pages their own table
//! would mean a second upload path, a second thumbnail path, a second serving
//! endpoint and a second thing `media_link` and `vignette` have to point at.
//! So a page *is* a `media`, with a `parent_media_id` naming the document it
//! belongs to and a `page_index` saying where in it. Upload, storage,
//! thumbnails, cropping and serving are then all the code that already exists.
//!
//! # What the parent is
//!
//! The document itself is a `media` too, with `is_document` set. It usually
//! holds no bytes of its own — it is the thing that carries the title, the
//! date, the place, the description and the note, all of which describe the
//! document as a whole rather than any one page. It is also the row a
//! `media_link` points at, so attaching a register to a person attaches the
//! document, not page four of it.
//!
//! Listing queries filter `parent_media_id IS NULL`, so a gallery shows the
//! document once rather than showing its eight pages loose beside it.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            // No FK: SQLite cannot attach one through ALTER TABLE. A page
            // whose parent disappears is cleaned up by the same purge that
            // removes the parent, and by the ORM layer meanwhile.
            ColumnDef::new(Media::ParentMediaId)
                .uuid()
                .null()
                .to_owned(),
            // Zero-based position within the document; 0 for a media that is
            // not a page of anything.
            ColumnDef::new(Media::PageIndex)
                .integer()
                .not_null()
                .default(0)
                .to_owned(),
            // Explicit rather than inferred from "has children": a document is
            // created before its first page is uploaded, and an empty document
            // that reads as an ordinary file would be a document the user
            // cannot add pages to.
            ColumnDef::new(Media::IsDocument)
                .boolean()
                .not_null()
                .default(false)
                .to_owned(),
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(Media::Table)
                        .add_column(column)
                        .to_owned(),
                )
                .await?;
        }

        // Answers "the pages of this document, in order", which is every read
        // the viewer makes.
        manager
            .create_index(
                Index::create()
                    .name("idx_media_parent_page")
                    .table(Media::Table)
                    .col(Media::ParentMediaId)
                    .col(Media::PageIndex)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_media_parent_page")
                    .table(Media::Table)
                    .to_owned(),
            )
            .await?;
        for column in [Media::IsDocument, Media::PageIndex, Media::ParentMediaId] {
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
    ParentMediaId,
    PageIndex,
    IsDocument,
}
