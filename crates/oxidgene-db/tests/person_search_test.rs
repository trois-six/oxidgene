//! Integration tests for the `person_search_fts` table (Sprint E.6).
//!
//! Runs against in-memory SQLite, which also verifies that the bundled
//! SQLite is compiled with FTS5 support (the migration would fail otherwise).

use oxidgene_core::search::fold_words;
use oxidgene_db::repo::{
    PersonSearchEntry, PersonSearchFilters, PersonSearchRepo, PersonSearchSort, connect,
    run_migrations,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
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
        surname: fold_words(surname),
        given_names: fold_words(given_names),
        maiden_name: None,
        birth_year: birth_year.map(str::to_owned),
        death_year: death_year.map(str::to_owned),
        sex: "male".into(),
        display_name: format!("{given_names} {surname}"),
        surname_display: surname.to_owned(),
        given_names_display: given_names.to_owned(),
        birth_place: None,
        date_sort: None,
        birth_qualifier: "exact".into(),
        death_qualifier: "exact".into(),
        spouse_names: String::new(),
        spouse_surnames: String::new(),
        spouse_given_names: String::new(),
        father_name: None,
        father_surname: None,
        father_given_names: None,
        mother_name: None,
        mother_surname: None,
        mother_given_names: None,
        children_count: 0,
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
            "RICHARD",
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
    let page = PersonSearchRepo::search(&db, tree_id, "rich", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.entries[0].surname, "richard");

    // Multi-word: all words must match (surname + given names).
    let page = PersonSearchRepo::search(&db, tree_id, "richard pierre", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);

    // Word order doesn't matter.
    let page = PersonSearchRepo::search(&db, tree_id, "pierre richard", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);

    // Non-matching word combination.
    let page = PersonSearchRepo::search(&db, tree_id, "richard jean", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);

    // Birth year is searchable.
    let page = PersonSearchRepo::search(&db, tree_id, "dupont 1850", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
}

/// Total hits for a `spouse_surname` filter alone.
async fn count_by_spouse_surname(db: &DatabaseConnection, tree_id: Uuid, surname: &str) -> u64 {
    let filters = PersonSearchFilters {
        spouse_surname: Some(surname.to_owned()),
        ..Default::default()
    };
    PersonSearchRepo::search_filtered(db, tree_id, "", &filters, PersonSearchSort::NameAsc, 10, 0)
        .await
        .unwrap()
        .total_count
}

/// `entry` plus a spouse, for the filters that match on a relative's name.
fn entry_with_spouse(
    tree_id: Uuid,
    surname: &str,
    given_names: &str,
    spouse_surname: &str,
    spouse_given_names: &str,
) -> PersonSearchEntry {
    PersonSearchEntry {
        spouse_names: format!("{spouse_given_names} {spouse_surname}"),
        spouse_surnames: fold_words(spouse_surname),
        spouse_given_names: fold_words(spouse_given_names),
        ..entry(tree_id, surname, given_names, None, None)
    }
}

#[tokio::test]
async fn relative_filters_are_accent_folded() {
    // The subject's own name filters have always folded accents; the spouse
    // and parent ones matched raw `person_name` rows and did not, so someone
    // searching "lebatard" found nobody married to a "Lebâtard".
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    PersonSearchRepo::replace_tree(
        &db,
        tree_id,
        &[
            entry_with_spouse(tree_id, "Branch A", "Child One", "Lebâtard", "Perrine"),
            entry_with_spouse(tree_id, "Branch B", "Child Two", "Moreau", "Anne"),
        ],
    )
    .await
    .unwrap();

    assert_eq!(count_by_spouse_surname(&db, tree_id, "lebatard").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "Lebâtard").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "LEBATARD").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "moreau").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "nobody").await, 0);
}

