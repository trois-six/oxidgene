//! Initial migration: create all 14 tables for OxidGene.

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
                    .col(string_null(PersonName::Prefix))
                    .col(string_null(PersonName::Suffix))
                    .col(string_null(PersonName::Nickname))
                    .col(boolean(PersonName::IsPrimary))
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

        // 4. family (FK → tree)
        manager
            .create_table(
                Table::create()
                    .table(Family::Table)
                    .if_not_exists()
                    .col(uuid(Family::Id).primary_key())
                    .col(uuid(Family::TreeId))
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

        // 9. source (FK → tree)
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

        // 10. citation (FK → source, person?, event?, family?)
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

        // 11. media (FK → tree)
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
                    .col(big_integer(Media::FileSize))
                    .col(string_null(Media::Title))
                    .col(string_null(Media::Description))
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
                    .name("idx_media_tree_id")
                    .table(Media::Table)
                    .col(Media::TreeId)
                    .to_owned(),
            )
            .await?;

        // 12. media_link (FK → media, person?, event?, source?, family?)
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

        // 13. note (FK → tree, person?, event?, family?, source?)
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

        // 14. person_ancestry (closure table, FK → tree, person x2)
        manager
            .create_table(
                Table::create()
                    .table(PersonAncestry::Table)
                    .if_not_exists()
                    .col(uuid(PersonAncestry::Id).primary_key())
                    .col(uuid(PersonAncestry::TreeId))
                    .col(uuid(PersonAncestry::AncestorId))
                    .col(uuid(PersonAncestry::DescendantId))
                    .col(integer(PersonAncestry::Depth))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_ancestry_tree")
                            .from(PersonAncestry::Table, PersonAncestry::TreeId)
                            .to(Tree::Table, Tree::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_ancestry_ancestor")
                            .from(PersonAncestry::Table, PersonAncestry::AncestorId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_person_ancestry_descendant")
                            .from(PersonAncestry::Table, PersonAncestry::DescendantId)
                            .to(Person::Table, Person::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_ancestry_ancestor")
                    .table(PersonAncestry::Table)
                    .col(PersonAncestry::AncestorId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_ancestry_descendant")
                    .table(PersonAncestry::Table)
                    .col(PersonAncestry::DescendantId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_person_ancestry_tree_desc_depth")
                    .table(PersonAncestry::Table)
                    .col(PersonAncestry::TreeId)
                    .col(PersonAncestry::DescendantId)
                    .col(PersonAncestry::Depth)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop in reverse dependency order.
        let tables = [
            PersonAncestry::Table.into_table_ref(),
            Note::Table.into_table_ref(),
            MediaLink::Table.into_table_ref(),
            Media::Table.into_table_ref(),
            Citation::Table.into_table_ref(),
            Source::Table.into_table_ref(),
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
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum PersonName {
    Table,
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
    PlaceId,
    PersonId,
    FamilyId,
    Description,
    CreatedAt,
    UpdatedAt,
    DeletedAt,
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
    FileSize,
    Title,
    Description,
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
    CreatedAt,
    UpdatedAt,
    DeletedAt,
}

#[derive(DeriveIden)]
enum PersonAncestry {
    Table,
    Id,
    TreeId,
    AncestorId,
    DescendantId,
    Depth,
}
