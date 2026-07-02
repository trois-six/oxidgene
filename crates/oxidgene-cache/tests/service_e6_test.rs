//! Sprint E.6 integration tests: DB-native search (`person_search_fts`) and
//! the person-cache-less desktop path (targeted person builds, one-pass
//! pedigree builds).
//!
//! All tests run `CacheService` against in-memory SQLite with a
//! `MemoryCacheStore` (which no longer caches persons), i.e. the exact
//! desktop configuration.

use std::sync::Arc;
use std::time::Instant;

use oxidgene_cache::service::CacheService;
use oxidgene_cache::store::memory::MemoryCacheStore;
use oxidgene_core::enums::{ChildType, EventType, NameType, Sex, SpouseRole};
use oxidgene_db::repo::{
    EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo, PersonNameRepo, PersonRepo,
    PersonSearchRepo, TreeRepo, connect, run_migrations,
};
use oxidgene_db::sea_orm::DatabaseConnection;
use uuid::Uuid;

async fn setup() -> (DatabaseConnection, CacheService) {
    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrations");
    let service = CacheService::new(Arc::new(MemoryCacheStore::new()), db.clone());
    (db, service)
}

async fn create_tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "E6 Tree".into(), None)
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
        Some(given.into()),
        Some(surname.into()),
        None,
        None,
        None,
        true,
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
        )
        .await
        .expect("birth event");
    }
    id
}

/// Create father + mother + child linked through a family with a marriage event.
async fn create_family_trio(db: &DatabaseConnection, tree_id: Uuid) -> (Uuid, Uuid, Uuid, Uuid) {
    let father = create_named_person(db, tree_id, Sex::Male, "Jean", "Dupont", Some(1850)).await;
    let mother =
        create_named_person(db, tree_id, Sex::Female, "Éloïse", "Lefèvre", Some(1855)).await;
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
    )
    .await
    .expect("marriage");

    (father, mother, child, family_id)
}

#[tokio::test]
async fn search_populates_lazily_and_matches() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    // person_search_fts is empty — the first search populates it lazily.
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
    let result = service.search(tree_id, "dupont", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 2, "Jean + Pierre Dupont");
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);

    // Accent-folded match.
    let result = service.search(tree_id, "eloise", 10, 0).await.unwrap();
    assert_eq!(result.total_count, 1);
    assert_eq!(result.entries[0].display_name, "Éloïse Lefèvre");
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
async fn rebuild_tree_full_populates_fts() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    let count = service.rebuild_tree_full(tree_id).await.unwrap();
    assert_eq!(count, 3);
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);
}

#[tokio::test]
async fn targeted_person_build_denormalizes_family() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, family_id) = create_family_trio(&db, tree_id).await;

    // Child: family_as_child with both parents' display names.
    let cached_child = service.get_or_build_person(tree_id, child).await.unwrap();
    assert_eq!(
        cached_child.primary_name.as_ref().unwrap().display_name,
        "Pierre Dupont"
    );
    let as_child = cached_child.family_as_child.expect("child link");
    assert_eq!(as_child.family_id, family_id);
    assert_eq!(as_child.father_id, Some(father));
    assert_eq!(as_child.father_display_name.as_deref(), Some("Jean Dupont"));
    assert_eq!(
        as_child.mother_display_name.as_deref(),
        Some("Éloïse Lefèvre")
    );
    assert!(cached_child.birth.is_some());

    // Father: families_as_spouse with spouse name, children and marriage.
    let cached_father = service.get_or_build_person(tree_id, father).await.unwrap();
    assert_eq!(cached_father.families_as_spouse.len(), 1);
    let link = &cached_father.families_as_spouse[0];
    assert_eq!(link.spouse_id, Some(mother));
    assert_eq!(link.spouse_display_name.as_deref(), Some("Éloïse Lefèvre"));
    assert!(link.children_ids.contains(&child));
    assert!(link.marriage.is_some(), "marriage event denormalized");
}

