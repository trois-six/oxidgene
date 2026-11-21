//! `person_search` table entity: one row per searchable person, the typed
//! read model the person search returns its results from.
//!
//! Derived from the person's projection and written with it; see
//! `crate::repo::PersonSearchRepo` and docs/data-model.md §4.3. Its words
//! are matched through [`super::person_search_token`].

use sea_orm::entity::prelude::*;

use super::sea_enums::{DateQualifier, Sex};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "person_search")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub person_id: Uuid,
    pub tree_id: Uuid,
    /// The primary surname, particle included, folded
    /// (`oxidgene_core::search::fold_words`): the name order's first key.
    #[sea_orm(column_type = "Text")]
    pub surname_key: String,
    /// The primary given names, folded: the name order's second key.
    #[sea_orm(column_type = "Text")]
    pub given_key: String,
    /// The maiden name, folded.
    #[sea_orm(column_type = "Text", nullable)]
    pub maiden_key: Option<String>,
    pub sex: Sex,
    /// The year of the birth, or of the baptism when the birth has no date.
    pub birth_year: Option<i32>,
    pub birth_qualifier: DateQualifier,
    /// The year of the death, or of the burial when the death has no date.
    pub death_year: Option<i32>,
    pub death_qualifier: DateQualifier,
    /// The date the birth order sorts by.
    pub date_sort: Option<Date>,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Text")]
    pub surname_display: String,
    #[sea_orm(column_type = "Text")]
    pub given_names_display: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub birth_place: Option<String>,
    /// Every spouse's display name, joined by
    /// `crate::repo::RELATIVE_SEP`.
    #[sea_orm(column_type = "Text")]
    pub spouse_names: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub father_name: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub mother_name: Option<String>,
    pub children_count: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person::Entity",
        from = "Column::PersonId",
        to = "super::person::Column::Id"
    )]
    Person,
    #[sea_orm(has_many = "super::person_search_token::Entity")]
    Token,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::person_search_token::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Token.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
