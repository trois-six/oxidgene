//! Integration tests for [`ProfileService`] — the denormalized person
//! projections that replaced the `oxidgene-cache` crate.
//!
//! Everything runs against in-memory SQLite, i.e. the desktop configuration.
//! Since the projections live in `person_denorm` rather than in a process-local
//! cache, these tests can assert what a cache never could: that a projection is
//! durable across service instances and never observed stale after a mutation.

use crate::common;

use std::time::Instant;

use oxidgene_api::profile::ProfileService;
use oxidgene_api::profile::invalidation::affected_persons;
use oxidgene_core::enums::{
    Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole,
};
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PersonDenormRepo, PersonNamePieces,
    PersonNamePiecesPatch, PersonNameRepo, PersonRepo, PersonSearchRepo, TreeRepo,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

use common::setup_db;

async fn setup() -> (DatabaseConnection, ProfileService) {
    let db = setup_db().await;
    let service = ProfileService::new(db.clone());
    (db, service)
}

/// Projection rows of a tree written under the current schema version.
async fn current_rows(db: &DatabaseConnection, tree_id: Uuid) -> i64 {
    use oxidgene_db::sea_orm::{ConnectionTrait, Statement};

    db.query_one_raw(Statement::from_sql_and_values(
        db.get_database_backend(),
        "SELECT COUNT(*) AS n FROM person_denorm WHERE tree_id = $1 AND schema_version = $2",
        [
            tree_id.into(),
            oxidgene_core::projection::PROJECTION_SCHEMA_VERSION.into(),
        ],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "n")
    .unwrap()
}

async fn create_tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "Projection Tree".into(), None)
        .await
        .expect("create tree");
    id
}

async fn create_named_person(
    db: &DatabaseConnection,
    tree_id: Uuid,
    sex: Sex,
    given: &str,
    surname: &str,
    birth_year: Option<i32>,
) -> Uuid {
    let id = Uuid::now_v7();
    PersonRepo::create(db, id, tree_id, sex)
        .await
        .expect("person");
    PersonNameRepo::create(
        db,
        Uuid::now_v7(),
        id,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some(given.into()),
            surname: Some(surname.into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("name");
    if let Some(year) = birth_year {
        EventRepo::create(
            db,
            Uuid::now_v7(),
            tree_id,
            EventType::Birth,
            Some(year.to_string()),
            chrono::NaiveDate::from_ymd_opt(year, 1, 1),
            None,
            Some(id),
            None,
            None,
            DateQualifier::default(),
            None,
            Calendar::default(),
            Default::default(),
        )
        .await
        .expect("birth event");
    }
    id
}

/// Create father + mother + child linked through a family with a marriage event.
async fn create_family_trio(db: &DatabaseConnection, tree_id: Uuid) -> (Uuid, Uuid, Uuid, Uuid) {
    let father = create_named_person(db, tree_id, Sex::Male, "Jean", "Dupont", Some(1850)).await;
    let mother = create_named_person(db, tree_id, Sex::Female, "Jane", "Smith", Some(1855)).await;
    let child = create_named_person(db, tree_id, Sex::Male, "Pierre", "Dupont", Some(1880)).await;

    let family_id = Uuid::now_v7();
    FamilyRepo::create(db, family_id, tree_id)
        .await
        .expect("family");
    FamilySpouseRepo::create(
        db,
        Uuid::now_v7(),
        family_id,
        father,
        SpouseRole::Husband,
        0,
    )
    .await
    .expect("spouse f");
    FamilySpouseRepo::create(db, Uuid::now_v7(), family_id, mother, SpouseRole::Wife, 1)
        .await
        .expect("spouse m");
    FamilyChildRepo::create(
        db,
        Uuid::now_v7(),
        family_id,
        child,
        ChildType::Biological,
        0,
    )
    .await
    .expect("child");
    EventRepo::create(
        db,
        Uuid::now_v7(),
        tree_id,
        EventType::Marriage,
        Some("1878".into()),
        chrono::NaiveDate::from_ymd_opt(1878, 6, 15),
        None,
        None,
        Some(family_id),
        None,
        DateQualifier::default(),
        None,
        Calendar::default(),
        Default::default(),
    )
    .await
    .expect("marriage");

    (father, mother, child, family_id)
}

#[tokio::test]
async fn search_materializes_lazily_and_matches() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    // Neither table has rows yet — the first search materializes both.
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 0);

    let result = service.search(tree_id, "dupont", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 2, "Jean + Pierre Dupont");
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 3);

    // Accent-folded match.
    let result = service.search(tree_id, "jane", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 1);
    assert_eq!(result.entries[0].display_name, "Jane Smith");
    assert_eq!(result.entries[0].birth_year.as_deref(), Some("1855"));

    // Multi-word across fields.
    let result = service
        .search(tree_id, "pierre dupont", 10, 0)
        .await
        .unwrap();
    assert_eq!(result.total_count, 1);

    // Empty query = browse mode, everyone, sorted by surname.
    let result = service.search(tree_id, "", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 3);
    assert_eq!(result.entries[0].surname_normalized, "dupont");
}

#[tokio::test]
async fn rebuild_tree_full_populates_both_tables() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    let count = service.rebuild_tree_full(&db, tree_id).await.unwrap();
    assert_eq!(count, 3);
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 3);
}

#[tokio::test]
async fn targeted_person_build_denormalizes_family() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, family_id) = create_family_trio(&db, tree_id).await;

    // Child: family_as_child with both parents' display names.
    let child_profile = service.get_or_build_person(tree_id, child).await.unwrap();
    assert_eq!(
        child_profile.primary_name.as_ref().unwrap().display_name,
        "Pierre Dupont"
    );
    let as_child = child_profile.family_as_child.expect("child link");
    assert_eq!(as_child.family_id, family_id);
    assert_eq!(as_child.father_id, Some(father));
    assert_eq!(as_child.father_display_name.as_deref(), Some("Jean Dupont"));
    assert_eq!(as_child.mother_display_name.as_deref(), Some("Jane Smith"));
    assert!(child_profile.birth.is_some());

    // Father: families_as_spouse with spouse name, children and marriage.
    let father_profile = service.get_or_build_person(tree_id, father).await.unwrap();
    assert_eq!(father_profile.families_as_spouse.len(), 1);
    let link = &father_profile.families_as_spouse[0];
    assert_eq!(link.spouse_id, Some(mother));
    assert_eq!(link.spouse_display_name.as_deref(), Some("Jane Smith"));
    assert!(link.children_ids.contains(&child));
    assert!(link.marriage().is_some(), "marriage event denormalized");
}

#[tokio::test]
async fn get_or_build_person_persists_the_projection() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, _child, _family) = create_family_trio(&db, tree_id).await;

    assert!(
        PersonDenormRepo::get(&db, tree_id, father)
            .await
            .unwrap()
            .is_none()
    );

    service.get_or_build_person(tree_id, father).await.unwrap();

    let stored = PersonDenormRepo::get(&db, tree_id, father)
        .await
        .unwrap()
        .expect("projection written on first build");
    assert_eq!(
        stored.primary_name.unwrap().display_name,
        "Jean Dupont",
        "the row holds the assembled projection, not just an id"
    );
}

