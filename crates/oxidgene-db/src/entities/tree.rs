//! `tree` table entity.

use sea_orm::entity::prelude::*;

use super::sea_enums::{Calendar, DateDisplayFormat, DateInputFormat, TreeDefaultPrivacy};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "tree")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub sosa_root_person_id: Option<Uuid>,
    pub self_person_id: Option<Uuid>,
    /// What `Privacy::Default` means for everything in this tree.
    pub default_privacy: TreeDefaultPrivacy,
    /// Whether entry fields suggest values as the user types.
    pub entry_suggestions: bool,
    /// How much of a date the tree's pages write.
    pub date_format: DateDisplayFormat,
    /// Whether lifespans write the birth and death symbols.
    pub date_symbols: bool,
    /// Whether an approximate date reads « c. ».
    pub date_circa: bool,
    /// The calendar a date recorded in another one is also given in.
    pub date_calendar: Calendar,
    /// Whether surname fields write in capitals.
    pub surname_uppercase: bool,
    /// Whether adding a relative offers the persons already in the tree.
    pub suggest_persons: bool,
    /// The order and form of a date field's parts.
    pub date_input_format: DateInputFormat,
    /// The calendar an empty date field starts in.
    pub date_input_calendar: Calendar,
    pub submitter_name: Option<String>,
    pub submitter_email: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub submitter_address: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    pub deleted_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::person::Entity")]
    Person,
    #[sea_orm(has_many = "super::family::Entity")]
    Family,
    #[sea_orm(has_many = "super::event::Entity")]
    Event,
    #[sea_orm(has_many = "super::place::Entity")]
    Place,
    #[sea_orm(has_many = "super::source::Entity")]
    Source,
    #[sea_orm(has_many = "super::media::Entity")]
    Media,
    #[sea_orm(has_many = "super::note::Entity")]
    Note,
}

impl Related<super::person::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Person.def()
    }
}

impl Related<super::family::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Family.def()
    }
}

impl Related<super::event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::place::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Place.def()
    }
}

impl Related<super::source::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Source.def()
    }
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl Related<super::note::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Note.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