#[tokio::test]
async fn a_filter_cannot_match_across_two_spouses() {
    // Several spouses share one column. Joined with a plain space, a substring
    // filter could match the end of one name and the start of the next; the
    // U+001F separator is what stops that.
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let mut subject = entry(tree_id, "Branch A", "Child One", None, None);
    subject.spouse_surnames = format!("{}\u{1f}{}", fold_words("Dupont"), fold_words("Martin"));
    PersonSearchRepo::replace_tree(&db, tree_id, &[subject])
        .await
        .unwrap();

    // Each spouse is still found on its own.
    assert_eq!(count_by_spouse_surname(&db, tree_id, "dupont").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "martin").await, 1);
    // But a span across the boundary is not a spouse anyone has.
    assert_eq!(count_by_spouse_surname(&db, tree_id, "tmar").await, 0);
    assert_eq!(
        count_by_spouse_surname(&db, tree_id, "dupontmartin").await,
        0
    );
}

#[tokio::test]
async fn relevance_ranks_a_prefix_above_a_later_match() {
    // `relevance` used to be an alias for name-ascending, so it ranked
    // nothing. It now puts what the searcher typed, matched as a prefix, first
    // — and it has to do so for a structured filter too, because that is what
    // the search page sends.
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    PersonSearchRepo::replace_tree(
        &db,
        tree_id,
        &[
            // Sorts first by name, so name order alone would put it on top.
            entry(tree_id, "Abbott", "Martin", None, None),
            entry(tree_id, "Martin", "Zoe", None, None),
        ],
    )
    .await
    .unwrap();

    let filters = PersonSearchFilters::default();
    let page = PersonSearchRepo::search_filtered(
        &db,
        tree_id,
        "martin",
        &filters,
        PersonSearchSort::Relevance,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!(page.total_count, 2);
    assert_eq!(
        page.entries[0].surname, "martin",
        "a surname beginning with the term outranks a given name"
    );

    // Name ordering stays available and unranked.
    let page = PersonSearchRepo::search_filtered(
        &db,
        tree_id,
        "martin",
        &filters,
        PersonSearchSort::NameAsc,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!(page.entries[0].surname, "abbott");
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
    e.surname = fold_words("Bernard");
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
/// Ignored by default — run with `cargo test -p oxidgene-db -- --ignored`.
///
/// The assertion is wall-clock, so running it alongside the rest of the suite
/// measured contention rather than the query: it failed under a loaded
/// `cargo nextest run --workspace` and passed on its own, at the same commit.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn search_performance_10k() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();

    let surnames = [
        "Richard", "Dupont", "Lefèvre", "Martin", "Bernard", "Moreau",
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
    let page = PersonSearchRepo::search(&db, tree_id, "richard 17", 20, 0)
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

/// Every script's accents and the letters that do not decompose fold the
/// same way on both sides: a stroke, a ligature, a Vietnamese tone mark, and
/// a hyphenated name typed with a space.
#[tokio::test]
async fn letters_of_every_script_fold_alike() {
    let db = setup_db().await;
    let tree_id = Uuid::now_v7();
    let entries = vec![
        entry(tree_id, "Łącka", "Zofia", None, None),
        entry(tree_id, "Nguyễn", "Thị", None, None),
        entry(tree_id, "Cæsar", "Anne-Sophie", None, None),
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    for (query, expected) in [
        ("lacka", "Zofia Łącka"),
        ("nguyen thi", "Thị Nguyễn"),
        ("caesar", "Anne-Sophie Cæsar"),
        ("anne sophie", "Anne-Sophie Cæsar"),
    ] {
        let page = PersonSearchRepo::search(&db, tree_id, query, 10, 0)
            .await
            .unwrap();
        assert_eq!(page.total_count, 1, "{query}");
        assert_eq!(page.entries[0].display_name, expected, "{query}");
    }
}

/// How many search rows, keys, and rows matched to their own key there are.
async fn rows_and_keys(db: &DatabaseConnection) -> (i64, i64, i64) {
    let count = |sql: &str| {
        let sql = sql.to_owned();
        async move {
            db.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        }
    };
    (
        count("SELECT COUNT(*) AS n FROM person_search_fts").await,
        count("SELECT COUNT(*) AS n FROM person_search_key").await,
        count(
            "SELECT COUNT(*) AS n FROM person_search_fts f \
             JOIN person_search_key k ON k.fts_rowid = f.rowid \
             AND k.person_id = f.person_id AND k.tree_id = f.tree_id",
        )
        .await,
    )
}

/// On SQLite every write keeps the key table in step with the search rows:
/// one key per row, carrying that row's rowid, person and tree.
#[tokio::test]
async fn search_keys_stay_in_step_with_the_rows() {
    let db = setup_db().await;
    let (tree_a, tree_b) = (Uuid::now_v7(), Uuid::now_v7());
    let first = entry(tree_a, "Fixture", "Alpha", None, None);
    let second = entry(tree_a, "Fixture", "Alpha", Some("1900"), None);
    let other = entry(tree_b, "Example", "Beta", None, None);

    PersonSearchRepo::replace_tree(&db, tree_a, &[first.clone(), second.clone()])
        .await
        .unwrap();
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&other))
        .await
        .unwrap();
    assert_eq!(rows_and_keys(&db).await, (3, 3, 3));
    assert!(PersonSearchRepo::has_tree(&db, tree_b).await.unwrap());
    assert!(
        !PersonSearchRepo::has_tree(&db, Uuid::now_v7())
            .await
            .unwrap()
    );

    let homonyms = PersonSearchRepo::homonyms(&db, tree_a, first.person_id, 10)
        .await
        .unwrap();
    assert_eq!(homonyms.len(), 1);
    assert_eq!(homonyms[0].person_id, second.person_id);

    // An upsert replaces the row and its key alike.
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&first))
        .await
        .unwrap();
    assert_eq!(rows_and_keys(&db).await, (3, 3, 3));

    PersonSearchRepo::delete_person(&db, second.person_id)
        .await
        .unwrap();
    assert_eq!(rows_and_keys(&db).await, (2, 2, 2));
    PersonSearchRepo::delete_tree(&db, tree_a).await.unwrap();
    assert_eq!(rows_and_keys(&db).await, (1, 1, 1));
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_b).await.unwrap(), 1);
}

