//! Tags on records, enforced by the database and the repositories that write
//! them: the `record_tag` constraint, folding, merges, deletion, live counts,
//! the media and person filters, and person merges.

use chrono::Utc;
use oxidgene_core::search::fold_words;
use oxidgene_core::types::RecordKind;
use oxidgene_core::{Calendar, DateQualifier, EventType, OxidGeneError, Sex};
use oxidgene_db::entities::{record_tag, tag};
use oxidgene_db::repo::{
    CitationRepo, EventDetails, EventRepo, MediaFilter, MediaLibraryRepo, MediaRepo,
    PaginationParams, PersonMergeRepo, PersonRepo, PersonSearchEntry, PersonSearchFilters,
    PersonSearchRepo, PersonSearchSort, PlaceRepo, RecordTagRepo, SourceRepo, TagRepo, TreeRepo,
    connect, run_migrations,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait, PaginatorTrait,
};
use uuid::Uuid;

async fn setup() -> (DatabaseConnection, Uuid) {
    let db = connect("sqlite::memory:").await.unwrap();
    run_migrations(&db).await.unwrap();
    let tree = Uuid::now_v7();
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    (db, tree)
}

async fn person(db: &DatabaseConnection, tree: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    PersonRepo::create(db, id, tree, Sex::Unknown)
        .await
        .unwrap();
    id
}

async fn tag_named(db: &DatabaseConnection, tree: Uuid, label: &str) -> tag::Model {
    TagRepo::create(db, tree, label).await.unwrap().0
}

async fn links(db: &DatabaseConnection) -> u64 {
    record_tag::Entity::find().count(db).await.unwrap()
}

fn count_of(usage: &[oxidgene_db::repo::TagUsage], id: Uuid, kind: RecordKind) -> i64 {
    usage
        .iter()
        .find(|entry| entry.tag.id == id)
        .map_or(-1, |entry| entry.count(kind))
}

#[tokio::test]
async fn check_constraint_requires_exactly_one_record() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Fictional").await;
    let p = person(&db, tree).await;
    let q = person(&db, tree).await;

    let link = |person_id: Option<Uuid>, note_id: Option<Uuid>| record_tag::ActiveModel {
        id: Set(Uuid::now_v7()),
        tree_id: Set(tree),
        tag_id: Set(tag.id),
        person_id: Set(person_id),
        note_id: Set(note_id),
        created_at: Set(Utc::now()),
        ..Default::default()
    };
    assert!(link(None, None).insert(&db).await.is_err(), "no record");
    assert!(
        link(Some(p), Some(Uuid::now_v7()))
            .insert(&db)
            .await
            .is_err(),
        "two records"
    );
    assert!(link(Some(p), None).insert(&db).await.is_ok());
    assert!(link(Some(q), None).insert(&db).await.is_ok());
}

