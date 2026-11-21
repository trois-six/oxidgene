//! Integration tests for the person search: the `person_search` rows and
//! the words of `person_search_token`.
//!
//! Runs against in-memory SQLite, and against PostgreSQL under `just
//! test-postgres`: the same tables, statements and answers on both. The
//! plan checks read how each backend runs a search's statements.

use chrono::Utc;
use oxidgene_core::enums::Sex;
use oxidgene_core::search::fold_words;
use oxidgene_db::entities::{person, person_search_token, sea_enums};
use oxidgene_db::repo::{
    PersonRepo, PersonSearchEntry, PersonSearchFilters, PersonSearchRepo, PersonSearchRow,
    PersonSearchSort, RelativeNames, TreeRepo,
};
use sea_orm::{
    ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait,
    PaginatorTrait, QueryFilter, Statement, TransactionTrait,
};
use uuid::Uuid;

use crate::common::setup_db;

/// A fresh database and a tree of it.
async fn setup() -> (DatabaseConnection, Uuid) {
    let db = setup_db().await;
    let tree = new_tree(&db).await;
    (db, tree)
}

async fn new_tree(db: &DatabaseConnection) -> Uuid {
    let tree = Uuid::now_v7();
    TreeRepo::create(db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    tree
}

/// `count` new persons of `tree`, inserted a few hundred at a time.
async fn persons(db: &DatabaseConnection, tree: Uuid, count: usize) -> Vec<Uuid> {
    let ids: Vec<Uuid> = (0..count).map(|_| Uuid::now_v7()).collect();
    let now = Utc::now();
    for chunk in ids.chunks(500) {
        person::Entity::insert_many(chunk.iter().map(|id| person::ActiveModel {
            id: Set(*id),
            tree_id: Set(tree),
            sex: Set(sea_enums::Sex::Unknown),
            privacy: Set(sea_enums::Privacy::Public),
            portrait_media_id: Set(None),
            portrait_vignette_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }))
        .exec(db)
        .await
        .unwrap();
    }
    ids
}

/// The search entry of a new person of `tree`.
async fn entry(
    db: &DatabaseConnection,
    tree_id: Uuid,
    surname: &str,
    given_names: &str,
    birth_year: Option<i32>,
    death_year: Option<i32>,
) -> PersonSearchEntry {
    let person_id = Uuid::now_v7();
    PersonRepo::create(db, person_id, tree_id, Sex::Male)
        .await
        .unwrap();
    entry_of(
        person_id,
        tree_id,
        surname,
        given_names,
        birth_year,
        death_year,
    )
}

/// The search entry of person `person_id`.
fn entry_of(
    person_id: Uuid,
    tree_id: Uuid,
    surname: &str,
    given_names: &str,
    birth_year: Option<i32>,
    death_year: Option<i32>,
) -> PersonSearchEntry {
    PersonSearchEntry {
        row: PersonSearchRow {
            person_id,
            tree_id,
            surname_key: fold_words(surname),
            given_key: fold_words(given_names),
            maiden_key: None,
            sex: sea_enums::Sex::Male,
            birth_year,
            birth_qualifier: sea_enums::DateQualifier::Exact,
            death_year,
            death_qualifier: sea_enums::DateQualifier::Exact,
            date_sort: None,
            display_name: format!("{given_names} {surname}"),
            surname_display: surname.to_owned(),
            given_names_display: given_names.to_owned(),
            birth_place: None,
            spouse_names: String::new(),
            father_name: None,
            mother_name: None,
            children_count: 0,
        },
        relatives: RelativeNames::default(),
    }
}

async fn search(db: &DatabaseConnection, tree: Uuid, query: &str) -> Vec<String> {
    PersonSearchRepo::search(db, tree, query, 100, 0)
        .await
        .unwrap()
        .entries
        .into_iter()
        .map(|row| row.display_name)
        .collect()
}

/// The display names a search with `filters` alone finds, in name order.
async fn filtered(
    db: &DatabaseConnection,
    tree: Uuid,
    filters: &PersonSearchFilters,
) -> Vec<String> {
    PersonSearchRepo::search_filtered(db, tree, "", filters, PersonSearchSort::NameAsc, 100, 0)
        .await
        .unwrap()
        .entries
        .into_iter()
        .map(|row| row.display_name)
        .collect()
}

/// How many words `person_search_token` holds, of every tree.
async fn words(db: &DatabaseConnection) -> u64 {
    person_search_token::Entity::find().count(db).await.unwrap()
}

#[tokio::test]
async fn an_empty_tree_finds_nobody() {
    let (db, tree_id) = setup().await;

    let page = PersonSearchRepo::search(&db, tree_id, "", 10, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 0);
    assert!(page.entries.is_empty());
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
    assert!(!PersonSearchRepo::has_tree(&db, tree_id).await.unwrap());
}

#[tokio::test]
async fn search_prefix_and_accent_folding() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(
            &db,
            tree_id,
            "RICHARD",
            "Pierre Marie",
            Some(1842),
            Some(1901),
        )
        .await,
        entry(&db, tree_id, "Dupont", "Jean", Some(1850), None).await,
        entry(&db, tree_id, "Lefèvre", "Éloïse", Some(1861), Some(1920)).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 3);
    assert!(PersonSearchRepo::has_tree(&db, tree_id).await.unwrap());

    // Accent-folded query matches accent-folded words.
    assert_eq!(search(&db, tree_id, "Eloise").await, ["Éloïse Lefèvre"]);
    // Accented query is folded before matching.
    assert_eq!(search(&db, tree_id, "lefèvre").await, ["Éloïse Lefèvre"]);
    // Prefix matching.
    assert_eq!(search(&db, tree_id, "rich").await, ["Pierre Marie RICHARD"]);
    // Multi-word: all words must match, in any order.
    assert_eq!(search(&db, tree_id, "richard pierre").await.len(), 1);
    assert_eq!(search(&db, tree_id, "pierre richard").await.len(), 1);
    assert!(search(&db, tree_id, "richard jean").await.is_empty());
    // Birth and death years are words; a prefix of one finds it.
    assert_eq!(search(&db, tree_id, "dupont 1850").await, ["Jean Dupont"]);
    assert_eq!(search(&db, tree_id, "192").await, ["Éloïse Lefèvre"]);
    // A word inside another is not the start of one.
    assert!(search(&db, tree_id, "pont").await.is_empty());
}