/// The person and tree lookups on a 100,000-row search table spread over
/// four trees. Each is an index probe through the key table, where a filter
/// on the FTS5 table's unindexed `person_id` or `tree_id` read every row:
/// measured with SQLite 3.53 at 280 ms to find five persons, 114 ms to learn
/// that a tree has no row, 83 ms to read one person's name, against well
/// under a millisecond each through the keys.
/// Ignored by default — run with `cargo test -p oxidgene-db -- --ignored`.
#[tokio::test]
#[ignore = "benchmark: run by `just bench` in release mode"]
async fn person_lookups_performance_100k() {
    let db = setup_db().await;
    let trees: Vec<Uuid> = (0..4).map(|_| Uuid::now_v7()).collect();
    for (index, tree_id) in trees.iter().enumerate() {
        let entries: Vec<PersonSearchEntry> = (0..25_000)
            .map(|i| {
                entry(
                    *tree_id,
                    &format!("Fixture{}", i % 997),
                    "Alpha",
                    None,
                    None,
                )
            })
            .collect();
        PersonSearchRepo::replace_tree(&db, *tree_id, &entries)
            .await
            .unwrap();
        if index == 0 {
            let probe = &entries[12_345];
            let started = std::time::Instant::now();
            PersonSearchRepo::homonyms(&db, *tree_id, probe.person_id, 10)
                .await
                .unwrap();
            println!("homonyms: {:?}", started.elapsed());
        }
    }
    let started = std::time::Instant::now();
    assert!(
        !PersonSearchRepo::has_tree(&db, Uuid::now_v7())
            .await
            .unwrap()
    );
    let missing_tree = started.elapsed();
    let started = std::time::Instant::now();
    assert_eq!(
        PersonSearchRepo::count_tree(&db, trees[1]).await.unwrap(),
        25_000
    );
    let count = started.elapsed();
    let started = std::time::Instant::now();
    PersonSearchRepo::delete_person(&db, Uuid::now_v7())
        .await
        .unwrap();
    let delete = started.elapsed();
    println!("missing tree: {missing_tree:?}, count: {count:?}, delete: {delete:?}");
    assert!(missing_tree.as_millis() < 10 && delete.as_millis() < 10);
}
