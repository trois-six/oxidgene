//! Integration tests for [`ProfileService`] — the denormalized person
//! projections that replaced the `oxidgene-cache` crate.
//!
//! Everything runs against in-memory SQLite, i.e. the desktop configuration.
//! Since the projections live in `person_denorm` rather than in a process-local
//! cache, these tests can assert what a cache never could: that a projection is
//! durable across service instances and never observed stale after a mutation.

use std::time::Instant;

use oxidgene_api::profile::ProfileService;
use oxidgene_core::enums::{
    Calendar, ChildType, DateQualifier, EventType, NameType, Sex, SpouseRole,
};
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PersonDenormRepo, PersonNamePieces,
    PersonNameRepo, PersonRepo, PersonSearchRepo, TreeRepo, connect, run_migrations,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

async fn setup() -> (DatabaseConnection, ProfileService) {
    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrations");
    let service = ProfileService::new(db.clone());
    (db, service)
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
            None,
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
        None,
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
    let child_profile = service
        .get_or_build_person(&db, tree_id, child)
        .await
        .unwrap();
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
    let father_profile = service
        .get_or_build_person(&db, tree_id, father)
        .await
        .unwrap();
    assert_eq!(father_profile.families_as_spouse.len(), 1);
    let link = &father_profile.families_as_spouse[0];
    assert_eq!(link.spouse_id, Some(mother));
    assert_eq!(link.spouse_display_name.as_deref(), Some("Jane Smith"));
    assert!(link.children_ids.contains(&child));
    assert!(link.marriage.is_some(), "marriage event denormalized");
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

    service
        .get_or_build_person(&db, tree_id, father)
        .await
        .unwrap();

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
        .get_or_build_person(&db, tree_id, father)
        .await
        .unwrap();
    assert_eq!(profile.primary_name.unwrap().display_name, "Jean Dupont");
    assert_eq!(
        restarted.get_all_persons(&db, tree_id).await.unwrap().len(),
        3,
        "no rebuild needed after a restart"
    );
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
        .invalidate_for_person(&db, tree_id, father)
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
            None,
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
        .get_or_build_person(&db, tree_id, person_id)
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
        None,
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
        None,
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
            None,
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
            None,
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
        .invalidate_for_person(&db, tree_id, father)
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
#[ignore = "benchmark — run manually"]
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
        .get_or_build_person(&db, tree_id, person_id)
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
#[ignore = "benchmark — run manually"]
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
        .get_or_build_person(&db, tree_id, person_id)
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
        .invalidate_for_person(&txn, tree_id, father)
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
        .invalidate_for_person(&txn, tree_id, father)
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