#[tokio::test]
async fn projections_survive_a_new_service_instance() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, _child, _family) = create_family_trio(&db, tree_id).await;
    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    drop(service);

    // A fresh service — as after a restart — reads the stored projections
    // without rebuilding anything.
    let restarted = ProfileService::new(db.clone());
    let profile = restarted
        .get_or_build_person(tree_id, father)
        .await
        .unwrap();
    assert_eq!(profile.primary_name.unwrap().display_name, "Jean Dupont");
    assert_eq!(
        restarted.get_all_persons(tree_id).await.unwrap().len(),
        3,
        "no rebuild needed after a restart"
    );
}

#[tokio::test]
async fn every_family_a_person_is_a_child_of_is_affected() {
    let (db, _service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, _family) = create_family_trio(&db, tree_id).await;

    // An adoptive family alongside the biological one: its parents list the
    // child too, so they must be refreshed when the child changes.
    let adoptive = create_named_person(&db, tree_id, Sex::Female, "Anne", "Martin", None).await;
    let adoptive_family = Uuid::now_v7();
    FamilyRepo::create(&db, adoptive_family, tree_id)
        .await
        .unwrap();
    FamilySpouseRepo::create(
        &db,
        Uuid::now_v7(),
        adoptive_family,
        adoptive,
        SpouseRole::Wife,
        0,
    )
    .await
    .unwrap();
    FamilyChildRepo::create(
        &db,
        Uuid::now_v7(),
        adoptive_family,
        child,
        ChildType::Adopted,
        0,
    )
    .await
    .unwrap();

    let mut expected = vec![father, mother, child, adoptive];
    expected.sort();
    assert_eq!(affected_persons(&db, child).await.unwrap(), expected);
}

#[tokio::test]
async fn name_mutation_refreshes_stored_projections() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    assert_eq!(
        service
            .search(tree_id, "jean", 10, 0)
            .await
            .unwrap()
            .total_count,
        1
    );

    // Rename Jean → Marcel (replace the primary name).
    let names = PersonNameRepo::list_by_person(&db, father).await.unwrap();
    PersonNameRepo::delete(&db, names[0].id).await.unwrap();
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        father,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Marcel".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();

    // Same entry point the REST / GraphQL handlers use.
    service
        .invalidate_for_mutation(&db, tree_id, &affected_persons(&db, father).await.unwrap())
        .await
        .unwrap();

    // Search rows follow.
    assert_eq!(
        service
            .search(tree_id, "jean", 10, 0)
            .await
            .unwrap()
            .total_count,
        0
    );
    let result = service.search(tree_id, "marcel", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 1);
    assert_eq!(result.entries[0].display_name, "Marcel Dupont");

    // And so does the *child's* stored projection, which embeds the father's
    // display name — the fan-out that makes denormalization non-trivial.
    let stored_child = PersonDenormRepo::get(&db, tree_id, child)
        .await
        .unwrap()
        .expect("child projection");
    assert_eq!(
        stored_child
            .family_as_child
            .expect("child link")
            .father_display_name
            .as_deref(),
        Some("Marcel Dupont"),
        "the relative's projection must not be left stale"
    );
}

#[tokio::test]
async fn person_delete_removes_projection_and_search_row() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (_father, mother, _child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    assert_eq!(
        service
            .search(tree_id, "smith", 10, 0)
            .await
            .unwrap()
            .total_count,
        1
    );

    PersonRepo::delete(&db, mother).await.unwrap();
    service
        .invalidate_for_person_delete(&db, tree_id, mother)
        .await
        .unwrap();

    assert_eq!(
        service
            .search(tree_id, "smith", 10, 0)
            .await
            .unwrap()
            .total_count,
        0
    );
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 2);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 2);
    assert!(
        PersonDenormRepo::get(&db, tree_id, mother)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn search_rows_name_the_close_relatives() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (_father, _mother, child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();

    let hit = service
        .search(tree_id, "pierre", 10, 0)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(hit.person_id, child);
    // A result has to say who someone is, not just what they are called —
    // two people of the same name are told apart by their parents.
    assert_eq!(hit.father_name.as_deref(), Some("Jean Dupont"));
    assert_eq!(hit.mother_name.as_deref(), Some("Jane Smith"));
    assert!(hit.spouse_names.is_empty());
    assert_eq!(hit.children_count, 0);

    let father_hit = service
        .search(tree_id, "jean dupont", 10, 0)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(father_hit.spouse_names, vec!["Jane Smith".to_string()]);
    assert_eq!(father_hit.children_count, 1);
    assert!(father_hit.father_name.is_none());
}

#[tokio::test]
async fn renaming_a_spouse_refreshes_the_other_search_row() {
    // The spouse's name now lives on the subject's own search row, so a rename
    // has to reach one hop out or the result keeps naming someone who no
    // longer exists under that name.
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, _child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();

    let names = PersonNameRepo::list_by_person(&db, mother).await.unwrap();
    let primary = names.iter().find(|n| n.is_primary).expect("primary name");
    PersonNameRepo::update(
        &db,
        primary.id,
        None,
        PersonNamePiecesPatch {
            surname: Some(Some("Moreau".into())),
            ..Default::default()
        },
        None,
        None,
    )
    .await
    .unwrap();

    let affected = oxidgene_api::profile::invalidation::affected_persons(&db, mother)
        .await
        .unwrap();
    service
        .invalidate_for_mutation(&db, tree_id, &affected)
        .await
        .unwrap();

    let father_hit = service
        .search(tree_id, "jean dupont", 10, 0)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(father_hit.person_id, father);
    assert_eq!(father_hit.spouse_names, vec!["Jane Moreau".to_string()]);
}

#[tokio::test]
async fn deleting_a_family_clears_the_children_parent_names() {
    // `affected_persons_for_family` returns only the spouses, which is right
    // for a marriage-date edit but not for a delete: the children lose their
    // parents, and nothing else would rebuild them.
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (_father, _mother, child, family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();

    let affected =
        oxidgene_api::profile::invalidation::affected_persons_for_family_delete(&db, family)
            .await
            .unwrap();
    assert!(
        affected.contains(&child),
        "a family delete must rebuild its children, not only its spouses"
    );

    FamilyRepo::delete(&db, family).await.unwrap();
    service
        .invalidate_for_mutation(&db, tree_id, &affected)
        .await
        .unwrap();

    let hit = service
        .search(tree_id, "pierre", 10, 0)
        .await
        .unwrap()
        .entries
        .remove(0);
    assert_eq!(hit.person_id, child);
    assert!(hit.father_name.is_none());
    assert!(hit.mother_name.is_none());
}

#[tokio::test]
async fn tree_teardown_clears_both_tables() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 3);

    service.invalidate_tree(&db, tree_id).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 0);
}

#[tokio::test]
async fn pedigree_builds_from_stored_projections() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, _family) = create_family_trio(&db, tree_id).await;

    let pedigree = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();

    assert!(pedigree.persons.contains_key(&child));
    assert!(pedigree.persons.contains_key(&father));
    assert!(pedigree.persons.contains_key(&mother));
    assert_eq!(pedigree.persons[&father].generation, -1);
    assert!(
        pedigree
            .edges
            .iter()
            .any(|e| e.parent_id == father && e.child_id == child)
    );

    // Rebuilt from scratch on every call — and identical each time.
    let again = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();
    assert_eq!(again.persons.len(), pedigree.persons.len());
    assert_eq!(again.edges.len(), pedigree.edges.len());
}

