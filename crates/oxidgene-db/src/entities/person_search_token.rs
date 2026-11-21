//! `person_search_token` table entity: the words of a person's search row,
//! one row per word and person, keyed so that a word prefix of a tree is
//! one range of its primary key.
//!
//! See `crate::repo::PersonSearchRepo` and docs/data-model.md §4.3.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "person_search_token")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub tree_id: Uuid,
    /// One folded word (`oxidgene_core::search::fold_words`).
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub token: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub person_id: Uuid,
    /// Which of the person's fields hold the word: a set of
    /// `crate::repo::SearchField` bits.
    pub fields: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::person_search::Entity",
        from = "Column::PersonId",
        to = "super::person_search::Column::PersonId"
    )]
    PersonSearch,
}

impl Related<super::person_search::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PersonSearch.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