#[tokio::test]
async fn a_name_filter_matches_the_words_it_begins() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(&db, tree_id, "de la Cruz", "Anne-Sophie", None, None).await,
        entry(&db, tree_id, "Dupont", "Jean Baptiste", None, None).await,
        entry(&db, tree_id, "Dupuis", "Sophie", None, None).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    let by = |surname: &str, given_names: &str| PersonSearchFilters {
        surname: (!surname.is_empty()).then(|| surname.to_owned()),
        given_names: (!given_names.is_empty()).then(|| given_names.to_owned()),
        ..PersonSearchFilters::default()
    };

    // A particle or a compound: a later word of the name.
    assert_eq!(
        filtered(&db, tree_id, &by("cruz", "")).await,
        ["Anne-Sophie de la Cruz"]
    );
    assert_eq!(
        filtered(&db, tree_id, &by("De La C", "")).await,
        ["Anne-Sophie de la Cruz"]
    );
    assert_eq!(
        filtered(&db, tree_id, &by("", "sophie")).await,
        ["Anne-Sophie de la Cruz", "Sophie Dupuis"]
    );
    assert_eq!(
        filtered(&db, tree_id, &by("Dup", "")).await,
        ["Jean Baptiste Dupont", "Sophie Dupuis"]
    );
    assert_eq!(
        filtered(&db, tree_id, &by("dup", "baptiste jean")).await,
        ["Jean Baptiste Dupont"]
    );
    // The inside of a word is not matched, nor another field's word.
    assert!(filtered(&db, tree_id, &by("pont", "")).await.is_empty());
    assert!(filtered(&db, tree_id, &by("sophie", "")).await.is_empty());
}