#[tokio::test]
async fn unique_index_makes_adding_a_tag_idempotent() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Fictional").await;
    let p = person(&db, tree).await;

    assert!(
        RecordTagRepo::add(&db, tree, tag.id, RecordKind::Person, p)
            .await
            .unwrap()
    );
    assert!(
        !RecordTagRepo::add(&db, tree, tag.id, RecordKind::Person, p)
            .await
            .unwrap()
    );
    assert_eq!(links(&db).await, 1);

    // The partial unique index refuses the same link written behind the
    // repository's back.
    let raw = record_tag::ActiveModel {
        id: Set(Uuid::now_v7()),
        tree_id: Set(tree),
        tag_id: Set(tag.id),
        person_id: Set(Some(p)),
        created_at: Set(Utc::now()),
        ..Default::default()
    };
    assert!(raw.insert(&db).await.is_err());
    assert!(
        RecordTagRepo::remove(&db, tree, tag.id, RecordKind::Person, p)
            .await
            .unwrap()
    );
    assert!(
        !RecordTagRepo::remove(&db, tree, tag.id, RecordKind::Person, p)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn bulk_add_returns_exactly_the_links_it_inserted() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Fictional").await;
    let (p, q, r) = (
        person(&db, tree).await,
        person(&db, tree).await,
        person(&db, tree).await,
    );
    RecordTagRepo::add(&db, tree, tag.id, RecordKind::Person, p)
        .await
        .unwrap();

    // A person already carrying the tag, and a repeated id, are not counted.
    let mut added = RecordTagRepo::add_persons(&db, tree, tag.id, &[p, q, q, r])
        .await
        .unwrap();
    added.sort();
    let mut expected = vec![q, r];
    expected.sort();
    assert_eq!(added, expected);
    assert_eq!(links(&db).await, 3);

    // Adding to persons who all carry the tag inserts nothing and names none.
    assert!(
        RecordTagRepo::add_persons(&db, tree, tag.id, &[p, q, r])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(links(&db).await, 3);
}

#[tokio::test]
async fn bulk_remove_returns_exactly_the_links_it_deleted() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Fictional").await;
    let (p, q, r, s) = (
        person(&db, tree).await,
        person(&db, tree).await,
        person(&db, tree).await,
        person(&db, tree).await,
    );
    RecordTagRepo::add_persons(&db, tree, tag.id, &[p, q, r])
        .await
        .unwrap();
    // A writer outside the bulk call takes the tag off `p` first: the bulk
    // call must not name it, since it deletes nothing for it.
    assert!(
        RecordTagRepo::remove(&db, tree, tag.id, RecordKind::Person, p)
            .await
            .unwrap()
    );

    // `s` never carried the tag, and a repeated id is removed once.
    let mut removed = RecordTagRepo::remove_persons(&db, tree, tag.id, &[p, q, q, r, s])
        .await
        .unwrap();
    removed.sort();
    let mut expected = vec![q, r];
    expected.sort();
    assert_eq!(removed, expected);
    assert_eq!(links(&db).await, 0);

    // Removing from persons who no longer carry the tag names none.
    assert!(
        RecordTagRepo::remove_persons(&db, tree, tag.id, &[p, q, r, s])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn labels_that_fold_alike_are_one_tag() {
    let (db, tree) = setup().await;
    let (first, created) = TagRepo::create(&db, tree, "  À vérifier ").await.unwrap();
    assert!(created);
    assert_eq!(first.label, "À vérifier");
    let (again, created) = TagRepo::create(&db, tree, "a verifier").await.unwrap();
    assert!(!created);
    assert_eq!(again.id, first.id);
    assert_eq!(again.label, "À vérifier", "the first spelling is kept");
    assert_eq!(first.normalized_label, fold_words("à vérifier"));

    // Another tree may use the same label.
    let other = Uuid::now_v7();
    TreeRepo::create(&db, other, "Other tree".into(), None)
        .await
        .unwrap();
    assert!(TagRepo::create(&db, other, "À VERIFIER").await.unwrap().1);
}

#[tokio::test]
async fn labels_must_have_one_to_eighty_characters() {
    let (db, tree) = setup().await;
    assert!(matches!(
        TagRepo::create(&db, tree, "   ").await,
        Err(OxidGeneError::Validation(_))
    ));
    let long = "x".repeat(81);
    assert!(matches!(
        TagRepo::create(&db, tree, &long).await,
        Err(OxidGeneError::Validation(_))
    ));
    assert!(TagRepo::create(&db, tree, &"x".repeat(80)).await.is_ok());
}

#[tokio::test]
async fn new_tags_take_the_least_used_colour() {
    let (db, tree) = setup().await;
    let mut colors = Vec::new();
    for n in 0..13 {
        colors.push(tag_named(&db, tree, &format!("Label {n:02}")).await.color);
    }
    let first_twelve: std::collections::BTreeSet<i16> = colors[..12].iter().copied().collect();
    assert_eq!(first_twelve, (0..12).collect());
    assert_eq!(colors[12], 0, "the thirteenth wraps to the lowest slot");
}

#[tokio::test]
async fn rename_and_recolour_check_their_inputs() {
    let (db, tree) = setup().await;
    let a = tag_named(&db, tree, "Alpha").await;
    let b = tag_named(&db, tree, "Beta").await;

    assert!(matches!(
        TagRepo::update(&db, tree, b.id, Some("ALPHA"), None).await,
        Err(OxidGeneError::Conflict(_))
    ));
    let renamed = TagRepo::update(&db, tree, b.id, Some("Gamma"), Some(7))
        .await
        .unwrap();
    assert_eq!((renamed.label.as_str(), renamed.color), ("Gamma", 7));
    assert_eq!(renamed.normalized_label, fold_words("gamma"));
    assert!(matches!(
        TagRepo::update(&db, tree, a.id, None, Some(12)).await,
        Err(OxidGeneError::Validation(_))
    ));
    assert!(matches!(
        TagRepo::update(&db, tree, a.id, None, Some(-1)).await,
        Err(OxidGeneError::Validation(_))
    ));
    // Another tree's tag is not found.
    let other = Uuid::now_v7();
    TreeRepo::create(&db, other, "Other tree".into(), None)
        .await
        .unwrap();
    assert!(matches!(
        TagRepo::get(&db, other, a.id).await,
        Err(OxidGeneError::NotFound { .. })
    ));
}

#[tokio::test]
async fn merge_moves_links_drops_duplicates_and_deletes_the_source() {
    let (db, tree) = setup().await;
    let from = tag_named(&db, tree, "From").await;
    let into = tag_named(&db, tree, "Into").await;
    let p1 = person(&db, tree).await;
    let p2 = person(&db, tree).await;
    for (tag, person) in [(from.id, p1), (into.id, p1), (from.id, p2)] {
        RecordTagRepo::add(&db, tree, tag, RecordKind::Person, person)
            .await
            .unwrap();
    }

    let moved = TagRepo::merge(&db, tree, from.id, into.id).await.unwrap();
    assert_eq!(
        moved, 1,
        "p2's link moves; p1's is dropped, as p1 has the target"
    );
    assert_eq!(links(&db).await, 2);
    let usage = TagRepo::list(&db, tree).await.unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(count_of(&usage, into.id, RecordKind::Person), 2);
    assert!(matches!(
        TagRepo::merge(&db, tree, into.id, into.id).await,
        Err(OxidGeneError::Validation(_))
    ));
}

#[tokio::test]
async fn delete_removes_the_tag_and_its_links() {
    let (db, tree) = setup().await;
    let doomed = tag_named(&db, tree, "Doomed").await;
    let keep = tag_named(&db, tree, "Keep").await;
    let p = person(&db, tree).await;
    RecordTagRepo::add(&db, tree, doomed.id, RecordKind::Person, p)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, keep.id, RecordKind::Person, p)
        .await
        .unwrap();

    assert_eq!(TagRepo::delete(&db, tree, doomed.id).await.unwrap(), 1);
    assert_eq!(links(&db).await, 1);
    let usage = TagRepo::list(&db, tree).await.unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].tag.id, keep.id);
    assert!(matches!(
        TagRepo::delete(&db, tree, doomed.id).await,
        Err(OxidGeneError::NotFound { .. })
    ));
}

