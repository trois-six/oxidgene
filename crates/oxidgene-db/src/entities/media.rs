//! `media` table entity.

use sea_orm::entity::prelude::*;

use super::sea_enums::{Calendar, DateQualifier, Privacy, SourceMediaType};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub tree_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub storage_key: Option<String>,
    pub sha256: Option<String>,
    pub thumbnail_key: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub page_count: i32,
    /// The document this is a page of, if it is one.
    pub parent_media_id: Option<Uuid>,
    /// Zero-based position within that document.
    pub page_index: i32,
    /// `true` when this row *is* a multi-page document rather than a file.
    pub is_document: bool,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    pub date_value: Option<String>,
    pub date_sort: Option<Date>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    /// GEDCOM's `SOURCE_MEDIA_TYPE` — what the medium physically is.
    pub source_media_type: SourceMediaType,
    /// What kind of record it is, where GEDCOM's vocabulary cannot say.
    /// Stored as its snake_case spelling; `None` when unclassified.
    pub document_category: Option<String>,
    /// Legacy JSON array retained to migrate existing installations. New reads
    /// and writes use `media_tag`, so tags are independently mutable.
    pub tags: String,
    pub place_id: Option<Uuid>,
    /// Recorded now, enforced when authentication lands.
    pub privacy: Privacy,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub deleted_at: Option<DateTimeUtc>,
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
        belongs_to = "super::place::Entity",
        from = "Column::PlaceId",
        to = "super::place::Column::Id"
    )]
    Place,
    #[sea_orm(has_many = "super::media_link::Entity")]
    MediaLink,
    #[sea_orm(has_many = "super::vignette::Entity")]
    Vignette,
}

impl Related<super::tree::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Tree.def()
    }
}

impl Related<super::place::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Place.def()
    }
}

impl Related<super::media_link::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaLink.def()
    }
}

impl Related<super::vignette::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Vignette.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