#[tokio::test]
async fn years_are_filtered_as_numbers() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(&db, tree_id, "Example", "Early", Some(850), Some(901)).await,
        entry(&db, tree_id, "Example", "Middle", Some(1849), Some(1917)).await,
        entry(&db, tree_id, "Example", "Late", Some(1990), None).await,
        entry(&db, tree_id, "Example", "Unknown", None, None).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    let born = |from: Option<i32>, to: Option<i32>| PersonSearchFilters {
        surname: Some("example".into()),
        birth_from: from,
        birth_to: to,
        ..PersonSearchFilters::default()
    };

    // 850 is before 1849 as a number, where "850" sorts after "1849".
    assert_eq!(
        filtered(&db, tree_id, &born(None, Some(1849))).await,
        ["Early Example", "Middle Example"]
    );
    assert_eq!(
        filtered(&db, tree_id, &born(Some(1000), None)).await,
        ["Late Example", "Middle Example"]
    );
    let died = PersonSearchFilters {
        death_from: Some(900),
        death_to: Some(1000),
        ..PersonSearchFilters::default()
    };
    assert_eq!(filtered(&db, tree_id, &died).await, ["Early Example"]);
    // A year alone, without a name: the tree's birth years.
    let alone = PersonSearchFilters {
        birth_from: Some(1900),
        ..PersonSearchFilters::default()
    };
    assert_eq!(filtered(&db, tree_id, &alone).await, ["Late Example"]);
    // The free text finds a year as it is shown, four digits at least.
    assert_eq!(search(&db, tree_id, "0850").await, ["Early Example"]);
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

#[tokio::test]
async fn relative_filters_are_accent_folded() {
    let (db, tree_id) = setup().await;
    let mut first = entry(&db, tree_id, "Branch A", "Child One", None, None).await;
    first.relatives.spouse_surnames = vec![fold_words("Lebâtard")];
    first.relatives.spouse_given_names = vec![fold_words("Perrine")];
    let mut second = entry(&db, tree_id, "Branch B", "Child Two", None, None).await;
    second.relatives.spouse_surnames = vec![fold_words("Moreau")];
    PersonSearchRepo::replace_tree(&db, tree_id, &[first, second])
        .await
        .unwrap();

    assert_eq!(count_by_spouse_surname(&db, tree_id, "lebatard").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "Lebâtard").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "LEBAT").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "moreau").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "nobody").await, 0);
    // A relative's name is not the person's own: the free text and the
    // person's own filters do not find it.
    assert!(search(&db, tree_id, "moreau").await.is_empty());
    let own = PersonSearchFilters {
        given_names: Some("perrine".into()),
        ..PersonSearchFilters::default()
    };
    assert!(filtered(&db, tree_id, &own).await.is_empty());
}

#[tokio::test]
async fn a_filter_cannot_match_across_two_spouses() {
    let (db, tree_id) = setup().await;
    let mut subject = entry(&db, tree_id, "Branch A", "Child One", None, None).await;
    subject.relatives.spouse_surnames = vec![fold_words("Dupont"), fold_words("Martin")];
    PersonSearchRepo::replace_tree(&db, tree_id, &[subject])
        .await
        .unwrap();

    // Each spouse is still found on its own.
    assert_eq!(count_by_spouse_surname(&db, tree_id, "dupont").await, 1);
    assert_eq!(count_by_spouse_surname(&db, tree_id, "martin").await, 1);
    // But a span across two of them is not a spouse anyone has.
    assert_eq!(count_by_spouse_surname(&db, tree_id, "tmar").await, 0);
    assert_eq!(
        count_by_spouse_surname(&db, tree_id, "dupontmartin").await,
        0
    );
}