/// A family lists its husband first and its wife after, whoever the pedigree
/// is drawn around. The persons are gathered in the order of a hash map, which
/// changes with every map, and the spouses used to follow it: the event panel
/// then named a person's parents' marriage after the father on one visit and
/// after the mother on the next. Several roots and repeated builds, each with
/// maps of their own, give a wandering order many chances to show.
#[tokio::test]
async fn a_pedigree_family_lists_the_husband_first() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, family) = create_family_trio(&db, tree_id).await;

    for _ in 0..8 {
        for root in [child, mother, father] {
            let pedigree = service
                .get_or_build_pedigree(tree_id, root, 2, 1)
                .await
                .unwrap();
            assert_eq!(
                pedigree.families[&family].spouse_ids,
                [father, mother],
                "around {root}"
            );
        }
    }
}

/// An approximate date has to survive the whole way to the pedigree node, or
/// the card that draws it turns "about 1849" into a flat "1849" and quietly
/// claims a precision the record never had.
///
/// The projection stores a *year string*, which cannot carry "about" on its
/// own — this is the test that the qualifier travels beside it.
#[tokio::test]
async fn a_pedigree_node_keeps_how_precise_its_dates_are() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let person_id = Uuid::now_v7();
    PersonRepo::create(&db, person_id, tree_id, Sex::Male)
        .await
        .expect("person");
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        person_id,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Child One".into()),
            surname: Some("Branch A".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("name");

    // Born about 1849, died before 1917 — the shape of a person whose dates
    // come from an age on a later record rather than from a register.
    for (event_type, year, qualifier) in [
        (EventType::Birth, 1849, DateQualifier::About),
        (EventType::Death, 1917, DateQualifier::Before),
    ] {
        EventRepo::create(
            &db,
            Uuid::now_v7(),
            tree_id,
            event_type,
            Some(year.to_string()),
            chrono::NaiveDate::from_ymd_opt(year, 1, 1),
            None,
            Some(person_id),
            None,
            None,
            qualifier,
            None,
            Calendar::default(),
            Default::default(),
        )
        .await
        .expect("event");
    }

    let pedigree = service
        .get_or_build_pedigree(tree_id, person_id, 1, 1)
        .await
        .unwrap();
    let node = &pedigree.persons[&person_id];

    let birth = node.birth.as_ref().expect("birth on the node");
    let death = node.death.as_ref().expect("death on the node");
    assert_eq!(birth.date_qualifier, DateQualifier::About);
    assert_eq!(death.date_qualifier, DateQualifier::Before);
    // The whole date, not a year pulled out of it — the events panel writes
    // « vers 2 nov. 1849 » from this, and a year-only value would silently
    // drop the day and month.
    assert_eq!(birth.date_value.as_deref(), Some("1849"));
    assert_eq!(death.date_value.as_deref(), Some("1917"));

    // The profile's own events carry it too — that is what the events panel
    // reads to write « vers 1849 » in full.
    let profile = service
        .get_or_build_person(tree_id, person_id)
        .await
        .unwrap();
    assert_eq!(
        profile.birth.as_ref().unwrap().date_qualifier,
        DateQualifier::About
    );
    assert_eq!(
        profile.death.as_ref().unwrap().date_qualifier,
        DateQualifier::Before
    );
}

/// The pedigree node used to hold a *year string* pulled out of the event, so
/// everything that did not fit — the day, the month, the far end of a range,
/// the calendar — was gone before the frontend saw it. The events panel showed
/// "1788" for a birth on 2 Nov 1788, and "between 1691" for a death recorded
/// as "between 11 Nov 1691 and 20 Aug 1693".
#[tokio::test]
async fn a_pedigree_node_keeps_whole_dates_not_just_the_year() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let person_id = Uuid::now_v7();
    PersonRepo::create(&db, person_id, tree_id, Sex::Male)
        .await
        .expect("person");
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        person_id,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Child Two".into()),
            surname: Some("Branch A".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("name");

    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Birth,
        Some("2 NOV 1788".to_string()),
        chrono::NaiveDate::from_ymd_opt(1788, 11, 2),
        None,
        Some(person_id),
        None,
        None,
        DateQualifier::default(),
        None,
        Calendar::default(),
        Default::default(),
    )
    .await
    .expect("birth");
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Death,
        Some("11 NOV 1691".to_string()),
        chrono::NaiveDate::from_ymd_opt(1691, 11, 11),
        None,
        Some(person_id),
        None,
        None,
        DateQualifier::Between,
        Some("20 AUG 1693".to_string()),
        Calendar::default(),
        Default::default(),
    )
    .await
    .expect("death");

    let pedigree = service
        .get_or_build_pedigree(tree_id, person_id, 1, 1)
        .await
        .unwrap();
    let node = &pedigree.persons[&person_id];

    let birth = node.birth.as_ref().expect("birth");
    assert_eq!(birth.date_value.as_deref(), Some("2 NOV 1788"));

    let death = node.death.as_ref().expect("death");
    assert_eq!(death.date_qualifier, DateQualifier::Between);
    // Without this the panel writes "between 1691" — a qualifier promising a
    // second date the projection could not carry.
    assert_eq!(death.date_value2.as_deref(), Some("20 AUG 1693"));
}

/// A parish register routinely records a baptism and no birth. GeneWeb dates
/// the card from the baptism rather than leaving it blank, and so do we.
#[tokio::test]
async fn a_card_falls_back_to_baptism_and_burial() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let person_id = Uuid::now_v7();
    PersonRepo::create(&db, person_id, tree_id, Sex::Male)
        .await
        .expect("person");
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        person_id,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Child Three".into()),
            surname: Some("Branch A".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("name");

    // No Birth, no Death — only the sacraments either side of them.
    for (event_type, value, year, qualifier) in [
        (EventType::Baptism, "1620", 1620, DateQualifier::About),
        (EventType::Burial, "1691", 1691, DateQualifier::Exact),
    ] {
        EventRepo::create(
            &db,
            Uuid::now_v7(),
            tree_id,
            event_type,
            Some(value.to_string()),
            chrono::NaiveDate::from_ymd_opt(year, 1, 1),
            None,
            Some(person_id),
            None,
            None,
            qualifier,
            None,
            Calendar::default(),
            Default::default(),
        )
        .await
        .expect("event");
    }

    let pedigree = service
        .get_or_build_pedigree(tree_id, person_id, 1, 1)
        .await
        .unwrap();
    let node = &pedigree.persons[&person_id];

    assert_eq!(
        node.birth.as_ref().map(|e| e.event_type),
        Some(EventType::Baptism),
        "a card with no birth should be dated from the baptism"
    );
    assert_eq!(
        node.death.as_ref().map(|e| e.event_type),
        Some(EventType::Burial)
    );
    // Each event keeps its *own* precision. GeneWeb sets one `approx` flag for
    // the pair here, which is how Geneanet ends up stamping "ca" on a burial
    // year that was recorded exactly.
    assert_eq!(
        node.birth.as_ref().unwrap().date_qualifier,
        DateQualifier::About
    );
    assert_eq!(
        node.death.as_ref().unwrap().date_qualifier,
        DateQualifier::Exact
    );
}

