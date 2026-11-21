//! The rules the schema itself holds (docs/data-model.md, Integrity
//! constraints), on SQLite and, under `just test-postgres`, PostgreSQL.
//!
//! The first half writes rows through the entities, past the repositories'
//! own checks, and expects the database to refuse each broken one; the
//! second half expects the repositories to refuse or correct the same
//! writes before the database sees them. All data is fictitious.

use chrono::Utc;
use oxidgene_core::ChildType;
use oxidgene_core::enums::{Calendar, DateQualifier, EventType, NameType, Sex, SpouseRole};
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::entities::{
    citation, event, event_witness, family_child, family_spouse, media_link, note, person,
    person_name, sea_enums,
};
use oxidgene_db::repo::{
    CitationRepo, EventDetails, EventRepo, EventWitnessRepo, FamilyChildRepo, FamilyRepo,
    FamilySpouseRepo, MediaLinkRepo, MediaRepo, NoteFilter, NoteRepo, PersonNamePieces,
    PersonNamePiecesPatch, PersonNameRepo, PersonRepo, SourceRepo, TreeRepo,
};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::common::setup_db;

/// A tree with what the rules are about: two persons, a family, an event of
/// the first person, an event of the family, a source and a document.
struct Fixture {
    db: DatabaseConnection,
    tree: Uuid,
    person: Uuid,
    other: Uuid,
    family: Uuid,
    event: Uuid,
    family_event: Uuid,
    source: Uuid,
    media: Uuid,
}

