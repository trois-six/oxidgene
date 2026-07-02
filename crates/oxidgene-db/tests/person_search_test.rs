//! Integration tests for the `person_search_fts` table (Sprint E.6).
//!
//! Runs against in-memory SQLite, which also verifies that the bundled
//! SQLite is compiled with FTS5 support (the migration would fail otherwise).

use oxidgene_core::search::normalize_for_search;
use oxidgene_db::repo::{PersonSearchEntry, PersonSearchRepo, connect, run_migrations};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

async fn setup_db() -> DatabaseConnection {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db)
        .await
        .expect("migrations (includes FTS5)");
    db
}

fn entry(
    tree_id: Uuid,
    surname: &str,
    given_names: &str,
    birth_year: Option<&str>,
    death_year: Option<&str>,
) -> PersonSearchEntry {
    PersonSearchEntry {
        person_id: Uuid::now_v7(),
        tree_id,
        surname: normalize_for_search(surname),
        given_names: normalize_for_search(given_names),
        maiden_name: None,
        birth_year: birth_year.map(str::to_owned),
        death_year: death_year.map(str::to_owned),
        sex: "male".into(),
        display_name: format!("{given_names} {surname}"),
        birth_place: None,
        date_sort: None,
    }
}

#[tokio::test]
async fn fts_table_created_and_empty_search_works() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let page = PersonSearchRepo::search(&db, tree_id, "", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);
    assert!(page.entries.is_empty());
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
}

#[tokio::test]
async fn search_prefix_and_accent_folding() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let entries = vec![
        entry(
            tree_id,
            "PERRAUD",
            "Pierre Marie",
            Some("1842"),
            Some("1901"),
        ),
        entry(tree_id, "Dupont", "Jean", Some("1850"), None),
        entry(tree_id, "Lefèvre", "Éloïse", Some("1861"), Some("1920")),
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);

    // Accent-folded query matches accent-folded tokens.
    let page = PersonSearchRepo::search(&db, tree_id, "Eloise", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.entries[0].display_name, "Éloïse Lefèvre");

    // Accented query is normalized before matching.
    let page = PersonSearchRepo::search(&db, tree_id, "lefèvre", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);

    // Prefix matching.
    let page = PersonSearchRepo::search(&db, tree_id, "perr", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.entries[0].surname, "perraud");

    // Multi-word: all words must match (surname + given names).
    let page = PersonSearchRepo::search(&db, tree_id, "perraud pierre", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);

    // Word order doesn't matter.
    let page = PersonSearchRepo::search(&db, tree_id, "pierre perraud", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);

    // Non-matching word combination.
    let page = PersonSearchRepo::search(&db, tree_id, "perraud jean", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);

    // Birth year is searchable.
    let page = PersonSearchRepo::search(&db, tree_id, "dupont 1850", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
}

#[tokio::test]
async fn browse_mode_sorted_and_paginated() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let entries = vec![
        entry(tree_id, "Zola", "Émile", None, None),
        entry(tree_id, "Alembert", "Jean", None, None),
        entry(tree_id, "Moreau", "Anne", None, None),
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();

    // Empty query = browse mode, sorted by surname.
    let page = PersonSearchRepo::search(&db, tree_id, "", 2, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 3);
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].surname, "alembert");
    assert_eq!(page.entries[1].surname, "moreau");

    // Second page.
    let page = PersonSearchRepo::search(&db, tree_id, "", 2, 2)
        .await
        .unwrap();
    assert_eq!(page.total_count, 3);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].surname, "zola");
}

#[tokio::test]
async fn upsert_and_delete() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let mut e = entry(tree_id, "Martin", "Paul", Some("1900"), None);
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&e))
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 1);

    // Upsert with a changed name replaces the row instead of duplicating it.
    e.surname = normalize_for_search("Bernard");
    e.display_name = "Paul Bernard".into();
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&e))
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 1);

    let page = PersonSearchRepo::search(&db, tree_id, "bernard", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    let page = PersonSearchRepo::search(&db, tree_id, "martin", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);

    // Delete removes the row.
    PersonSearchRepo::delete_person(&db, e.person_id)
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
}

#[tokio::test]
async fn trees_are_isolated() {
    let db = setup_db().await;
    let tree_a = Uuid::now_v7();
    let tree_b = Uuid::now_v7();

    PersonSearchRepo::replace_tree(&db, tree_a, &[entry(tree_a, "Durand", "Luc", None, None)])
        .await
        .unwrap();
    PersonSearchRepo::replace_tree(&db, tree_b, &[entry(tree_b, "Durand", "Léa", None, None)])
        .await
        .unwrap();

    let page = PersonSearchRepo::search(&db, tree_a, "durand", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.entries[0].given_names, "luc");

    PersonSearchRepo::delete_tree(&db, tree_a).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_a).await.unwrap(), 0);
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_b).await.unwrap(), 1);
}

/// Performance regression guard: FTS5 search on a 10K-person tree must stay
/// well under the 50 ms server-side search target from the caching spec.
#[tokio::test]
async fn search_performance_10k() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let surnames = [
        "Perraud", "Dupont", "Lefèvre", "Martin", "Bernard", "Moreau",
    ];
    let givens = ["Jean", "Pierre", "Marie", "Éloïse", "Luc", "Anne"];
    let entries: Vec<PersonSearchEntry> = (0..10_000)
        .map(|i| {
            entry(
                tree_id,
                &format!("{}{}", surnames[i % surnames.len()], i / surnames.len()),
                givens[i % givens.len()],
                Some(&format!("{}", 1700 + (i % 300))),
                None,
            )
        })
        .collect();

    let t0 = std::time::Instant::now();
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    let build = t0.elapsed();

    let t1 = std::time::Instant::now();
    let page = PersonSearchRepo::search(&db, tree_id, "perraud 17", 20, 0)
        .await
        .unwrap();
    let search = t1.elapsed();

    println!(
        "FTS build 10k: {build:?}, search: {search:?}, hits: {}",
        page.total_count
    );
    assert!(page.total_count > 0);
    assert!(
        search.as_millis() < 50,
        "FTS5 search took {search:?}, expected < 50ms"
    );
}