#[tokio::test]
async fn relevance_ranks_a_prefix_above_a_later_match() {
    let (db, tree_id) = setup().await;
    let entries = [
        // Sorts first by name, so name order alone would put it on top.
        entry(&db, tree_id, "Abbott", "Martin", None, None).await,
        entry(&db, tree_id, "de Martin", "Ada", None, None).await,
        entry(&db, tree_id, "Martin", "Zoe", None, None).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();

    let db = &db;
    let ranked = |sort| async move {
        PersonSearchRepo::search_filtered(
            db,
            tree_id,
            "martin",
            &PersonSearchFilters::default(),
            sort,
            10,
            0,
        )
        .await
        .unwrap()
        .entries
        .into_iter()
        .map(|row| row.display_name)
        .collect::<Vec<_>>()
    };
    // The surname beginning with the term, then the given names beginning
    // with it, then the match further in.
    assert_eq!(
        ranked(PersonSearchSort::Relevance).await,
        ["Zoe Martin", "Martin Abbott", "Ada de Martin"]
    );
    // Name ordering stays available and unranked.
    assert_eq!(
        ranked(PersonSearchSort::NameAsc).await,
        ["Martin Abbott", "Ada de Martin", "Zoe Martin"]
    );
    assert_eq!(
        ranked(PersonSearchSort::NameDesc).await,
        ["Zoe Martin", "Ada de Martin", "Martin Abbott"]
    );

    // A search made of a structured filter ranks by it.
    let filters = PersonSearchFilters {
        surname: Some("martin".into()),
        ..PersonSearchFilters::default()
    };
    let page = PersonSearchRepo::search_filtered(
        db,
        tree_id,
        "",
        &filters,
        PersonSearchSort::Relevance,
        10,
        0,
    )
    .await
    .unwrap();
    let names: Vec<String> = page
        .entries
        .into_iter()
        .map(|row| row.display_name)
        .collect();
    assert_eq!(names, ["Zoe Martin", "Ada de Martin"]);
}

#[tokio::test]
async fn browse_mode_sorted_and_paginated() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(&db, tree_id, "Zola", "Émile", None, None).await,
        entry(&db, tree_id, "Alembert", "Jean", None, None).await,
        entry(&db, tree_id, "Moreau", "Anne", None, None).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();

    // Empty query = browse mode, sorted by surname.
    let page = PersonSearchRepo::search(&db, tree_id, "", 2, 0)
        .await
        .unwrap();
    assert_eq!(page.total_count, 3);
    let surnames: Vec<&str> = page
        .entries
        .iter()
        .map(|r| r.surname_key.as_str())
        .collect();
    assert_eq!(surnames, ["alembert", "moreau"]);

    // Second page.
    let page = PersonSearchRepo::search(&db, tree_id, "", 2, 2)
        .await
        .unwrap();
    assert_eq!(page.total_count, 3);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].surname_key, "zola");
}

#[tokio::test]
async fn upsert_and_delete() {
    let (db, tree_id) = setup().await;
    let mut e = entry(&db, tree_id, "Martin", "Paul", Some(1900), None).await;
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&e))
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 1);
    let first_words = words(&db).await;

    // Upsert with a changed name replaces the row and its words instead of
    // adding to them.
    e.row.surname_key = fold_words("Bernard");
    e.row.display_name = "Paul Bernard".into();
    PersonSearchRepo::upsert(&db, std::slice::from_ref(&e))
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 1);
    assert_eq!(words(&db).await, first_words);
    assert_eq!(search(&db, tree_id, "bernard").await, ["Paul Bernard"]);
    assert!(search(&db, tree_id, "martin").await.is_empty());

    // Delete removes the row and its words.
    PersonSearchRepo::delete_person(&db, e.row.person_id)
        .await
        .unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 0);
    assert_eq!(words(&db).await, 0);
}