async fn fixture() -> Fixture {
    let db = setup_db().await;
    let ids: [Uuid; 8] = std::array::from_fn(|_| Uuid::now_v7());
    let [
        tree,
        person,
        other,
        family,
        event,
        family_event,
        source,
        media,
    ] = ids;
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    for id in [person, other] {
        PersonRepo::create(&db, id, tree, Sex::Unknown)
            .await
            .unwrap();
    }
    FamilyRepo::create(&db, family, tree).await.unwrap();
    for (id, person_id, family_id) in [
        (event, Some(person), None),
        (family_event, None, Some(family)),
    ] {
        EventRepo::create(
            &db,
            id,
            tree,
            EventType::Other,
            None,
            None,
            None,
            person_id,
            family_id,
            None,
            DateQualifier::Exact,
            None,
            Calendar::Gregorian,
            EventDetails::default(),
        )
        .await
        .unwrap();
    }
    SourceRepo::create(
        &db,
        source,
        tree,
        "Sample register".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    MediaRepo::create_document(&db, media, tree, Some("Sample".into()), Utc::now())
        .await
        .unwrap();
    Fixture {
        db,
        tree,
        person,
        other,
        family,
        event,
        family_event,
        source,
        media,
    }
}

impl Fixture {
    fn spouse(&self, person_id: Uuid, role: sea_enums::SpouseRole) -> family_spouse::ActiveModel {
        family_spouse::ActiveModel {
            id: Set(Uuid::now_v7()),
            family_id: Set(self.family),
            person_id: Set(person_id),
            role: Set(role),
            sort_order: Set(0),
        }
    }

    fn child(&self, child_type: sea_enums::ChildType) -> family_child::ActiveModel {
        family_child::ActiveModel {
            id: Set(Uuid::now_v7()),
            family_id: Set(self.family),
            person_id: Set(self.other),
            child_type: Set(child_type),
            sort_order: Set(0),
        }
    }

    fn witness(&self, relation: Option<&str>) -> event_witness::ActiveModel {
        event_witness::ActiveModel {
            id: Set(Uuid::now_v7()),
            event_id: Set(self.event),
            person_id: Set(self.other),
            relation: Set(relation.map(str::to_owned)),
            sort_order: Set(0),
        }
    }

    fn name(&self, is_primary: bool) -> person_name::ActiveModel {
        person_name::ActiveModel {
            id: Set(Uuid::now_v7()),
            person_id: Set(self.person),
            name_type: Set(sea_enums::NameType::Birth),
            given_names: Set(Some("Sample".into())),
            surname: Set(Some("Fixture".into())),
            surname_prefix: Set(None),
            prefix: Set(None),
            suffix: Set(None),
            nickname: Set(None),
            is_primary: Set(is_primary),
            sort_order: Set(0),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        }
    }

    fn citation(
        &self,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
    ) -> citation::ActiveModel {
        citation::ActiveModel {
            id: Set(Uuid::now_v7()),
            source_id: Set(self.source),
            person_id: Set(person_id),
            event_id: Set(event_id),
            family_id: Set(family_id),
            page: Set(None),
            confidence: Set(None),
            text: Set(None),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        }
    }

    fn media_link(
        &self,
        person_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> media_link::ActiveModel {
        media_link::ActiveModel {
            id: Set(Uuid::now_v7()),
            media_id: Set(self.media),
            person_id: Set(person_id),
            event_id: Set(None),
            source_id: Set(source_id),
            family_id: Set(None),
            sort_order: Set(0),
        }
    }

    fn note(&self, person_id: Option<Uuid>, media_id: Option<Uuid>) -> note::ActiveModel {
        note::ActiveModel {
            id: Set(Uuid::now_v7()),
            tree_id: Set(self.tree),
            text: Set("Sample note".into()),
            person_id: Set(person_id),
            event_id: Set(None),
            family_id: Set(None),
            source_id: Set(None),
            media_id: Set(media_id),
            repository_id: Set(None),
            shared_group: Set(None),
            confidential: Set(false),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
            deleted_at: Set(None),
        }
    }

    /// Set column `column` of the first person to `value`, as SQL.
    async fn set_person(&self, column: person::Column, value: Expr) -> Result<(), sea_orm::DbErr> {
        person::Entity::update_many()
            .col_expr(column, value)
            .filter(person::Column::Id.eq(self.person))
            .exec(&self.db)
            .await
            .map(drop)
    }

    /// Set column `column` of event `id` to `value`, as SQL.
    async fn set_event(
        &self,
        id: Uuid,
        column: event::Column,
        value: Expr,
    ) -> Result<(), sea_orm::DbErr> {
        event::Entity::update_many()
            .col_expr(column, value)
            .filter(event::Column::Id.eq(id))
            .exec(&self.db)
            .await
            .map(drop)
    }
}

#[tokio::test]
async fn a_family_holds_a_person_once_as_a_spouse_and_once_as_a_child() {
    let f = fixture().await;
    f.spouse(f.person, sea_enums::SpouseRole::Husband)
        .insert(&f.db)
        .await
        .expect("a first spouse link");
    assert!(
        f.spouse(f.person, sea_enums::SpouseRole::Partner)
            .insert(&f.db)
            .await
            .is_err(),
        "the same person again, in another role"
    );
    f.spouse(f.other, sea_enums::SpouseRole::Wife)
        .insert(&f.db)
        .await
        .expect("another person");

    f.child(sea_enums::ChildType::Biological)
        .insert(&f.db)
        .await
        .expect("a first child link");
    assert!(
        f.child(sea_enums::ChildType::Adopted)
            .insert(&f.db)
            .await
            .is_err(),
        "the same child again"
    );
}

#[tokio::test]
async fn an_event_holds_a_witness_once_per_relation() {
    let f = fixture().await;
    for relation in [Some("Godfather"), Some("Witness"), None] {
        f.witness(relation)
            .insert(&f.db)
            .await
            .expect("one witness per relation");
    }
    for relation in [Some("Godfather"), None] {
        assert!(
            f.witness(relation).insert(&f.db).await.is_err(),
            "{relation:?} again"
        );
    }
}

#[tokio::test]
async fn a_person_has_one_primary_name_and_one_portrait() {
    let f = fixture().await;
    f.name(true).insert(&f.db).await.expect("a primary name");
    f.name(false).insert(&f.db).await.expect("a secondary name");
    f.name(false).insert(&f.db).await.expect("another");
    assert!(
        f.name(true).insert(&f.db).await.is_err(),
        "a second primary name"
    );

    f.set_person(person::Column::PortraitMediaId, Expr::value(f.media))
        .await
        .expect("a whole medium as portrait");
    assert!(
        f.set_person(person::Column::PortraitVignetteId, Expr::value(f.media))
            .await
            .is_err(),
        "a crop beside it"
    );
}

#[tokio::test]
async fn an_event_has_one_owner_at_most() {
    let f = fixture().await;
    assert!(
        f.set_event(f.event, event::Column::FamilyId, Expr::value(f.family))
            .await
            .is_err(),
        "a person's event made a family's too"
    );
    assert!(
        f.set_event(
            f.family_event,
            event::Column::RelatedFamilyId,
            Expr::value(f.family)
        )
        .await
        .is_err(),
        "a family event naming a related family"
    );
    f.set_event(
        f.event,
        event::Column::RelatedFamilyId,
        Expr::value(f.family),
    )
    .await
    .expect("an individual event names a family it does not belong to");
    // An event may have no owner, but then names no related family.
    assert!(
        f.set_event(
            f.event,
            event::Column::PersonId,
            Expr::value(Option::<Uuid>::None)
        )
        .await
        .is_err(),
        "the owner removed from an event naming a related family"
    );
}

#[tokio::test]
async fn a_citation_a_media_link_and_a_note_name_exactly_one_record() {
    let f = fixture().await;
    assert!(f.citation(None, None, None).insert(&f.db).await.is_err());
    assert!(
        f.citation(None, Some(f.family_event), Some(f.family))
            .insert(&f.db)
            .await
            .is_err()
    );
    f.citation(None, Some(f.family_event), None)
        .insert(&f.db)
        .await
        .expect("a citation of one event");

    assert!(f.media_link(None, None).insert(&f.db).await.is_err());
    assert!(
        f.media_link(Some(f.person), Some(f.source))
            .insert(&f.db)
            .await
            .is_err()
    );
    f.media_link(Some(f.person), None)
        .insert(&f.db)
        .await
        .expect("a link to one person");

    assert!(f.note(None, None).insert(&f.db).await.is_err());
    assert!(
        f.note(Some(f.person), Some(f.media))
            .insert(&f.db)
            .await
            .is_err()
    );
    f.note(None, Some(f.media))
        .insert(&f.db)
        .await
        .expect("a note about one medium");
}

#[tokio::test]
async fn an_enumerated_column_refuses_a_value_its_enum_lacks() {
    let f = fixture().await;
    assert!(
        f.set_person(person::Column::Sex, Expr::value("robot"))
            .await
            .is_err()
    );
    assert!(
        f.set_person(person::Column::Privacy, Expr::value("secret"))
            .await
            .is_err()
    );
    for (column, value) in [
        (event::Column::EventType, "coronation"),
        (event::Column::DateQualifier, "roughly"),
        (event::Column::Calendar, "lunar"),
    ] {
        assert!(
            f.set_event(f.event, column, Expr::value(value))
                .await
                .is_err(),
            "{value}"
        );
    }
    // Every value of an enum is accepted: the lists are the enums' own.
    for sex in ["male", "female", "unknown"] {
        f.set_person(person::Column::Sex, Expr::value(sex))
            .await
            .expect(sex);
    }
}

#[tokio::test]
async fn the_repositories_refuse_a_repeated_link_as_a_conflict() {
    let f = fixture().await;
    let conflict = |result: Result<(), OxidGeneError>| {
        assert!(
            matches!(result, Err(OxidGeneError::Conflict(_))),
            "{result:?}"
        );
    };
    FamilySpouseRepo::create(
        &f.db,
        Uuid::now_v7(),
        f.family,
        f.person,
        SpouseRole::Husband,
        0,
    )
    .await
    .unwrap();
    conflict(
        FamilySpouseRepo::create(
            &f.db,
            Uuid::now_v7(),
            f.family,
            f.person,
            SpouseRole::Partner,
            1,
        )
        .await
        .map(drop),
    );
    FamilyChildRepo::create(
        &f.db,
        Uuid::now_v7(),
        f.family,
        f.other,
        ChildType::Biological,
        0,
    )
    .await
    .unwrap();
    conflict(
        FamilyChildRepo::create(&f.db, Uuid::now_v7(), f.family, f.other, ChildType::Step, 1)
            .await
            .map(drop),
    );
    for relation in [Some("Godmother"), Some("Witness"), None] {
        EventWitnessRepo::create(
            &f.db,
            Uuid::now_v7(),
            f.event,
            f.other,
            relation.map(str::to_owned),
            0,
        )
        .await
        .unwrap();
    }
    for relation in [Some("Godmother"), None, Some("")] {
        conflict(
            EventWitnessRepo::create(
                &f.db,
                Uuid::now_v7(),
                f.event,
                f.other,
                relation.map(str::to_owned),
                0,
            )
            .await
            .map(drop),
        );
    }
}

#[tokio::test]
async fn the_repositories_refuse_a_row_naming_no_record_or_several() {
    let f = fixture().await;
    let invalid = |result: Result<(), OxidGeneError>| {
        assert!(
            matches!(result, Err(OxidGeneError::Validation(_))),
            "{result:?}"
        );
    };
    for (person_id, event_id) in [(None, None), (Some(f.person), Some(f.event))] {
        invalid(
            CitationRepo::create(
                &f.db,
                Uuid::now_v7(),
                f.source,
                person_id,
                event_id,
                None,
                None,
                None,
                None,
            )
            .await
            .map(drop),
        );
        invalid(
            MediaLinkRepo::create(
                &f.db,
                Uuid::now_v7(),
                f.media,
                person_id,
                event_id,
                None,
                None,
                0,
            )
            .await
            .map(drop),
        );
        invalid(
            NoteRepo::create(
                &f.db,
                Uuid::now_v7(),
                f.tree,
                "Sample note".into(),
                false,
                &NoteFilter {
                    person_id,
                    event_id,
                    ..NoteFilter::default()
                },
            )
            .await
            .map(drop),
        );
    }
}

#[tokio::test]
async fn a_name_made_primary_takes_the_place_of_the_previous_one() {
    let f = fixture().await;
    let pieces = |given: &str| PersonNamePieces {
        given_names: Some(given.to_owned()),
        surname: Some("Fixture".into()),
        ..PersonNamePieces::default()
    };
    let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
    for (id, given) in [(first, "First"), (second, "Second")] {
        PersonNameRepo::create(&f.db, id, f.person, NameType::Birth, pieces(given), true, 0)
            .await
            .unwrap();
    }
    let primary = || async {
        PersonNameRepo::list_by_person(&f.db, f.person)
            .await
            .unwrap()
            .into_iter()
            .filter(|name| name.is_primary)
            .map(|name| name.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(primary().await, [second]);

    PersonNameRepo::update(
        &f.db,
        first,
        None,
        PersonNamePiecesPatch::default(),
        Some(true),
        None,
    )
    .await
    .unwrap();
    assert_eq!(primary().await, [first]);
}
