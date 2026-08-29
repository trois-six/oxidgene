//! Initial migration: create the full OxidGene schema in one shot.
//!
//! The project currently supports recreating its data from source imports, so
//! this migration is the complete current schema rather than an incremental
//! history of intermediate representations.

use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};
use sea_orm_migration::{prelude::*, schema::*};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 1. tree (root table, no FKs)
        manager
            .create_table(
                Table::create()
                    .table(Tree::Table)
                    .if_not_exists()
                    .col(uuid(Tree::Id).primary_key())
                    .col(string(Tree::Name))
                    .col(string_null(Tree::Description))
                    // No FK: person is created after tree and SQLite cannot
                    // add this cyclic foreign key after table creation.
                    .col(uuid_null(Tree::SosaRootPersonId))
                    .col(uuid_null(Tree::SelfPersonId))
                    .col(
                        ColumnDef::new(Tree::DefaultPrivacy)
                            .string_len(10)
                            .not_null()
                            .default("private"),
                    )
                    .col(timestamp_with_time_zone(Tree::CreatedAt))
                    .col(timestamp_with_time_zone(Tree::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Tree::DeletedAt))
                    .to_owned(),
            )
            .await?;

        // 2. person (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(Person::Table)
                    .if_not_exists()
                    .col(uuid(Person::Id).primary_key())
                    .col(uuid(Person::TreeId))
                    .col(string_len(Person::Sex, 10))
                    .col(
                        ColumnDef::new(Person::Privacy)
                            .string_len(10)
                            .not_null()
                            .default("default"),
                    )
                    .col(uuid_null(Person::PortraitMediaId))
                    .col(uuid_null(Person::PortraitVignetteId))
                    .col(timestamp_with_time_zone(Person::CreatedAt))
                    .col(timestamp_with_time_zone(Person::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Person::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_tree")
                            .from(Person::Table, Person::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_tree_id")
                    .table(Person::Table)
                    .col(Person::TreeId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_portrait_media_id")
                    .table(Person::Table)
                    .col(Person::PortraitMediaId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_portrait_vignette_id")
                    .table(Person::Table)
                    .col(Person::PortraitVignetteId)
                    .to_owned(),
            )
            .await?;

        // 3. person_name (FK → person)
        manager
            .create_table(
                Table::create()
                    .table(PersonName::Table)
                    .if_not_exists()
                    .col(uuid(PersonName::Id).primary_key())
                    .col(uuid(PersonName::PersonId))
                    .col(string_len(PersonName::NameType, 20))
                    .col(string_null(PersonName::GivenNames))
                    .col(string_null(PersonName::Surname))
                    .col(string_null(PersonName::SurnamePrefix))
                    .col(string_null(PersonName::Prefix))
                    .col(string_null(PersonName::Suffix))
                    .col(string_null(PersonName::Nickname))
                    .col(boolean(PersonName::IsPrimary))
                    .col(integer(PersonName::SortOrder).default(0))
                    .col(timestamp_with_time_zone(PersonName::CreatedAt))
                    .col(timestamp_with_time_zone(PersonName::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_name_person")
                            .from(PersonName::Table, PersonName::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_name_person_id")
                    .table(PersonName::Table)
                    .col(PersonName::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_name_surname")
                    .table(PersonName::Table)
                    .col(PersonName::Surname)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_name_given_names")
                    .table(PersonName::Table)
                    .col(PersonName::GivenNames)
                    .to_owned(),
            )
            .await?;

        // 4. family (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(Family::Table)
                    .if_not_exists()
                    .col(uuid(Family::Id).primary_key())
                    .col(uuid(Family::TreeId))
                    .col(
                        ColumnDef::new(Family::Privacy)
                            .string_len(10)
                            .not_null()
                            .default("default"),
                    )
                    .col(timestamp_with_time_zone(Family::CreatedAt))
                    .col(timestamp_with_time_zone(Family::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Family::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_family_tree")
                            .from(Family::Table, Family::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_family_tree_id")
                    .table(Family::Table)
                    .col(Family::TreeId)
                    .to_owned(),
            )
            .await?;

        // 5. family_spouse (FK → family, person)
        manager
            .create_table(
                Table::create()
                    .table(FamilySpouse::Table)
                    .if_not_exists()
                    .col(uuid(FamilySpouse::Id).primary_key())
                    .col(uuid(FamilySpouse::FamilyId))
                    .col(uuid(FamilySpouse::PersonId))
                    .col(string_len(FamilySpouse::Role, 10))
                    .col(integer(FamilySpouse::SortOrder))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_family_spouse_family")
                            .from(FamilySpouse::Table, FamilySpouse::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_family_spouse_person")
                            .from(FamilySpouse::Table, FamilySpouse::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_family_spouse_family_id")
                    .table(FamilySpouse::Table)
                    .col(FamilySpouse::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_family_spouse_person_id")
                    .table(FamilySpouse::Table)
                    .col(FamilySpouse::PersonId)
                    .to_owned(),
            )
            .await?;

        // 6. family_child (FK → family, person)
        manager
            .create_table(
                Table::create()
                    .table(FamilyChild::Table)
                    .if_not_exists()
                    .col(uuid(FamilyChild::Id).primary_key())
                    .col(uuid(FamilyChild::FamilyId))
                    .col(uuid(FamilyChild::PersonId))
                    .col(string_len(FamilyChild::ChildType, 15))
                    .col(integer(FamilyChild::SortOrder))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_family_child_family")
                            .from(FamilyChild::Table, FamilyChild::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_family_child_person")
                            .from(FamilyChild::Table, FamilyChild::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_family_child_family_id")
                    .table(FamilyChild::Table)
                    .col(FamilyChild::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_family_child_person_id")
                    .table(FamilyChild::Table)
                    .col(FamilyChild::PersonId)
                    .to_owned(),
            )
            .await?;

        // 7. place (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(Place::Table)
                    .if_not_exists()
                    .col(uuid(Place::Id).primary_key())
                    .col(uuid(Place::TreeId))
                    .col(string(Place::Name))
                    .col(double_null(Place::Latitude))
                    .col(double_null(Place::Longitude))
                    .col(timestamp_with_time_zone(Place::CreatedAt))
                    .col(timestamp_with_time_zone(Place::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_place_tree")
                            .from(Place::Table, Place::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_place_tree_id")
                    .table(Place::Table)
                    .col(Place::TreeId)
                    .to_owned(),
            )
            .await?;

        // 8. event (FK → tree, place?, person?, family?)
        manager
            .create_table(
                Table::create()
                    .table(Event::Table)
                    .if_not_exists()
                    .col(uuid(Event::Id).primary_key())
                    .col(uuid(Event::TreeId))
                    .col(string_len(Event::EventType, 25))
                    .col(string_null(Event::DateValue))
                    .col(date_null(Event::DateSort))
                    .col(
                        ColumnDef::new(Event::DateQualifier)
                            .string_len(10)
                            .not_null()
                            .default("exact"),
                    )
                    .col(string_null(Event::DateValue2))
                    .col(
                        ColumnDef::new(Event::Calendar)
                            .string_len(20)
                            .not_null()
                            .default("gregorian"),
                    )
                    .col(string_null(Event::Cause))
                    .col(uuid_null(Event::PlaceId))
                    .col(uuid_null(Event::PersonId))
                    .col(uuid_null(Event::FamilyId))
                    .col(string_null(Event::Description))
                    .col(timestamp_with_time_zone(Event::CreatedAt))
                    .col(timestamp_with_time_zone(Event::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Event::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_tree")
                            .from(Event::Table, Event::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_place")
                            .from(Event::Table, Event::PlaceId)
                            .to(Place::Table, Place::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_person")
                            .from(Event::Table, Event::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_family")
                            .from(Event::Table, Event::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_tree_id")
                    .table(Event::Table)
                    .col(Event::TreeId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_person_id")
                    .table(Event::Table)
                    .col(Event::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_family_id")
                    .table(Event::Table)
                    .col(Event::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_date_sort")
                    .table(Event::Table)
                    .col(Event::DateSort)
                    .to_owned(),
            )
            .await?;

        // 9. event_witness (FK → event, person)
        manager
            .create_table(
                Table::create()
                    .table(EventWitness::Table)
                    .if_not_exists()
                    .col(uuid(EventWitness::Id).primary_key())
                    .col(uuid(EventWitness::EventId))
                    .col(uuid(EventWitness::PersonId))
                    .col(string_null(EventWitness::Relation))
                    .col(integer(EventWitness::SortOrder))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_witness_event")
                            .from(EventWitness::Table, EventWitness::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_event_witness_person")
                            .from(EventWitness::Table, EventWitness::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_witness_event_id")
                    .table(EventWitness::Table)
                    .col(EventWitness::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_event_witness_person_id")
                    .table(EventWitness::Table)
                    .col(EventWitness::PersonId)
                    .to_owned(),
            )
            .await?;

        // 10. source (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(Source::Table)
                    .if_not_exists()
                    .col(uuid(Source::Id).primary_key())
                    .col(uuid(Source::TreeId))
                    .col(string(Source::Title))
                    .col(string_null(Source::Author))
                    .col(string_null(Source::Publisher))
                    .col(string_null(Source::Abbreviation))
                    .col(string_null(Source::RepositoryName))
                    .col(timestamp_with_time_zone(Source::CreatedAt))
                    .col(timestamp_with_time_zone(Source::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Source::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_source_tree")
                            .from(Source::Table, Source::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_source_tree_id")
                    .table(Source::Table)
                    .col(Source::TreeId)
                    .to_owned(),
            )
            .await?;

        // 11. citation (FK → source, person?, event?, family?)
        manager
            .create_table(
                Table::create()
                    .table(Citation::Table)
                    .if_not_exists()
                    .col(uuid(Citation::Id).primary_key())
                    .col(uuid(Citation::SourceId))
                    .col(uuid_null(Citation::PersonId))
                    .col(uuid_null(Citation::EventId))
                    .col(uuid_null(Citation::FamilyId))
                    .col(string_null(Citation::Page))
                    .col(string_len(Citation::Confidence, 10))
                    .col(text_null(Citation::Text))
                    .col(timestamp_with_time_zone(Citation::CreatedAt))
                    .col(timestamp_with_time_zone(Citation::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_citation_source")
                            .from(Citation::Table, Citation::SourceId)
                            .to(Source::Table, Source::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_citation_person")
                            .from(Citation::Table, Citation::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_citation_event")
                            .from(Citation::Table, Citation::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_citation_family")
                            .from(Citation::Table, Citation::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_source_id")
                    .table(Citation::Table)
                    .col(Citation::SourceId)
                    .to_owned(),
            )
            .await?;

        // 12. media (FK → tree; place? enforced at ORM layer only, see note below)
        manager
            .create_table(
                Table::create()
                    .table(Media::Table)
                    .if_not_exists()
                    .col(uuid(Media::Id).primary_key())
                    .col(uuid(Media::TreeId))
                    .col(string(Media::FileName))
                    .col(string(Media::MimeType))
                    .col(string(Media::FilePath))
                    .col(string_null(Media::StorageKey))
                    .col(string_null(Media::Sha256))
                    .col(string_null(Media::ThumbnailKey))
                    .col(integer_null(Media::Width))
                    .col(integer_null(Media::Height))
                    .col(integer(Media::PageCount).default(1))
                    .col(uuid_null(Media::ParentMediaId))
                    .col(integer(Media::PageIndex).default(0))
                    .col(boolean(Media::IsDocument).default(false))
                    .col(big_integer(Media::FileSize))
                    .col(string_null(Media::Title))
                    .col(string_null(Media::Description))
                    .col(string_null(Media::DateValue))
                    .col(date_null(Media::DateSort))
                    .col(
                        ColumnDef::new(Media::DateQualifier)
                            .string_len(10)
                            .not_null()
                            .default("exact"),
                    )
                    .col(string_null(Media::DateValue2))
                    .col(
                        ColumnDef::new(Media::Calendar)
                            .string_len(20)
                            .not_null()
                            .default("gregorian"),
                    )
                    .col(
                        ColumnDef::new(Media::SourceMediaType)
                            .string_len(20)
                            .not_null()
                            .default("other"),
                    )
                    .col(string_null(Media::DocumentCategory))
                    // No FK: kept consistent with the original ALTER TABLE
                    // ADD COLUMN, which SQLite can't attach a FK to either.
                    .col(uuid_null(Media::PlaceId))
                    .col(
                        ColumnDef::new(Media::Privacy)
                            .string_len(10)
                            .not_null()
                            .default("default"),
                    )
                    .col(timestamp_with_time_zone(Media::CreatedAt))
                    .col(timestamp_with_time_zone(Media::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Media::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_tree")
                            .from(Media::Table, Media::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_tree_sha256")
                    .table(Media::Table)
                    .col(Media::TreeId)
                    .col(Media::Sha256)
                    .to_owned(),
            )
            .await?;
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
        manager
            .create_index(
                Index::create()
                    .name("idx_media_tree_id")
                    .table(Media::Table)
                    .col(Media::TreeId)
                    .to_owned(),
            )
            .await?;

        // 13. media_link (FK → media, person?, event?, source?, family?)
        manager
            .create_table(
                Table::create()
                    .table(MediaLink::Table)
                    .if_not_exists()
                    .col(uuid(MediaLink::Id).primary_key())
                    .col(uuid(MediaLink::MediaId))
                    .col(uuid_null(MediaLink::PersonId))
                    .col(uuid_null(MediaLink::EventId))
                    .col(uuid_null(MediaLink::SourceId))
                    .col(uuid_null(MediaLink::FamilyId))
                    .col(integer(MediaLink::SortOrder))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_link_media")
                            .from(MediaLink::Table, MediaLink::MediaId)
                            .to(Media::Table, Media::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_link_person")
                            .from(MediaLink::Table, MediaLink::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_link_event")
                            .from(MediaLink::Table, MediaLink::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_link_source")
                            .from(MediaLink::Table, MediaLink::SourceId)
                            .to(Source::Table, Source::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_link_family")
                            .from(MediaLink::Table, MediaLink::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_media_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::MediaId)
                    .to_owned(),
            )
            .await?;

        // 14. note (FK → tree, person?, event?, family?, source?)
        manager
            .create_table(
                Table::create()
                    .table(Note::Table)
                    .if_not_exists()
                    .col(uuid(Note::Id).primary_key())
                    .col(uuid(Note::TreeId))
                    .col(text(Note::Text))
                    .col(uuid_null(Note::PersonId))
                    .col(uuid_null(Note::EventId))
                    .col(uuid_null(Note::FamilyId))
                    .col(uuid_null(Note::SourceId))
                    .col(uuid_null(Note::MediaId))
                    .col(timestamp_with_time_zone(Note::CreatedAt))
                    .col(timestamp_with_time_zone(Note::UpdatedAt))
                    .col(timestamp_with_time_zone_null(Note::DeletedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_note_tree")
                            .from(Note::Table, Note::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_note_person")
                            .from(Note::Table, Note::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_note_event")
                            .from(Note::Table, Note::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_note_family")
                            .from(Note::Table, Note::FamilyId)
                            .to(Family::Table, Family::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_note_source")
                            .from(Note::Table, Note::SourceId)
                            .to(Source::Table, Source::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_tree_id")
                    .table(Note::Table)
                    .col(Note::TreeId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_media_id")
                    .table(Note::Table)
                    .col(Note::MediaId)
                    .to_owned(),
            )
            .await?;

        // 15. vignette (FK → media, person?, event?)
        manager
            .create_table(
                Table::create()
                    .table(Vignette::Table)
                    .if_not_exists()
                    .col(uuid(Vignette::Id).primary_key())
                    .col(uuid(Vignette::MediaId))
                    .col(integer(Vignette::Page).default(0))
                    .col(integer(Vignette::X))
                    .col(integer(Vignette::Y))
                    .col(integer(Vignette::Width))
                    .col(integer(Vignette::Height))
                    .col(uuid_null(Vignette::PersonId))
                    .col(uuid_null(Vignette::EventId))
                    .col(timestamp_with_time_zone(Vignette::CreatedAt))
                    .col(timestamp_with_time_zone(Vignette::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_media")
                            .from(Vignette::Table, Vignette::MediaId)
                            .to(Media::Table, Media::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_person")
                            .from(Vignette::Table, Vignette::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_vignette_event")
                            .from(Vignette::Table, Vignette::EventId)
                            .to(Event::Table, Event::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        for (name, column) in [
            ("idx_vignette_media_id", Vignette::MediaId),
            ("idx_vignette_person_id", Vignette::PersonId),
            ("idx_vignette_event_id", Vignette::EventId),
        ] {
            manager
                .create_index(
                    Index::create()
                        .name(name)
                        .table(Vignette::Table)
                        .col(column)
                        .to_owned(),
                )
                .await?;
        }

        // 16. media_tag (FK → media)
        manager
            .create_table(
                Table::create()
                    .table(MediaTag::Table)
                    .if_not_exists()
                    .col(uuid(MediaTag::MediaId))
                    .col(string(MediaTag::NormalizedTag))
                    .col(string(MediaTag::Tag))
                    .col(timestamp_with_time_zone(MediaTag::CreatedAt))
                    .primary_key(
                        Index::create()
                            .col(MediaTag::MediaId)
                            .col(MediaTag::NormalizedTag),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_media_tag_media")
                            .from(MediaTag::Table, MediaTag::MediaId)
                            .to(Media::Table, Media::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // 17. background_job (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(BackgroundJob::Table)
                    .if_not_exists()
                    .col(uuid(BackgroundJob::Id).primary_key())
                    .col(uuid(BackgroundJob::TreeId))
                    .col(uuid_null(BackgroundJob::ActiveTreeId))
                    .col(string_len(BackgroundJob::Kind, 16))
                    .col(string_len(BackgroundJob::Format, 16))
                    .col(string_len(BackgroundJob::Status, 16))
                    .col(string_len(BackgroundJob::Phase, 32))
                    .col(string_null(BackgroundJob::SourceKey))
                    .col(string_null(BackgroundJob::ArtifactKey))
                    .col(text_null(BackgroundJob::PayloadJson))
                    .col(string_null(BackgroundJob::OriginalFilename))
                    .col(boolean(BackgroundJob::MergeOccupations).default(false))
                    .col(boolean(BackgroundJob::MergeNames).default(false))
                    .col(big_integer(BackgroundJob::Done).default(0))
                    .col(big_integer(BackgroundJob::Total).default(0))
                    .col(integer(BackgroundJob::Attempt).default(0))
                    .col(string_null(BackgroundJob::LeaseOwner))
                    .col(timestamp_with_time_zone_null(BackgroundJob::LeaseUntil))
                    .col(boolean(BackgroundJob::CancelRequested).default(false))
                    .col(text_null(BackgroundJob::ResultJson))
                    .col(string_null(BackgroundJob::ErrorCode))
                    .col(timestamp_with_time_zone(BackgroundJob::CreatedAt))
                    .col(timestamp_with_time_zone(BackgroundJob::UpdatedAt))
                    .col(timestamp_with_time_zone_null(BackgroundJob::StartedAt))
                    .col(timestamp_with_time_zone_null(BackgroundJob::FinishedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_background_job_tree")
                            .from(BackgroundJob::Table, BackgroundJob::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_background_job_active_tree")
                            .from(BackgroundJob::Table, BackgroundJob::ActiveTreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_background_job_active_tree")
                    .table(BackgroundJob::Table)
                    .col(BackgroundJob::ActiveTreeId)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_background_job_claim")
                    .table(BackgroundJob::Table)
                    .col(BackgroundJob::Status)
                    .col(BackgroundJob::LeaseUntil)
                    .col(BackgroundJob::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 18. person_search_fts (Sprint E.6): a real FTS5 virtual table on
        // SQLite (desktop), or a plain table + index on PostgreSQL (web,
        // where FTS5 isn't available — matching falls back to LIKE on
        // pre-normalized token columns computed in Rust before insert).
        let conn = manager.get_connection();
        match manager.get_database_backend() {
            DbBackend::Sqlite => {
                conn.execute_raw(Statement::from_string(
                    DbBackend::Sqlite,
                    r#"
                    CREATE VIRTUAL TABLE IF NOT EXISTS person_search_fts USING fts5(
                        surname,
                        given_names,
                        maiden_name,
                        birth_year,
                        death_year,
                        person_id UNINDEXED,
                        tree_id UNINDEXED,
                        sex UNINDEXED,
                        display_name UNINDEXED,
                        surname_display UNINDEXED,
                        given_names_display UNINDEXED,
                        birth_place UNINDEXED,
                        date_sort UNINDEXED
                    )
                    "#
                    .to_owned(),
                ))
                .await?;
            }
            backend => {
                conn.execute_raw(Statement::from_string(
                    backend,
                    r#"
                    CREATE TABLE IF NOT EXISTS person_search_fts (
                        person_id TEXT NOT NULL PRIMARY KEY,
                        tree_id TEXT NOT NULL,
                        surname TEXT NOT NULL DEFAULT '',
                        given_names TEXT NOT NULL DEFAULT '',
                        maiden_name TEXT,
                        birth_year TEXT,
                        death_year TEXT,
                        sex TEXT NOT NULL DEFAULT 'unknown',
                        display_name TEXT NOT NULL DEFAULT '',
                        surname_display TEXT NOT NULL DEFAULT '',
                        given_names_display TEXT NOT NULL DEFAULT '',
                        birth_place TEXT,
                        date_sort TEXT
                    )
                    "#
                    .to_owned(),
                ))
                .await?;
                conn.execute_raw(Statement::from_string(
                    backend,
                    "CREATE INDEX IF NOT EXISTS idx_person_search_fts_tree_id \
                     ON person_search_fts (tree_id)"
                        .to_owned(),
                ))
                .await?;
            }
        }

        // Supporting indexes for relationship traversal.
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_person_id")
                    .table(Citation::Table)
                    .col(Citation::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_event_id")
                    .table(Citation::Table)
                    .col(Citation::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_citation_family_id")
                    .table(Citation::Table)
                    .col(Citation::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_person_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_event_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_source_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::SourceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_media_link_family_id")
                    .table(MediaLink::Table)
                    .col(MediaLink::FamilyId)
                    .to_owned(),
            )
            .await?;

        // Durable person projections.
        manager
            .create_table(
                Table::create()
                    .table(PersonDenorm::Table)
                    .if_not_exists()
                    .col(uuid(PersonDenorm::PersonId).primary_key())
                    .col(uuid(PersonDenorm::TreeId))
                    .col(text(PersonDenorm::Payload))
                    .col(integer(PersonDenorm::SchemaVersion).default(0))
                    .col(timestamp_with_time_zone(PersonDenorm::UpdatedAt))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_denorm_person")
                            .from(PersonDenorm::Table, PersonDenorm::PersonId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_denorm_tree")
                            .from(PersonDenorm::Table, PersonDenorm::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_denorm_tree_schema_version")
                    .table(PersonDenorm::Table)
                    .col(PersonDenorm::TreeId)
                    .col(PersonDenorm::SchemaVersion)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_person_denorm_tree_id")
                    .table(PersonDenorm::Table)
                    .col(PersonDenorm::TreeId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_event_place_id")
                    .table(Event::Table)
                    .col(Event::PlaceId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_person_id")
                    .table(Note::Table)
                    .col(Note::PersonId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_event_id")
                    .table(Note::Table)
                    .col(Note::EventId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_family_id")
                    .table(Note::Table)
                    .col(Note::FamilyId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_note_source_id")
                    .table(Note::Table)
                    .col(Note::SourceId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        conn.execute_raw(Statement::from_string(
            manager.get_database_backend(),
            "DROP TABLE IF EXISTS person_search_fts".to_owned(),
        ))
        .await?;

        // Drop in reverse dependency order. `person_denorm` leads: it is a
        // projection with foreign keys onto `person` and `tree`, so it has to
        // go before either of them.
        let tables = [
            BackgroundJob::Table.into_table_ref(),
            PersonDenorm::Table.into_table_ref(),
            MediaTag::Table.into_table_ref(),
            Vignette::Table.into_table_ref(),
            Note::Table.into_table_ref(),
            MediaLink::Table.into_table_ref(),
            Media::Table.into_table_ref(),
            Citation::Table.into_table_ref(),
            Source::Table.into_table_ref(),
            EventWitness::Table.into_table_ref(),
            Event::Table.into_table_ref(),
            Place::Table.into_table_ref(),
            FamilyChild::Table.into_table_ref(),
            FamilySpouse::Table.into_table_ref(),
            Family::Table.into_table_ref(),
            PersonName::Table.into_table_ref(),
            Person::Table.into_table_ref(),
            Tree::Table.into_table_ref(),
        ];
        for table in tables {
            manager
                .drop_table(Table::drop().table(table).if_exists().to_owned())
                .await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Iden enums for table/column names
// ---------------------------------------------------------------------------

#[derive(DeriveIden)]
enum Tree {
    Table,
    Id,
    Name,
    Description,
    SosaRootPersonId,
    SelfPersonId,
    DefaultPrivacy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum Person {
    Table,
    Id,
    TreeId,
    Sex,
    Privacy,
    PortraitMediaId,
    PortraitVignetteId,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum PersonName {
    Table,
    SurnamePrefix,
    SortOrder,
    Id,
    PersonId,
    NameType,
    GivenNames,
    Surname,
    Prefix,
    Suffix,
    Nickname,
    IsPrimary,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Family {
    Table,
    Id,
    TreeId,
    Privacy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum FamilySpouse {
    Table,
    Id,
    FamilyId,
    PersonId,
    Role,
    SortOrder,
}

#[derive(DeriveIden)]
enum FamilyChild {
    Table,
    Id,
    FamilyId,
    PersonId,
    ChildType,
    SortOrder,
}

#[derive(DeriveIden)]
enum Place {
    Table,
    Id,
    TreeId,
    Name,
    Latitude,
    Longitude,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
#[allow(clippy::enum_variant_names)]
enum Event {
    Table,
    Id,
    TreeId,
    EventType,
    DateValue,
    DateSort,
    DateQualifier,
    DateValue2,
    Calendar,
    Cause,
    PlaceId,
    PersonId,
    FamilyId,
    Description,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum EventWitness {
    Table,
    Id,
    EventId,
    PersonId,
    Relation,
    SortOrder,
}

#[derive(DeriveIden)]
enum Source {
    Table,
    Id,
    TreeId,
    Title,
    Author,
    Publisher,
    Abbreviation,
    RepositoryName,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum Citation {
    Table,
    Id,
    SourceId,
    PersonId,
    EventId,
    FamilyId,
    Page,
    Confidence,
    Text,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    TreeId,
    FileName,
    MimeType,
    FilePath,
    StorageKey,
    Sha256,
    ThumbnailKey,
    Width,
    Height,
    PageCount,
    ParentMediaId,
    PageIndex,
    IsDocument,
    FileSize,
    Title,
    Description,
    DateValue,
    DateSort,
    DateQualifier,
    DateValue2,
    Calendar,
    SourceMediaType,
    DocumentCategory,
    PlaceId,
    Privacy,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum MediaLink {
    Table,
    Id,
    MediaId,
    PersonId,
    EventId,
    SourceId,
    FamilyId,
    SortOrder,
}

#[derive(DeriveIden)]
enum Note {
    Table,
    Id,
    TreeId,
    Text,
    PersonId,
    EventId,
    FamilyId,
    SourceId,
    MediaId,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum PersonDenorm {
    Table,
    PersonId,
    TreeId,
    Payload,
    SchemaVersion,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Vignette {
    Table,
    Id,
    MediaId,
    Page,
    X,
    Y,
    Width,
    Height,
    PersonId,
    EventId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum MediaTag {
    Table,
    MediaId,
    NormalizedTag,
    Tag,
    CreatedAt,
}

#[derive(DeriveIden)]
enum BackgroundJob {
    Table,
    Id,
    TreeId,
    ActiveTreeId,
    Kind,
    Format,
    Status,
    Phase,
    SourceKey,
    ArtifactKey,
    PayloadJson,
    OriginalFilename,
    MergeOccupations,
    MergeNames,
    Done,
    Total,
    Attempt,
    LeaseOwner,
    LeaseUntil,
    CancelRequested,
    ResultJson,
    ErrorCode,
    CreatedAt,
    UpdatedAt,
    StartedAt,
    FinishedAt,
}