#[tokio::test]
async fn counts_exclude_soft_deleted_records() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Counted").await;
    let live = person(&db, tree).await;
    let gone = person(&db, tree).await;
    let event = Uuid::now_v7();
    EventRepo::create(
        &db,
        event,
        tree,
        EventType::Occupation,
        None,
        None,
        None,
        Some(live),
        None,
        None,
        DateQualifier::default(),
        None,
        Calendar::default(),
        EventDetails::default(),
    )
    .await
    .unwrap();
    let place = Uuid::now_v7();
    PlaceRepo::create(&db, place, tree, "Fictional place".into(), None, None)
        .await
        .unwrap();
    for (kind, id) in [
        (RecordKind::Person, live),
        (RecordKind::Person, gone),
        (RecordKind::Event, event),
        (RecordKind::Place, place),
    ] {
        RecordTagRepo::add(&db, tree, tag.id, kind, id)
            .await
            .unwrap();
    }
    PersonRepo::delete(&db, gone).await.unwrap();
    EventRepo::delete(&db, event).await.unwrap();

    let usage = TagRepo::list(&db, tree).await.unwrap();
    assert_eq!(count_of(&usage, tag.id, RecordKind::Person), 1);
    assert_eq!(count_of(&usage, tag.id, RecordKind::Event), 0);
    // A place is removed outright, so its link is the only count left.
    assert_eq!(count_of(&usage, tag.id, RecordKind::Place), 1);
    assert_eq!(count_of(&usage, tag.id, RecordKind::Media), 0);
}