/// The commonest shape in a parish tree, and the one that broke: a Birth event
/// exists but carries no date — an empty stub someone made to hang a source
/// on — while the dated record is the Baptism. Falling back only when the
/// *event* is missing keeps the stub and draws a blank year.
#[tokio::test]
async fn a_dateless_birth_does_not_mask_a_dated_baptism() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let person_id = Uuid::now_v7();
    PersonRepo::create(&db, person_id, tree_id, Sex::Male)
        .await
        .expect("person");
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        person_id,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Child Four".into()),
            surname: Some("Branch A".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("name");

    // A Birth with no date at all, and a Baptism that has one.
    for (event_type, value, sort, qualifier) in [
        (EventType::Birth, None, None, DateQualifier::Exact),
        (
            EventType::Baptism,
            Some("1620".to_string()),
            chrono::NaiveDate::from_ymd_opt(1620, 1, 1),
            DateQualifier::About,
        ),
        // Mirror image on the other end: a dateless Death over a dated Burial.
        (EventType::Death, None, None, DateQualifier::Exact),
        (
            EventType::Burial,
            Some("1691".to_string()),
            chrono::NaiveDate::from_ymd_opt(1691, 1, 1),
            DateQualifier::Exact,
        ),
    ] {
        EventRepo::create(
            &db,
            Uuid::now_v7(),
            tree_id,
            event_type,
            value,
            sort,
            None,
            Some(person_id),
            None,
            None,
            qualifier,
            None,
            Calendar::default(),
            Default::default(),
        )
        .await
        .expect("event");
    }

    let pedigree = service
        .get_or_build_pedigree(tree_id, person_id, 1, 1)
        .await
        .unwrap();
    let node = &pedigree.persons[&person_id];

    let birth = node
        .birth
        .as_ref()
        .expect("a dated event to date the card by");
    assert_eq!(birth.event_type, EventType::Baptism);
    assert_eq!(birth.date_value.as_deref(), Some("1620"));
    assert_eq!(birth.date_qualifier, DateQualifier::About);

    let death = node.death.as_ref().expect("a dated event");
    assert_eq!(death.event_type, EventType::Burial);
    assert_eq!(death.date_value.as_deref(), Some("1691"));
}

/// A payload written by an older build must be rebuilt, not served.
///
/// This is the trap the schema version exists to close: every field added to
/// `PersonProfile` carries `#[serde(default)]`, so an old payload deserializes
/// cleanly and comes back *looking complete*. Nothing could tell it apart from
/// a person who genuinely has nothing recorded, which is how the date
/// qualifiers shipped invisible on every existing install.
#[tokio::test]
async fn a_projection_from_an_older_build_is_rebuilt_rather_than_served() {
    use oxidgene_db::sea_orm::{ConnectionTrait, Statement};

    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    assert_eq!(current_rows(&db, tree_id).await, 3);

    // Age the rows the way a binary upgrade does: the payloads stay byte for
    // byte what they were, only the version they were written under moves.
    // Then corrupt one so that *serving* it would be unmistakable.
    let backend = db.get_database_backend();
    db.execute_raw(Statement::from_string(
        backend,
        "UPDATE person_denorm SET schema_version = 0",
    ))
    .await
    .unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        backend,
        "UPDATE person_denorm SET payload = replace(payload, 'Pierre', 'STALE') WHERE person_id = $1",
        [child.into()],
    ))
    .await
    .unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        backend,
        "UPDATE person_denorm SET payload = replace(payload, 'Jean', 'STALE') WHERE person_id = $1",
        [father.into()],
    ))
    .await
    .unwrap();

    // Physically still there; usable, no.
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert_eq!(current_rows(&db, tree_id).await, 0);
    assert!(
        PersonDenormRepo::get(&db, tree_id, child)
            .await
            .unwrap()
            .is_none(),
        "a stale row reads as absent, which is what sends callers to rebuild"
    );

    // The ordinary read path heals it, with no rebuild call of its own.
    let profile = service.get_or_build_person(tree_id, child).await.unwrap();
    assert_eq!(
        profile.primary_name.as_ref().unwrap().display_name,
        "Pierre Dupont",
        "the doctored payload must not have been served"
    );
    assert_eq!(
        current_rows(&db, tree_id).await,
        1,
        "and the rebuilt row is stamped with the current version"
    );

    // One healed person must not pass for a healed tree: a tree-wide read
    // still finds the others stale and rebuilds them all before serving.
    let everyone = service.get_all_persons(tree_id).await.unwrap();
    assert_eq!(everyone.len(), 3);
    assert!(
        everyone.iter().all(|profile| !profile
            .primary_name
            .as_ref()
            .is_some_and(|name| name.display_name.contains("STALE"))),
        "a stale payload was served by a tree-wide read"
    );
    assert_eq!(current_rows(&db, tree_id).await, 3);

    // A pedigree reads the healed tree.
    let pedigree = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();
    assert_eq!(pedigree.persons[&child].display_name, "Pierre Dupont");
    assert_eq!(current_rows(&db, tree_id).await, 3);
}

#[tokio::test]
async fn pedigree_reflects_a_mutation_immediately() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;

    let before = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();
    assert_eq!(before.persons[&father].display_name, "Jean Dupont");

    let names = PersonNameRepo::list_by_person(&db, father).await.unwrap();
    PersonNameRepo::delete(&db, names[0].id).await.unwrap();
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        father,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Marcel".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();
    service
        .invalidate_for_mutation(&db, tree_id, &affected_persons(&db, father).await.unwrap())
        .await
        .unwrap();

    let after = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();
    assert_eq!(
        after.persons[&father].display_name, "Marcel Dupont",
        "no pedigree cache left to serve a stale node"
    );
}

/// A family of `tree_id` whose `parents` (with their roles) have `child`.
async fn link_parents(
    db: &DatabaseConnection,
    tree_id: Uuid,
    parents: &[(Uuid, SpouseRole)],
    child: Uuid,
) -> Uuid {
    let family_id = Uuid::now_v7();
    FamilyRepo::create(db, family_id, tree_id).await.unwrap();
    for (position, (parent, role)) in parents.iter().enumerate() {
        FamilySpouseRepo::create(
            db,
            Uuid::now_v7(),
            family_id,
            *parent,
            *role,
            position as i32,
        )
        .await
        .unwrap();
    }
    FamilyChildRepo::create(
        db,
        Uuid::now_v7(),
        family_id,
        child,
        ChildType::Biological,
        0,
    )
    .await
    .unwrap();
    family_id
}