#[tokio::test]
async fn name_mutation_updates_search_rows() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, _mother, _child, _family) = create_family_trio(&db, tree_id).await;

    // Populate the search table.
    service.rebuild_tree_full(tree_id).await.unwrap();
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
        Some("Marcel".into()),
        Some("Dupont".into()),
        None,
        None,
        None,
        true,
    )
    .await
    .unwrap();

    // Same invalidation entry point the REST/GraphQL handlers use.
    service
        .invalidate_for_person(tree_id, father)
        .await
        .unwrap();

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
}

#[tokio::test]
async fn person_delete_removes_search_row() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (_father, mother, _child, _family) = create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(tree_id).await.unwrap();
    assert_eq!(
        service
            .search(tree_id, "lefevre", 10, 0)
            .await
            .unwrap()
            .total_count,
        1
    );

    PersonRepo::delete(&db, mother).await.unwrap();
    service
        .invalidate_for_person_delete(tree_id, mother)
        .await
        .unwrap();

    assert_eq!(
        service
            .search(tree_id, "lefevre", 10, 0)
            .await
            .unwrap()
            .total_count,
        0
    );
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 2);
}

#[tokio::test]
async fn tree_invalidation_clears_search_rows() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    create_family_trio(&db, tree_id).await;

    service.rebuild_tree_full(tree_id).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);

    service.invalidate_tree(tree_id).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
}

#[tokio::test]
async fn pedigree_builds_without_person_cache() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;
    let (father, mother, child, _family) = create_family_trio(&db, tree_id).await;

    // Closure table entries (normally maintained by the family handlers).
    use oxidgene_db::repo::PersonAncestryRepo;
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, father, child, 1)
        .await
        .unwrap();
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, mother, child, 1)
        .await
        .unwrap();

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

    // Second read comes from the pedigree cache (still stored).
    let again = service
        .get_or_build_pedigree(tree_id, child, 2, 1)
        .await
        .unwrap();
    assert_eq!(again.persons.len(), pedigree.persons.len());
}

/// Performance regression guard (Sprint E.6): with the person cache removed
/// on desktop, person loads and searches must stay within the interactive
/// budget (< 100 ms in debug builds; release is ~10× faster).
#[tokio::test]
async fn person_load_and_search_performance() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let surnames = ["Perraud", "Dupont", "Lefèvre", "Martin", "Bernard"];
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

    // Full rebuild (GEDCOM-import path) — populates person_search_fts.
    let t0 = Instant::now();
    service.rebuild_tree_full(tree_id).await.unwrap();
    let rebuild = t0.elapsed();

    // Targeted single-person load (person detail page path).
    let t1 = Instant::now();
    let cached = service
        .get_or_build_person(tree_id, person_id)
        .await
        .unwrap();
    let person_load = t1.elapsed();
    assert_eq!(cached.person_id, person_id);

    // FTS search.
    let t2 = Instant::now();
    let result = service.search(tree_id, "perraud 17", 20, 0).await.unwrap();
    let search = t2.elapsed();
    assert!(result.total_count > 0);

    println!(
        "E6 perf (2k persons): rebuild_tree_full={rebuild:?}, \
         person_load={person_load:?}, search={search:?}"
    );
    assert!(
        person_load.as_millis() < 100,
        "targeted person load took {person_load:?}, expected < 100ms"
    );
    assert!(
        search.as_millis() < 100,
        "FTS search took {search:?}, expected < 100ms"
    );
}

/// Large-tree benchmark approximating a big GEDCOM import (20K persons).
/// Ignored by default — run with `cargo test -p oxidgene-cache -- --ignored`.
#[tokio::test]
#[ignore = "benchmark — run manually"]
async fn bench_large_tree_20k() {
    let (db, service) = setup().await;
    let tree_id = create_tree(&db).await;

    let surnames = [
        "Perraud", "Dupont", "Lefèvre", "Martin", "Bernard", "Moreau",
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
    service.rebuild_tree_full(tree_id).await.unwrap();
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
        "E6 bench (20k persons): rebuild_tree_full={rebuild:?}, \
         person_load={person_load:?}, search={search:?} ({} hits)",
        result.total_count
    );
}