/// A person deleted from `person` takes their search row and its words
/// with them, by cascade.
#[tokio::test]
async fn a_deleted_person_takes_their_row_and_words() {
    let (db, tree_id) = setup().await;
    let kept = entry(&db, tree_id, "Example", "Kept", None, None).await;
    let gone = entry(&db, tree_id, "Example", "Gone", Some(1900), None).await;
    PersonSearchRepo::replace_tree(&db, tree_id, &[kept.clone(), gone.clone()])
        .await
        .unwrap();
    let before = words(&db).await;

    person::Entity::delete_by_id(gone.row.person_id)
        .exec(&db)
        .await
        .unwrap();

    assert_eq!(search(&db, tree_id, "example").await, ["Kept Example"]);
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(), 1);
    // Their "example", "gone" and "1900".
    assert_eq!(words(&db).await, before - 3);
    let left = person_search_token::Entity::find()
        .filter(person_search_token::Column::PersonId.eq(gone.row.person_id))
        .count(&db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn trees_are_isolated() {
    let db = setup_db().await;
    let tree_a = new_tree(&db).await;
    let tree_b = new_tree(&db).await;
    let in_a = entry(&db, tree_a, "Durand", "Luc", None, None).await;
    let in_b = entry(&db, tree_b, "Durand", "Léa", None, None).await;
    PersonSearchRepo::replace_tree(&db, tree_a, &[in_a])
        .await
        .unwrap();
    PersonSearchRepo::replace_tree(&db, tree_b, &[in_b])
        .await
        .unwrap();

    assert_eq!(search(&db, tree_a, "durand").await, ["Luc Durand"]);
    let in_b = filtered(
        &db,
        tree_b,
        &PersonSearchFilters {
            surname: Some("durand".into()),
            ..PersonSearchFilters::default()
        },
    )
    .await;
    assert_eq!(in_b, ["Léa Durand"]);

    PersonSearchRepo::delete_tree(&db, tree_a).await.unwrap();
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_a).await.unwrap(), 0);
    assert_eq!(PersonSearchRepo::count_tree(&db, tree_b).await.unwrap(), 1);
    assert_eq!(search(&db, tree_b, "durand").await, ["Léa Durand"]);
}

/// Every script's accents and the letters that do not decompose fold the
/// same way on both sides: a stroke, a ligature, a Vietnamese tone mark, and
/// a hyphenated name typed with a space.
#[tokio::test]
async fn letters_of_every_script_fold_alike() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(&db, tree_id, "Łącka", "Zofia", None, None).await,
        entry(&db, tree_id, "Nguyễn", "Thị", None, None).await,
        entry(&db, tree_id, "Cæsar", "Anne-Sophie", None, None).await,
        entry(&db, tree_id, "Иванова", "Мария", None, None).await,
    ];
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    for (query, expected) in [
        ("lacka", "Zofia Łącka"),
        ("nguyen thi", "Thị Nguyễn"),
        ("caesar", "Anne-Sophie Cæsar"),
        ("anne sophie", "Anne-Sophie Cæsar"),
        // A prefix outside ASCII: its range ends at the next code point.
        ("иван", "Мария Иванова"),
    ] {
        assert_eq!(search(&db, tree_id, query).await, [expected], "{query}");
    }
}