async fn set_sosa_root(db: &DatabaseConnection, tree_id: Uuid, root: Option<Uuid>) {
    TreeRepo::update(
        db,
        tree_id,
        oxidgene_db::repo::TreeChanges {
            sosa_root_person_id: Some(root),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

/// SOSA numbers follow the parents' roles: a husband doubles, a wife doubles
/// plus one, and a partner is an ancestor without a number.
#[tokio::test]
async fn sosa_numbers_follow_the_parents_roles() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let [root, father, mother, grandfather, partner, unrelated] = [(); 6].map(|_| Uuid::now_v7());
    for person in [root, father, mother, grandfather, partner, unrelated] {
        PersonRepo::create(&db, person, tree_id, Sex::Unknown)
            .await
            .unwrap();
    }
    link_parents(
        &db,
        tree_id,
        &[
            (father, SpouseRole::Husband),
            (mother, SpouseRole::Wife),
            (partner, SpouseRole::Partner),
        ],
        root,
    )
    .await;
    link_parents(&db, tree_id, &[(grandfather, SpouseRole::Husband)], father).await;
    set_sosa_root(&db, tree_id, Some(root)).await;

    for (person, expected) in [
        (root, Some(1)),
        (father, Some(2)),
        (mother, Some(3)),
        (grandfather, Some(4)),
        (partner, None),
        (unrelated, None),
    ] {
        assert_eq!(
            service.sosa_number(tree_id, person).await.unwrap(),
            expected
        );
    }
    let marked = service
        .sosa_ancestors_among(
            tree_id,
            &[root, father, mother, grandfather, partner, unrelated],
        )
        .await
        .unwrap();
    assert_eq!(
        marked,
        [root, father, mother, grandfather, partner]
            .into_iter()
            .collect(),
        "a partner is an ancestor all the same"
    );
}

/// The stored SOSA ancestry follows every change to its inputs without
/// anything asking for it: a new link, a deletion, a new root.
#[tokio::test]
async fn the_stored_sosa_ancestry_follows_the_tree() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, family) = create_family_trio(&db, tree_id).await;
    let grandmother = create_named_person(&db, tree_id, Sex::Female, "Ida", "Exemple", None).await;
    set_sosa_root(&db, tree_id, Some(child)).await;
    assert_eq!(service.sosa_number(tree_id, mother).await.unwrap(), Some(3));
    assert_eq!(
        service.sosa_number(tree_id, grandmother).await.unwrap(),
        None
    );

    link_parents(&db, tree_id, &[(grandmother, SpouseRole::Wife)], father).await;
    assert_eq!(
        service.sosa_number(tree_id, grandmother).await.unwrap(),
        Some(5)
    );
    let pedigree = service
        .get_or_build_pedigree(tree_id, father, 1, 0)
        .await
        .unwrap();
    assert!(pedigree.persons[&grandmother].sosa_ancestor);

    FamilyRepo::delete(&db, family).await.unwrap();
    assert_eq!(service.sosa_number(tree_id, father).await.unwrap(), None);
    assert_eq!(
        service.sosa_number(tree_id, grandmother).await.unwrap(),
        None
    );

    set_sosa_root(&db, tree_id, Some(father)).await;
    assert_eq!(service.sosa_number(tree_id, father).await.unwrap(), Some(1));
    assert_eq!(
        service.sosa_number(tree_id, grandmother).await.unwrap(),
        Some(3)
    );

    PersonRepo::delete(&db, father).await.unwrap();
    assert!(
        service
            .sosa_ancestors_among(tree_id, &[father, grandmother])
            .await
            .unwrap()
            .is_empty(),
        "a deleted root is no root"
    );
}

#[tokio::test]
async fn expand_pedigree_returns_only_the_new_generation() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;

    // Add a grandfather one generation above the father.
    let grandfather =
        create_named_person(&db, tree_id, Sex::Male, "Louis", "Dupont", Some(1820)).await;
    let gf_family = Uuid::now_v7();
    FamilyRepo::create(&db, gf_family, tree_id).await.unwrap();
    FamilySpouseRepo::create(
        &db,
        Uuid::now_v7(),
        gf_family,
        grandfather,
        SpouseRole::Husband,
        0,
    )
    .await
    .unwrap();
    FamilyChildRepo::create(
        &db,
        Uuid::now_v7(),
        gf_family,
        father,
        ChildType::Biological,
        0,
    )
    .await
    .unwrap();

    let delta = service
        .expand_pedigree(
            tree_id,
            child,
            oxidgene_core::projection::PedigreeDirection::Ancestors,
            1,
            2,
            0,
        )
        .await
        .unwrap();

    let new_ids: Vec<Uuid> = delta.new_nodes.iter().map(|n| n.person_id).collect();
    assert!(
        new_ids.contains(&grandfather),
        "the newly reachable generation is in the delta"
    );
    assert!(
        !new_ids.contains(&father) && !new_ids.contains(&child),
        "already-loaded nodes are not repeated: {new_ids:?}"
    );
    assert_eq!(delta.ancestor_depth_loaded, 2);
}

/// Performance regression guard: person loads and searches must stay within
/// the interactive budget (< 100 ms in debug builds; release is ~10× faster).
/// Ignored by default — run with `cargo test -p oxidgene-api -- --ignored`.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn person_load_and_search_performance() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let surnames = ["Richard", "Dupont", "Lefèvre", "Martin", "Bernard"];
    let givens = ["Jean", "Pierre", "Marie", "Luc", "Anne"];
    let mut last_person = None;
    for i in 0..2_000 {
        let id = create_named_person(
            &db,
            tree_id,
            if i % 2 == 0 { Sex::Male } else { Sex::Female },
            givens[i % givens.len()],
            &format!("{}{}", surnames[i % surnames.len()], i),
            Some(1700 + (i as i32 % 300)),
        )
        .await;
        last_person = Some(id);
    }
    let person_id = last_person.unwrap();

    // Full rebuild (GEDCOM-import path) — populates both tables.
    let t0 = Instant::now();
    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    let rebuild = t0.elapsed();

    // Person detail page path — now a single indexed row read.
    let t1 = Instant::now();
    let profile = service
        .get_or_build_person(tree_id, person_id)
        .await
        .unwrap();
    let person_load = t1.elapsed();
    assert_eq!(profile.person_id, person_id);

    // FTS search.
    let t2 = Instant::now();
    let result = service.search(tree_id, "richard 17", 20, 0).await.unwrap();
    let search = t2.elapsed();
    assert!(result.total_count > 0);

    println!(
        "projection perf (2k persons): rebuild_tree_full={rebuild:?}, \
         person_load={person_load:?}, search={search:?}"
    );
    assert!(
        person_load.as_millis() < 100,
        "person load took {person_load:?}, expected < 100ms"
    );
    assert!(
        search.as_millis() < 100,
        "FTS search took {search:?}, expected < 100ms"
    );
}

/// Large-tree benchmark approximating a big GEDCOM import (20K persons).
/// Ignored by default — run with `cargo test -p oxidgene-api -- --ignored`.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn bench_large_tree_20k() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let surnames = [
        "Richard", "Dupont", "Lefèvre", "Martin", "Bernard", "Moreau",
    ];
    let givens = ["Jean", "Pierre", "Marie", "Éloïse", "Luc", "Anne"];
    let mut last_person = None;
    for i in 0..20_000 {
        let id = create_named_person(
            &db,
            tree_id,
            if i % 2 == 0 { Sex::Male } else { Sex::Female },
            givens[i % givens.len()],
            &format!("{}{}", surnames[i % surnames.len()], i),
            Some(1500 + (i as i32 % 500)),
        )
        .await;
        last_person = Some(id);
    }
    let person_id = last_person.unwrap();

    let t0 = Instant::now();
    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    let rebuild = t0.elapsed();

    let t1 = Instant::now();
    service
        .get_or_build_person(tree_id, person_id)
        .await
        .unwrap();
    let person_load = t1.elapsed();

    let t2 = Instant::now();
    let result = service.search(tree_id, "moreau 19", 20, 0).await.unwrap();
    let search = t2.elapsed();

    println!(
        "projection bench (20k persons): rebuild_tree_full={rebuild:?}, \
         person_load={person_load:?}, search={search:?} ({} hits)",
        result.total_count
    );
}

// ─────────────────────────── Atomicity ───────────────────────────────