#[tokio::test]
async fn citations_of_a_soft_deleted_source_are_not_counted() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Sourced").await;
    let source = Uuid::now_v7();
    SourceRepo::create(
        &db,
        source,
        tree,
        "Fictional source".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let citation = Uuid::now_v7();
    CitationRepo::create(&db, citation, source, None, None, None, None, None, None)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, tag.id, RecordKind::Citation, citation)
        .await
        .unwrap();
    assert_eq!(
        count_of(
            &TagRepo::list(&db, tree).await.unwrap(),
            tag.id,
            RecordKind::Citation
        ),
        1
    );

    // The source's soft delete leaves its citation row, and the citation
    // with it is no longer live.
    SourceRepo::delete(&db, source).await.unwrap();
    let usage = TagRepo::list(&db, tree).await.unwrap();
    assert_eq!(count_of(&usage, tag.id, RecordKind::Citation), 0);
    let page = RecordTagRepo::records_of_tag(
        &db,
        tree,
        tag.id,
        RecordKind::Citation,
        &PaginationParams::default(),
    )
    .await
    .unwrap();
    assert!(page.edges.is_empty());
    assert_eq!(page.total_count, 0);
}

#[tokio::test]
async fn tags_of_records_are_sorted_by_folded_label() {
    let (db, tree) = setup().await;
    let zebra = tag_named(&db, tree, "Zèbre").await;
    let alpha = tag_named(&db, tree, "alpha").await;
    let p = person(&db, tree).await;
    let q = person(&db, tree).await;
    RecordTagRepo::add(&db, tree, zebra.id, RecordKind::Person, p)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, alpha.id, RecordKind::Person, p)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, zebra.id, RecordKind::Person, q)
        .await
        .unwrap();

    let tags = RecordTagRepo::tags_of(&db, RecordKind::Person, &[p, q])
        .await
        .unwrap();
    let labels: Vec<&str> = tags[&p].iter().map(|tag| tag.label.as_str()).collect();
    assert_eq!(labels, ["alpha", "Zèbre"]);
    assert_eq!(tags[&q].len(), 1);
    assert!(
        RecordTagRepo::tags_of(&db, RecordKind::Family, &[p])
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn records_of_a_tag_page_by_cursor_over_live_records() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Paged").await;
    let mut ids = Vec::new();
    for _ in 0..3 {
        let id = person(&db, tree).await;
        RecordTagRepo::add(&db, tree, tag.id, RecordKind::Person, id)
            .await
            .unwrap();
        ids.push(id);
    }
    ids.sort();
    PersonRepo::delete(&db, ids[2]).await.unwrap();

    let first = RecordTagRepo::records_of_tag(
        &db,
        tree,
        tag.id,
        RecordKind::Person,
        &PaginationParams {
            first: 1,
            after: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(first.total_count, 2);
    assert_eq!(first.edges[0].node, ids[0]);
    assert!(first.page_info.has_next_page);

    let second = RecordTagRepo::records_of_tag(
        &db,
        tree,
        tag.id,
        RecordKind::Person,
        &PaginationParams {
            first: 1,
            after: first.page_info.end_cursor.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(second.edges.len(), 1);
    assert_eq!(second.edges[0].node, ids[1]);
    assert!(!second.page_info.has_next_page);
}

#[tokio::test]
async fn media_tags_filter_and_count_live_documents() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Documents").await;
    let document = Uuid::now_v7();
    MediaRepo::create_document(
        &db,
        document,
        tree,
        Some("Fictional scan".into()),
        Utc::now(),
    )
    .await
    .unwrap();
    let untagged = Uuid::now_v7();
    MediaRepo::create_document(&db, untagged, tree, Some("Other scan".into()), Utc::now())
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, tag.id, RecordKind::Media, document)
        .await
        .unwrap();

    let filter = MediaFilter {
        tags: vec![tag.id],
        ..MediaFilter::default()
    };
    let listed = MediaLibraryRepo::list(&db, tree, &filter, &PaginationParams::default(), None)
        .await
        .unwrap();
    assert_eq!(listed.total_count, 1);
    assert_eq!(listed.edges[0].node.id, document);
    assert_eq!(listed.edges[0].node.tags.len(), 1);
    assert_eq!(listed.edges[0].node.tags[0].label, "Documents");

    let facets = MediaLibraryRepo::facets(&db, tree, &[]).await.unwrap();
    assert_eq!(facets.tags.len(), 1);
    assert_eq!(facets.tags[0].tag.id, tag.id);
    assert_eq!(facets.tags[0].count, 1);

    // Purging the document takes its links with it.
    MediaRepo::purge(&db, document).await.unwrap();
    assert_eq!(links(&db).await, 0);
}

#[tokio::test]
async fn person_search_filters_by_every_tag_given() {
    let (db, tree) = setup().await;
    let both = tag_named(&db, tree, "Both").await;
    let only = tag_named(&db, tree, "Only").await;
    let mut entries: Vec<PersonSearchEntry> = Vec::new();
    for n in 0..3 {
        let id = person(&db, tree).await;
        entries.push(PersonSearchEntry {
            person_id: id,
            ..entry(tree, &format!("Person {n}"))
        });
    }
    PersonSearchRepo::replace_tree(&db, tree, &entries)
        .await
        .unwrap();
    for (index, tags) in [(0, vec![&both, &only]), (1, vec![&both])] {
        for tag in tags {
            RecordTagRepo::add(
                &db,
                tree,
                tag.id,
                RecordKind::Person,
                entries[index].person_id,
            )
            .await
            .unwrap();
        }
    }

    let with_both = PersonSearchFilters {
        tags: vec![both.id],
        ..PersonSearchFilters::default()
    };
    let page = PersonSearchRepo::search_filtered(
        &db,
        tree,
        "",
        &with_both,
        PersonSearchSort::NameAsc,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!(page.total_count, 2);

    let with_both_and_only = PersonSearchFilters {
        tags: vec![both.id, only.id],
        ..PersonSearchFilters::default()
    };
    let page = PersonSearchRepo::search_filtered(
        &db,
        tree,
        "",
        &with_both_and_only,
        PersonSearchSort::NameAsc,
        10,
        0,
    )
    .await
    .unwrap();
    assert_eq!(page.total_count, 1);
    assert_eq!(page.entries[0].person_id, entries[0].person_id);
}

#[tokio::test]
async fn person_merge_gives_the_kept_person_both_sets_of_tags() {
    let (db, tree) = setup().await;
    let shared = tag_named(&db, tree, "Shared").await;
    let extra = tag_named(&db, tree, "Extra").await;
    let kept = person(&db, tree).await;
    let duplicate = person(&db, tree).await;
    RecordTagRepo::add(&db, tree, shared.id, RecordKind::Person, kept)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, shared.id, RecordKind::Person, duplicate)
        .await
        .unwrap();
    RecordTagRepo::add(&db, tree, extra.id, RecordKind::Person, duplicate)
        .await
        .unwrap();

    PersonMergeRepo::absorb(&db, tree, kept, duplicate)
        .await
        .unwrap();

    let tags = RecordTagRepo::tags_of(&db, RecordKind::Person, &[kept, duplicate])
        .await
        .unwrap();
    let labels: Vec<&str> = tags[&kept].iter().map(|tag| tag.label.as_str()).collect();
    assert_eq!(labels, ["Extra", "Shared"]);
    assert!(!tags.contains_key(&duplicate));
    assert_eq!(links(&db).await, 2, "the shared tag is not doubled");
}

#[tokio::test]
async fn purging_a_tree_removes_its_tags_and_links() {
    let (db, tree) = setup().await;
    let tag = tag_named(&db, tree, "Gone with the tree").await;
    let p = person(&db, tree).await;
    RecordTagRepo::add(&db, tree, tag.id, RecordKind::Person, p)
        .await
        .unwrap();

    TreeRepo::soft_delete(&db, tree).await.unwrap();
    TreeRepo::purge(&db, tree).await.unwrap();
    assert_eq!(links(&db).await, 0);
    assert_eq!(tag::Entity::find().count(&db).await.unwrap(), 0);
}

fn entry(tree_id: Uuid, name: &str) -> PersonSearchEntry {
    PersonSearchEntry {
        person_id: Uuid::nil(),
        tree_id,
        surname: fold_words(name),
        given_names: fold_words("Fictional"),
        maiden_name: None,
        birth_year: None,
        death_year: None,
        sex: "unknown".into(),
        display_name: name.to_owned(),
        surname_display: name.to_owned(),
        given_names_display: "Fictional".into(),
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
async fn links_refuse_records_and_tags_of_another_tree() {
    let (db, tree) = setup().await;
    let other = Uuid::now_v7();
    TreeRepo::create(&db, other, "Other fictional tree".into(), None)
        .await
        .unwrap();
    let tag = tag_named(&db, tree, "Fictional").await;
    let other_tag = tag_named(&db, other, "Fictional").await;
    let own = person(&db, tree).await;
    let foreign = person(&db, other).await;

    let own_place = Uuid::now_v7();
    PlaceRepo::create(&db, own_place, tree, "Fictional place".into(), None, None)
        .await
        .unwrap();
    let foreign_place = Uuid::now_v7();
    PlaceRepo::create(
        &db,
        foreign_place,
        other,
        "Fictional place".into(),
        None,
        None,
    )
    .await
    .unwrap();
    let own_source = Uuid::now_v7();
    SourceRepo::create(
        &db,
        own_source,
        tree,
        "Fictional source".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let own_citation = Uuid::now_v7();
    CitationRepo::create(
        &db,
        own_citation,
        own_source,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let foreign_source = Uuid::now_v7();
    SourceRepo::create(
        &db,
        foreign_source,
        other,
        "Fictional source".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let foreign_citation = Uuid::now_v7();
    CitationRepo::create(
        &db,
        foreign_citation,
        foreign_source,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // Records of this tree take the tag, citations through their source.
    assert!(
        RecordTagRepo::add(&db, tree, tag.id, RecordKind::Place, own_place)
            .await
            .unwrap()
    );
    assert!(
        RecordTagRepo::add(&db, tree, tag.id, RecordKind::Citation, own_citation)
            .await
            .unwrap()
    );

    // A record of another tree is refused, whatever its kind.
    for (kind, id) in [
        (RecordKind::Person, foreign),
        (RecordKind::Place, foreign_place),
        (RecordKind::Citation, foreign_citation),
    ] {
        assert!(
            matches!(
                RecordTagRepo::add(&db, tree, tag.id, kind, id).await,
                Err(OxidGeneError::NotFound { .. })
            ),
            "{kind:?} of another tree"
        );
    }

    // A tag of another tree cannot be put on a record of this tree, by the
    // repository or by a link written directly.
    assert!(
        RecordTagRepo::add(&db, tree, other_tag.id, RecordKind::Person, own)
            .await
            .is_err()
    );
    let raw = record_tag::ActiveModel {
        id: Set(Uuid::now_v7()),
        tree_id: Set(tree),
        tag_id: Set(other_tag.id),
        person_id: Set(Some(own)),
        created_at: Set(Utc::now()),
        ..Default::default()
    };
    assert!(raw.insert(&db).await.is_err());
    assert_eq!(links(&db).await, 2, "refused links are not written");
}