#[tokio::test]
async fn homonyms_share_the_folded_name() {
    let (db, tree_id) = setup().await;
    let first = entry(&db, tree_id, "Fixture", "Alpha", None, None).await;
    let mut second = entry(&db, tree_id, "FIXTURE", "Álpha", Some(1900), None).await;
    second.row.date_sort = chrono::NaiveDate::from_ymd_opt(1900, 1, 1);
    let other = entry(&db, tree_id, "Fixture", "Beta", None, None).await;
    let half = entry(&db, tree_id, "Fixture", "", None, None).await;
    PersonSearchRepo::replace_tree(
        &db,
        tree_id,
        &[first.clone(), second.clone(), other, half.clone()],
    )
    .await
    .unwrap();

    let homonyms = PersonSearchRepo::homonyms(&db, tree_id, first.row.person_id, 10)
        .await
        .unwrap();
    let ids: Vec<Uuid> = homonyms.iter().map(|row| row.person_id).collect();
    assert_eq!(ids, [second.row.person_id]);
    assert!(
        PersonSearchRepo::homonyms(&db, tree_id, half.row.person_id, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Re-indexing more persons than SQLite binds in one statement (32 766)
/// replaces their rows: the old rows are deleted a bounded chunk at a time.
#[tokio::test]
async fn upsert_replaces_more_rows_than_one_statement_binds() {
    let (db, tree_id) = setup().await;
    let entries: Vec<PersonSearchEntry> = persons(&db, tree_id, 33_000)
        .await
        .into_iter()
        .enumerate()
        .map(|(n, id)| entry_of(id, tree_id, "Example", &format!("Person {n}"), None, None))
        .collect();
    PersonSearchRepo::upsert(&db, &entries).await.unwrap();
    PersonSearchRepo::upsert(&db, &entries).await.unwrap();
    assert_eq!(
        PersonSearchRepo::count_tree(&db, tree_id).await.unwrap(),
        33_000
    );
    // "example", "person", and each number.
    assert_eq!(words(&db).await, 33_000 * 3);
}

/// A page past the last match still reports how many matches there are, so
/// a stale pager does not read as an empty search.
#[tokio::test]
async fn a_page_past_the_end_keeps_the_total() {
    let (db, tree_id) = setup().await;
    let entries = vec![
        entry(&db, tree_id, "Example", "Ann", None, None).await,
        entry(&db, tree_id, "Example", "Bob", None, None).await,
        entry(&db, tree_id, "Sample", "Cid", None, None).await,
    ];
    PersonSearchRepo::upsert(&db, &entries).await.unwrap();

    let page = PersonSearchRepo::search(&db, tree_id, "example", 10, 10)
        .await
        .unwrap();
    assert!(page.entries.is_empty());
    assert_eq!(page.total_count, 2);
}

/// An offset past `i64::MAX` reads no page, which PostgreSQL would refuse.
#[tokio::test]
async fn a_huge_offset_returns_an_empty_page() {
    let (db, tree_id) = setup().await;
    let entries = [entry(&db, tree_id, "Example", "Ann", None, None).await];
    PersonSearchRepo::upsert(&db, &entries).await.unwrap();
    let page = PersonSearchRepo::search(&db, tree_id, "", 10, u64::MAX)
        .await
        .unwrap();
    assert!(page.entries.is_empty());
    assert_eq!(page.total_count, 1);
}

// ── Plans ───────────────────────────────────────────────────────────────

/// The searches whose plans are checked: what the search page, its
/// suggestions, the pickers and the assistant send.
fn planned_searches() -> Vec<(
    &'static str,
    &'static str,
    PersonSearchFilters,
    PersonSearchSort,
)> {
    let surname = |value: &str| PersonSearchFilters {
        surname: Some(value.into()),
        ..PersonSearchFilters::default()
    };
    vec![
        (
            "surname prefix",
            "",
            surname("exa"),
            PersonSearchSort::Relevance,
        ),
        (
            "given name and surname",
            "",
            PersonSearchFilters {
                given_names: Some("ann".into()),
                ..surname("example")
            },
            PersonSearchSort::NameAsc,
        ),
        (
            "surname prefix and birth years",
            "",
            PersonSearchFilters {
                birth_from: Some(1800),
                birth_to: Some(1850),
                ..surname("exa")
            },
            PersonSearchSort::BirthAsc,
        ),
        (
            "free text",
            "ann exa",
            PersonSearchFilters::default(),
            PersonSearchSort::Relevance,
        ),
        (
            "relative",
            "",
            PersonSearchFilters {
                father_surname: Some("exa".into()),
                ..PersonSearchFilters::default()
            },
            PersonSearchSort::NameAsc,
        ),
        (
            "birth years alone",
            "",
            PersonSearchFilters {
                birth_from: Some(1800),
                birth_to: Some(1850),
                ..PersonSearchFilters::default()
            },
            PersonSearchSort::NameAsc,
        ),
        (
            "browse",
            "",
            PersonSearchFilters::default(),
            PersonSearchSort::NameAsc,
        ),
    ]
}

/// The plan lines of `statement` as its backend runs it. On PostgreSQL,
/// with sequential scans priced out: a plan that still reads a search
/// table whole has no index for the statement, whatever the table holds.
async fn plan(db: &DatabaseConnection, statement: &Statement) -> Vec<String> {
    let backend = db.get_database_backend();
    let explained = Statement {
        sql: match backend {
            DbBackend::Sqlite => format!("EXPLAIN QUERY PLAN {}", statement.sql),
            _ => format!("EXPLAIN {}", statement.sql),
        },
        ..statement.clone()
    };
    let txn = db.begin().await.unwrap();
    if backend == DbBackend::Postgres {
        txn.execute_unprepared("SET LOCAL enable_seqscan = off")
            .await
            .unwrap();
    }
    let rows = txn.query_all_raw(explained).await.unwrap();
    txn.rollback().await.unwrap();
    let column = match backend {
        DbBackend::Sqlite => "detail",
        _ => "QUERY PLAN",
    };
    rows.iter()
        .map(|row| row.try_get::<String>("", column).unwrap())
        .collect()
}

/// Whether a plan line reads a search table whole: SQLite's bare `SCAN`,
/// PostgreSQL's `Seq Scan`.
fn reads_whole(line: &str) -> bool {
    let line = line.trim_start().trim_start_matches("->").trim_start();
    let sqlite = line.starts_with("SCAN person_search") && !line.contains(" USING ");
    let postgres = line.starts_with("Seq Scan on person_search");
    sqlite || postgres
}

/// Every search's count and page reads the search tables through their
/// indexes, on the backend under test: SQLite here, PostgreSQL under `just
/// test-postgres`, where the weekly job catches a sequential scan.
#[tokio::test]
async fn every_search_reads_the_search_tables_through_an_index() {
    let (db, tree_id) = setup().await;
    let mut entries = Vec::new();
    for n in 0..20 {
        let mut entry = entry(
            &db,
            tree_id,
            "Example",
            &format!("Ann {n}"),
            Some(1790 + n),
            None,
        )
        .await;
        entry.relatives.father_surname = Some("example".into());
        entries.push(entry);
    }
    PersonSearchRepo::replace_tree(&db, tree_id, &entries)
        .await
        .unwrap();
    // No statistics: on a test tree they would describe a handful of rows,
    // which SQLite rightly reads whole. Without, it takes any usable index,
    // so a full read means the statement has none — as the SQL plans guard
    // reads its plans. PostgreSQL prices sequential scans out instead.

    let backend = db.get_database_backend();
    for (name, query, filters, sort) in planned_searches() {
        let statements =
            PersonSearchRepo::search_statements(backend, tree_id, query, &filters, sort, 25, 0);
        for statement in [&statements.count, &statements.page] {
            let lines = plan(&db, statement).await;
            assert!(
                !lines.iter().any(|line| reads_whole(line)),
                "{name}: {}\n{}",
                statement.sql,
                lines.join("\n")
            );
            if !query.is_empty() || filters.surname.is_some() || filters.father_surname.is_some() {
                let words = lines.iter().any(|line| {
                    line.contains("person_search_token USING PRIMARY KEY")
                        || line.contains("pk_person_search_token")
                });
                assert!(words, "{name} reads no word range:\n{}", lines.join("\n"));
            }
        }
        // And answers on this backend.
        PersonSearchRepo::search_filtered(&db, tree_id, query, &filters, sort, 25, 0)
            .await
            .unwrap();
    }
}

#[test]
fn a_plan_line_reading_a_table_whole_is_recognised() {
    assert!(reads_whole("SCAN person_search"));
    assert!(reads_whole("  ->  Seq Scan on person_search_token t"));
    assert!(!reads_whole(
        "SEARCH person_search_token USING PRIMARY KEY (tree_id=? AND token>? AND token<?)"
    ));
    assert!(!reads_whole(
        "SCAN person_search USING COVERING INDEX idx_person_search_tree_name"
    ));
    assert!(!reads_whole(
        "Index Only Scan using pk_person_search_token on person_search_token"
    ));
}