/// The whole point of running the mutation and the refresh on one transaction:
/// a failure after the write must undo the projection too, so the two can never
/// be observed out of step. This is what a post-commit refresh could not offer.
#[tokio::test]
async fn a_rolled_back_mutation_leaves_no_projection_behind() {
    use oxidgene_db::sea_orm::TransactionTrait;

    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;
    service.rebuild_tree_full(&db, tree_id).await.unwrap();

    // Rename the father and refresh the projections — then roll back instead
    // of committing, as an error mid-handler would.
    let txn = db.begin().await.unwrap();
    let names = PersonNameRepo::list_by_person(&txn, father).await.unwrap();
    PersonNameRepo::delete(&txn, names[0].id).await.unwrap();
    PersonNameRepo::create(
        &txn,
        Uuid::now_v7(),
        father,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Marcel".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();
    service
        .invalidate_for_mutation(
            &txn,
            tree_id,
            &affected_persons(&txn, father).await.unwrap(),
        )
        .await
        .unwrap();

    // Inside the transaction, both the row and the projection show the new name.
    let staged = PersonDenormRepo::get(&txn, tree_id, father)
        .await
        .unwrap()
        .expect("projection staged in the transaction");
    assert_eq!(staged.primary_name.unwrap().display_name, "Marcel Dupont");

    txn.rollback().await.unwrap();

    // After the rollback, the name is unchanged...
    let names = PersonNameRepo::list_by_person(&db, father).await.unwrap();
    assert_eq!(names[0].display_name(), "Jean Dupont");

    // ...and so is the projection — no orphan refresh survived the rollback.
    let stored = PersonDenormRepo::get(&db, tree_id, father)
        .await
        .unwrap()
        .expect("projection");
    assert_eq!(
        stored.primary_name.unwrap().display_name,
        "Jean Dupont",
        "the projection must roll back with the mutation"
    );

    // The relative's embedded copy of the name must be consistent too.
    let stored_child = PersonDenormRepo::get(&db, tree_id, child)
        .await
        .unwrap()
        .expect("child projection");
    assert_eq!(
        stored_child
            .family_as_child
            .expect("child link")
            .father_display_name
            .as_deref(),
        Some("Jean Dupont"),
    );

    // And search still reflects the pre-mutation state.
    assert_eq!(
        service
            .search(tree_id, "marcel", 10, 0)
            .await
            .unwrap()
            .total_count,
        0
    );
    assert_eq!(
        service
            .search(tree_id, "jean", 10, 0)
            .await
            .unwrap()
            .total_count,
        1
    );
}

/// A committed transaction applies both halves.
#[tokio::test]
async fn a_committed_mutation_applies_data_and_projection_together() {
    use oxidgene_db::sea_orm::TransactionTrait;

    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;
    service.rebuild_tree_full(&db, tree_id).await.unwrap();

    let txn = db.begin().await.unwrap();
    let names = PersonNameRepo::list_by_person(&txn, father).await.unwrap();
    PersonNameRepo::delete(&txn, names[0].id).await.unwrap();
    PersonNameRepo::create(
        &txn,
        Uuid::now_v7(),
        father,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Marcel".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();
    service
        .invalidate_for_mutation(
            &txn,
            tree_id,
            &affected_persons(&txn, father).await.unwrap(),
        )
        .await
        .unwrap();
    txn.commit().await.unwrap();

    let stored_child = PersonDenormRepo::get(&db, tree_id, child)
        .await
        .unwrap()
        .expect("child projection");
    assert_eq!(
        stored_child
            .family_as_child
            .expect("child link")
            .father_display_name
            .as_deref(),
        Some("Marcel Dupont"),
    );
    assert_eq!(
        service
            .search(tree_id, "marcel", 10, 0)
            .await
            .unwrap()
            .total_count,
        1
    );
}

// ─────────────────────── Whole-tree rebuilds ─────────────────────────

/// The router, its projection service and its writer over a fresh
/// database where a read does not wait for a write: a SQLite file with its
/// read pool, as the desktop runs, or a schema of the run's PostgreSQL
/// database. The directory lives as long as it is held.
async fn file_backed_app() -> (
    axum::Router,
    std::sync::Arc<ProfileService>,
    DatabaseConnection,
    tempfile::TempDir,
) {
    let directory = tempfile::tempdir().expect("database directory");
    let connections: oxidgene_db::repo::Connections =
        if std::env::var("OXIDGENE_TEST_POSTGRES_URL").is_ok() {
            setup_db().await.into()
        } else {
            common::setup_file_db(directory.path()).await
        };
    let state = common::offline(oxidgene_api::AppState::new(
        connections,
        common::test_media_root(),
    ));
    let profiles = std::sync::Arc::clone(&state.profiles);
    let db = state.db.clone();
    (oxidgene_api::build_router(state), profiles, db, directory)
}

/// Mark every projection of the database as written by an older build, the
/// way a binary upgrade leaves them.
async fn outdate_projections(db: &DatabaseConnection) {
    use oxidgene_db::sea_orm::{ConnectionTrait, Statement};
    db.execute_raw(Statement::from_string(
        db.get_database_backend(),
        "UPDATE person_denorm SET schema_version = 0",
    ))
    .await
    .unwrap();
}

/// Requests arriving together on a tree whose projections an upgrade
/// outdated wait for one rebuild, which commits whole: none of them runs
/// its own, and none reads the tree while only a part of it is rebuilt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_reads_arriving_together_rebuild_a_tree_once_and_read_it_whole() {
    use axum::http::Method;

    const BLOCKS: usize = 150;
    let (app, profiles, db, _directory) = file_backed_app().await;
    let (tree, anchor) = common::family_blocks_tree(&app, &db, BLOCKS).await;
    let pedigree =
        format!("/api/v1/trees/{tree}/pedigree/{anchor}?ancestor_depth=2&descendant_depth=0");
    let before = common::ok(&app, Method::GET, &pedigree, None).await;
    let nodes = before["persons"].as_object().unwrap().len();
    assert!(
        nodes >= 7,
        "the root, its parents and grandparents: {nodes}"
    );

    outdate_projections(&db).await;
    let rebuilds = profiles.full_rebuilds();
    let requests: Vec<_> = (0..16)
        .map(|at| {
            let app = app.clone();
            let uri = match at % 4 {
                0 | 1 => pedigree.clone(),
                2 => format!("/api/v1/trees/{tree}/profiles?first=1"),
                _ => format!("/api/v1/trees/{tree}/persons/search?q="),
            };
            tokio::spawn(async move { common::ok(&app, Method::GET, &uri, None).await })
        })
        .collect();
    for (at, request) in requests.into_iter().enumerate() {
        let answer = request.await.unwrap();
        match at % 4 {
            0 | 1 => assert_eq!(
                answer["persons"].as_object().unwrap().len(),
                nodes,
                "a pedigree read a part of the tree"
            ),
            _ => assert_eq!(
                answer["total_count"],
                BLOCKS * 10,
                "request {at} read a part of the tree"
            ),
        }
    }
    assert_eq!(
        profiles.full_rebuilds() - rebuilds,
        1,
        "the requests arriving together share one rebuild"
    );
    assert_eq!(
        current_rows(&db, tree.parse().unwrap()).await,
        (BLOCKS * 10) as i64
    );
}

/// The admin rebuild is one transaction: a reader never finds the tree
/// between its rows' deletion and their rewriting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_forced_rebuild_never_shows_a_part_of_the_tree() {
    use axum::http::Method;

    const BLOCKS: usize = 150;
    let (app, profiles, db, _directory) = file_backed_app().await;
    let (tree, _anchor) = common::family_blocks_tree(&app, &db, BLOCKS).await;
    let tree_id: Uuid = tree.parse().unwrap();

    let rebuilding = {
        let profiles = std::sync::Arc::clone(&profiles);
        tokio::spawn(async move { profiles.rebuild_tree(tree_id).await.unwrap() })
    };
    let uri = format!("/api/v1/trees/{tree}/profiles?first=1");
    while !rebuilding.is_finished() {
        let page = common::ok(&app, Method::GET, &uri, None).await;
        assert_eq!(page["total_count"], BLOCKS * 10, "a part of the tree");
    }
    assert_eq!(rebuilding.await.unwrap(), BLOCKS * 10);
}

/// A worker's maintenance pass rebuilds the trees an upgrade outdated, so
/// that their first read finds them built; a tree already current is left
/// alone.
#[tokio::test]
async fn the_trees_an_upgrade_outdated_are_rebuilt_before_their_first_read() {
    let (db, service) = setup().await;
    let outdated = create_tree(&db).await;
    create_family_trio(&db, outdated).await;
    service.rebuild_tree_full(&db, outdated).await.unwrap();
    outdate_projections(&db).await;
    let current = create_tree(&db).await;
    create_family_trio(&db, current).await;
    service.rebuild_tree_full(&db, current).await.unwrap();

    assert_eq!(
        PersonDenormRepo::stale_trees(&db).await.unwrap(),
        [outdated]
    );
    let rebuilds = service.full_rebuilds();
    assert_eq!(service.rebuild_stale_trees().await.unwrap(), 1);
    assert_eq!(service.full_rebuilds() - rebuilds, 1);
    assert_eq!(current_rows(&db, outdated).await, 3);
    assert!(PersonDenormRepo::stale_trees(&db).await.unwrap().is_empty());

    // Its first read rebuilds nothing.
    service.search(outdated, "", 10, 0).await.unwrap();
    assert_eq!(service.full_rebuilds() - rebuilds, 1);
}

// ─────────────────────── Wide refreshes ──────────────────────────────

/// A refresh builds the persons its mutation affects, whatever their
/// number, and no other: renaming a place 600 persons of 3,000 are born in
/// builds 600 projections, in slices, and reads no other person's records.
#[tokio::test]
async fn a_wide_refresh_builds_the_persons_it_affects_and_no_other() {
    use axum::http::Method;

    const BLOCKS: usize = 300;
    let (app, profiles, db, _directory) = file_backed_app().await;
    let (tree, _anchor) = common::family_blocks_tree(&app, &db, BLOCKS).await;
    let places = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/places?name=Southmere,%20Shire"),
        None,
    )
    .await;
    let place = places["edges"][0]["node"]["id"].as_str().unwrap();
    let born_in = |profiles: &[serde_json::Value], name: &str| {
        profiles
            .iter()
            .filter(|profile| profile["birth"]["place_name"] == name)
            .count()
    };
    // Two persons of each block of ten are born there.
    let born_there = BLOCKS * 2;
    assert_eq!(
        born_in(&common::all_profiles(&app, &tree).await, "Southmere, Shire"),
        born_there
    );

    let (built, rebuilds) = (profiles.built_profiles(), profiles.full_rebuilds());
    common::ok(
        &app,
        Method::PUT,
        &format!("/api/v1/trees/{tree}/places/{place}"),
        Some(serde_json::json!({ "name": "Southmere Magna, Shire" })),
    )
    .await;
    assert_eq!(
        profiles.built_profiles() - built,
        born_there as u64,
        "the refresh built other persons than those born in the place"
    );
    assert_eq!(profiles.full_rebuilds(), rebuilds, "the tree was rebuilt");

    let after = common::all_profiles(&app, &tree).await;
    assert_eq!(born_in(&after, "Southmere Magna, Shire"), born_there);
    assert_eq!(born_in(&after, "Southmere, Shire"), 0);
    let found = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/search?q=&limit=100"),
        None,
    )
    .await;
    let birth_places: Vec<&str> = found["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["birth_place"].as_str())
        .collect();
    assert!(
        birth_places.contains(&"Southmere Magna, Shire")
            && !birth_places.contains(&"Southmere, Shire"),
        "the search rows follow: {birth_places:?}"
    );
}

/// A refresh skips the persons of its set that are not live persons of the
/// tree — unknown, or another tree's — instead of failing or projecting
/// them, however small the set.
#[tokio::test]
async fn a_refresh_skips_who_is_not_a_person_of_the_tree() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, child, _family) = create_family_trio(&db, tree_id).await;
    service.rebuild_tree_full(&db, tree_id).await.unwrap();
    let other_tree = create_tree(&db).await;
    let outsider =
        create_named_person(&db, other_tree, Sex::Female, "Given_x", "BRANCH_X", None).await;

    service
        .invalidate_for_mutation(&db, tree_id, &[father, Uuid::now_v7(), outsider, child])
        .await
        .expect("the refresh goes on without them");
    assert_eq!(PersonDenormRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert!(
        PersonDenormRepo::get(&db, tree_id, outsider)
            .await
            .unwrap()
            .is_none(),
        "a person of another tree is not projected under this one"
    );
    assert_eq!(
        service
            .search(tree_id, "branch_x", 10, 0)
            .await
            .unwrap()
            .total_count,
        0
    );
}

/// Re-cutting a surname at its particle refreshes its bearers and their
/// relatives in the transaction of the change: the tree is not rebuilt.
#[tokio::test]
async fn a_surname_recut_refreshes_its_bearers_without_rebuilding_the_tree() {
    use axum::http::Method;

    let (app, profiles, db, _directory) = file_backed_app().await;
    let (tree, _anchor) = common::family_blocks_tree(&app, &db, 3).await;
    let bearer = common::new_person(&app, &tree).await;
    common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/persons/{bearer}/names"),
        Some(serde_json::json!({
            "name_type": "birth",
            "given_names": "Given_a",
            "surname_prefix": "LE",
            "surname": "BRANCH",
            "is_primary": true,
        })),
    )
    .await;
    let (built, rebuilds) = (profiles.built_profiles(), profiles.full_rebuilds());
    let out = common::ok(
        &app,
        Method::PATCH,
        &format!("/api/v1/trees/{tree}/dictionary/family-names/particle"),
        Some(serde_json::json!({ "value": "LE BRANCH", "particle": "" })),
    )
    .await;
    assert_eq!(out["names_updated"], 1);
    assert_eq!(profiles.full_rebuilds(), rebuilds, "the tree was rebuilt");
    assert_eq!(profiles.built_profiles() - built, 1, "the bearer alone");
    let profile = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/profiles/{bearer}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_name"]["surname"], "LE BRANCH");
}

// ─────────────────────── Sliced rebuilds ─────────────────────────────

/// A projection counts its person's notes and citations without reading
/// them, and counts the same whether the whole tree or the person alone is
/// rebuilt: the live notes, and the citations of live sources.
#[tokio::test]
async fn a_projection_counts_the_live_notes_and_the_citations_of_live_sources() {
    use axum::http::Method;
    use serde_json::json;

    let (app, _profiles, _db, _directory) = file_backed_app().await;
    let tree = common::new_tree(&app, "Counts").await;
    let person = common::new_person(&app, &tree).await;
    let created = |path: &str, body: serde_json::Value| {
        let (app, uri) = (app.clone(), format!("/api/v1/trees/{tree}/{path}"));
        async move {
            common::ok(&app, Method::POST, &uri, Some(body)).await["id"]
                .as_str()
                .unwrap()
                .to_owned()
        }
    };
    let kept = created("sources", json!({ "title": "Sample register" })).await;
    let dropped = created("sources", json!({ "title": "Other register" })).await;
    for source in [&kept, &dropped] {
        created(
            "citations",
            json!({ "source_id": source, "person_id": person, "page": "f. 12" }),
        )
        .await;
    }
    created(
        "notes",
        json!({ "text": "Sample remark", "person_id": person }),
    )
    .await;
    let removed = created(
        "notes",
        json!({ "text": "Other remark", "person_id": person }),
    )
    .await;
    for record in [format!("notes/{removed}"), format!("sources/{dropped}")] {
        common::ok(
            &app,
            Method::DELETE,
            &format!("/api/v1/trees/{tree}/{record}"),
            None,
        )
        .await;
    }

    for rebuild in [
        "profiles/rebuild".to_owned(),
        format!("profiles/rebuild/{person}"),
    ] {
        common::ok(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree}/{rebuild}"),
            None,
        )
        .await;
        let profile = common::ok(
            &app,
            Method::GET,
            &format!("/api/v1/trees/{tree}/profiles/{person}"),
            None,
        )
        .await;
        assert_eq!(profile["citation_count"], 1, "{rebuild}: {profile}");
        assert_eq!(profile["note_count"], 1, "{rebuild}: {profile}");
    }
}

/// A whole-tree rebuild builds the tree a slice at a time and writes what
/// rebuilding each person alone writes: slicing loses nobody at a slice's
/// edge and changes no projection.
#[tokio::test]
async fn a_whole_tree_rebuild_writes_what_each_person_rebuilt_alone_would() {
    use axum::http::Method;

    // 600 persons: two slices, the second one partial.
    const BLOCKS: usize = 60;
    let (app, profiles, db, _directory) = file_backed_app().await;
    let (tree, _anchor) = common::family_blocks_tree(&app, &db, BLOCKS).await;
    let tree_id: Uuid = tree.parse().unwrap();
    let payloads = |profiles: Vec<serde_json::Value>| -> Vec<serde_json::Value> {
        profiles
            .into_iter()
            .map(|mut profile| {
                // When it was built says nothing of what was built.
                profile.as_object_mut().unwrap().remove("built_at");
                profile
            })
            .collect()
    };

    let built = profiles.built_profiles();
    common::ok(
        &app,
        Method::POST,
        &format!("/api/v1/trees/{tree}/profiles/rebuild"),
        None,
    )
    .await;
    assert_eq!(profiles.built_profiles() - built, (BLOCKS * 10) as u64);
    let whole = payloads(common::all_profiles(&app, &tree).await);
    assert_eq!(whole.len(), BLOCKS * 10);

    for profile in &whole {
        let person: Uuid = profile["person_id"].as_str().unwrap().parse().unwrap();
        profiles.rebuild_one(tree_id, person).await.unwrap();
    }
    assert_eq!(payloads(common::all_profiles(&app, &tree).await), whole);
    let found = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/search?q="),
        None,
    )
    .await;
    assert_eq!(found["total_count"], BLOCKS * 10);
}

// ─────────────────────── The most persons of a pedigree ──────────────

/// A descent wider than a pedigree holds is cut at a generation, on REST
/// and GraphQL alike: the generation that would cross the bound is left
/// out with those beyond, the pedigree says how deep it reaches and that
/// it was cut, and an expansion past the cut brings nothing more.
#[tokio::test]
async fn a_pedigree_past_the_most_persons_stops_at_a_generation_and_says_so() {
    use axum::http::Method;
    use oxidgene_api::service::pedigrees::MAX_PEDIGREE_PERSONS;

    // A root with 70 children, each with 60: 4,200 grandchildren.
    const CHILDREN: usize = 70;
    const GRANDCHILDREN: usize = 60;
    const { assert!(CHILDREN + CHILDREN * GRANDCHILDREN > MAX_PEDIGREE_PERSONS) };
    let mut gedcom = String::from("0 HEAD\n1 GEDC\n2 VERS 5.5.1\n1 CHAR UTF-8\n");
    gedcom += "0 @R@ INDI\n1 NAME Rootgiven /BRANCH_R/\n1 SEX M\n1 FAMS @FR@\n";
    gedcom += "0 @FR@ FAM\n1 HUSB @R@\n";
    for child in 0..CHILDREN {
        gedcom += &format!("1 CHIL @C{child}@\n");
    }
    for child in 0..CHILDREN {
        gedcom += &format!(
            "0 @C{child}@ INDI\n1 NAME Child_{child} /BRANCH_R/\n1 SEX M\n\
             1 FAMC @FR@\n1 FAMS @F{child}@\n0 @F{child}@ FAM\n1 HUSB @C{child}@\n"
        );
        for grandchild in 0..GRANDCHILDREN {
            gedcom += &format!("1 CHIL @G{child}_{grandchild}@\n");
        }
        for grandchild in 0..GRANDCHILDREN {
            gedcom += &format!(
                "0 @G{child}_{grandchild}@ INDI\n1 NAME Grandchild_{grandchild} /BRANCH_R/\n\
                 1 SEX F\n1 FAMC @F{child}@\n"
            );
        }
    }
    gedcom += "0 TRLR\n";

    let (app, _profiles, db, _directory) = file_backed_app().await;
    let tree = common::new_tree(&app, "Wide descent").await;
    common::import_gedcom(&app, &db, &tree, &gedcom).await;
    let found = common::ok(
        &app,
        Method::GET,
        &format!("/api/v1/trees/{tree}/persons/search?q=Rootgiven"),
        None,
    )
    .await;
    let root = found["entries"][0]["person_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pedigree = |depth: usize| {
        let (app, uri) = (
            app.clone(),
            format!(
                "/api/v1/trees/{tree}/pedigree/{root}?ancestor_depth=0&descendant_depth={depth}"
            ),
        );
        async move { common::ok(&app, Method::GET, &uri, None).await }
    };

    // One generation fits: nothing is cut, and nothing says so.
    let children = pedigree(1).await;
    assert_eq!(children["persons"].as_object().unwrap().len(), 1 + CHILDREN);
    assert_eq!(children["descendant_depth_loaded"], 1);
    assert!(children.get("truncated").is_none(), "{children}");

    // Two would not: the second generation is left out whole.
    let cut = pedigree(3).await;
    assert_eq!(cut["persons"].as_object().unwrap().len(), 1 + CHILDREN);
    assert_eq!(cut["descendant_depth_loaded"], 1);
    assert_eq!(cut["truncated"], true);

    // Expanding past the cut brings nothing, and says why.
    let expanded = common::ok(
        &app,
        Method::GET,
        &format!(
            "/api/v1/trees/{tree}/pedigree/{root}/expand\
             ?direction=descendants&from_depth=1&to_depth=2"
        ),
        None,
    )
    .await;
    assert_eq!(expanded["new_nodes"], serde_json::json!([]));
    assert_eq!(expanded["descendant_depth_loaded"], 1);
    assert_eq!(expanded["truncated"], true);

    if cfg!(feature = "graphql") {
        let data = common::gql_ok(
            &app,
            "query($tree: ID!, $root: ID!) {
                pedigree(treeId: $tree, rootPersonId: $root, ancestorDepth: 0, descendantDepth: 3) {
                    truncated descendantDepthLoaded nodes { personId }
                }
                expandPedigree(treeId: $tree, rootPersonId: $root, direction: DESCENDANTS,
                               fromDepth: 1, toDepth: 2) {
                    truncated descendantDepthLoaded newNodes { personId }
                }
            }",
            serde_json::json!({ "tree": tree, "root": root }),
        )
        .await;
        assert_eq!(data["pedigree"]["truncated"], true);
        assert_eq!(data["pedigree"]["descendantDepthLoaded"], 1);
        assert_eq!(
            data["pedigree"]["nodes"].as_array().unwrap().len(),
            1 + CHILDREN
        );
        assert_eq!(data["expandPedigree"]["truncated"], true);
        assert_eq!(data["expandPedigree"]["descendantDepthLoaded"], 1);
        assert_eq!(data["expandPedigree"]["newNodes"], serde_json::json!([]));
    }
}
