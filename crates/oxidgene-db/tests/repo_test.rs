//! Integration tests for the repository layer.
//!
//! All tests run against an in-memory SQLite database.

use oxidgene_core::enums::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{
    CitationRepo, EventFilter, EventRepo, FamilyChildRepo, FamilyRepo, FamilySpouseRepo,
    MediaLinkRepo, MediaRepo, NoteRepo, PaginationParams, PersonAncestryRepo, PersonNameRepo,
    PersonRepo, PlaceRepo, SourceRepo, TreeRepo, connect, run_migrations,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

/// Helper: create a fresh in-memory DB with migrations applied.
async fn setup_db() -> DatabaseConnection {
    let db = connect("sqlite::memory:")
        .await
        .expect("connect to in-memory SQLite");
    run_migrations(&db).await.expect("migrations");
    db
}

/// Helper: create a tree and return its ID.
async fn create_tree(db: &DatabaseConnection) -> Uuid {
    let id = Uuid::now_v7();
    TreeRepo::create(db, id, "Test Tree".into(), Some("A test tree".into()))
        .await
        .expect("create tree");
    id
}

/// Helper: create a person and return its ID.
async fn create_person(db: &DatabaseConnection, tree_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    PersonRepo::create(db, id, tree_id, Sex::Male)
        .await
        .expect("create person");
    id
}

// ───────────────────────── Tree tests ─────────────────────────

#[tokio::test]
async fn tree_crud() {
    let db = setup_db().await;
    let id = Uuid::now_v7();

    // Create
    let tree = TreeRepo::create(&db, id, "My Tree".into(), Some("desc".into()))
        .await
        .unwrap();
    assert_eq!(tree.id, id);
    assert_eq!(tree.name, "My Tree");
    assert_eq!(tree.description.as_deref(), Some("desc"));
    assert!(tree.deleted_at.is_none());

    // Get
    let fetched = TreeRepo::get(&db, id).await.unwrap();
    assert_eq!(fetched.id, id);

    // Update
    let updated = TreeRepo::update(&db, id, Some("Renamed".into()), None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.description.as_deref(), Some("desc")); // unchanged

    // Update description to None
    let updated2 = TreeRepo::update(&db, id, None, Some(None)).await.unwrap();
    assert!(updated2.description.is_none());

    // Soft-delete
    TreeRepo::delete(&db, id).await.unwrap();

    // Get after delete returns NotFound
    let err = TreeRepo::get(&db, id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

#[tokio::test]
async fn tree_list_pagination() {
    let db = setup_db().await;

    // Create 5 trees
    let mut ids = Vec::new();
    for i in 0..5 {
        let id = Uuid::now_v7();
        ids.push(id);
        TreeRepo::create(&db, id, format!("Tree {i}"), None)
            .await
            .unwrap();
    }

    // List first 3
    let params = PaginationParams {
        first: 3,
        after: None,
    };
    let conn = TreeRepo::list(&db, &params).await.unwrap();
    assert_eq!(conn.edges.len(), 3);
    assert_eq!(conn.total_count, 5);
    assert!(conn.page_info.has_next_page);

    // List next page using end_cursor
    let params2 = PaginationParams {
        first: 3,
        after: conn.page_info.end_cursor.clone(),
    };
    let conn2 = TreeRepo::list(&db, &params2).await.unwrap();
    assert_eq!(conn2.edges.len(), 2);
    assert!(!conn2.page_info.has_next_page);
    assert_eq!(conn2.total_count, 5);

    // Soft-deleted trees are excluded from list
    TreeRepo::delete(&db, ids[0]).await.unwrap();
    let params3 = PaginationParams {
        first: 100,
        after: None,
    };
    let conn3 = TreeRepo::list(&db, &params3).await.unwrap();
    assert_eq!(conn3.total_count, 4);
    assert_eq!(conn3.edges.len(), 4);
}

// ───────────────────────── Person tests ─────────────────────────

#[tokio::test]
async fn person_crud() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let id = Uuid::now_v7();

    // Create
    let person = PersonRepo::create(&db, id, tree_id, Sex::Female)
        .await
        .unwrap();
    assert_eq!(person.id, id);
    assert_eq!(person.tree_id, tree_id);
    assert_eq!(person.sex, Sex::Female);

    // Get
    let fetched = PersonRepo::get(&db, id).await.unwrap();
    assert_eq!(fetched.sex, Sex::Female);

    // Update sex
    let updated = PersonRepo::update(&db, id, Some(Sex::Male)).await.unwrap();
    assert_eq!(updated.sex, Sex::Male);

    // Soft-delete
    PersonRepo::delete(&db, id).await.unwrap();
    let err = PersonRepo::get(&db, id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

#[tokio::test]
async fn person_list_tree_scoped() {
    let db = setup_db().await;
    let tree_a = create_tree(&db).await;
    let tree_b = create_tree(&db).await;

    // Create 2 persons in tree_a, 1 in tree_b
    create_person(&db, tree_a).await;
    create_person(&db, tree_a).await;
    create_person(&db, tree_b).await;

    let params = PaginationParams::default();
    let conn_a = PersonRepo::list(&db, tree_a, &params).await.unwrap();
    assert_eq!(conn_a.total_count, 2);

    let conn_b = PersonRepo::list(&db, tree_b, &params).await.unwrap();
    assert_eq!(conn_b.total_count, 1);
}

// ───────────────────────── PersonName tests ─────────────────────────

#[tokio::test]
async fn person_name_crud() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;
    let id = Uuid::now_v7();

    // Create
    let name = PersonNameRepo::create(
        &db,
        id,
        person_id,
        NameType::Birth,
        Some("Jean".into()),
        Some("Dupont".into()),
        None,
        None,
        None,
        true,
    )
    .await
    .unwrap();
    assert_eq!(name.given_names.as_deref(), Some("Jean"));
    assert!(name.is_primary);

    // Get
    let fetched = PersonNameRepo::get(&db, id).await.unwrap();
    assert_eq!(fetched.surname.as_deref(), Some("Dupont"));

    // List by person
    let names = PersonNameRepo::list_by_person(&db, person_id)
        .await
        .unwrap();
    assert_eq!(names.len(), 1);

    // Update
    let updated = PersonNameRepo::update(
        &db,
        id,
        Some(NameType::Married),
        None,
        Some(Some("Martin".into())),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(updated.surname.as_deref(), Some("Martin"));
    assert_eq!(updated.name_type, NameType::Married);

    // Hard-delete
    PersonNameRepo::delete(&db, id).await.unwrap();
    let err = PersonNameRepo::get(&db, id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Family + Spouse + Child tests ─────────────────────────

#[tokio::test]
async fn family_lifecycle() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let family_id = Uuid::now_v7();

    // Create family
    let family = FamilyRepo::create(&db, family_id, tree_id).await.unwrap();
    assert_eq!(family.id, family_id);

    // Create persons for spouse/child
    let husband_id = create_person(&db, tree_id).await;
    let wife_id = create_person(&db, tree_id).await;
    let child_id = create_person(&db, tree_id).await;

    // Add spouses
    let sp1_id = Uuid::now_v7();
    let sp1 = FamilySpouseRepo::create(&db, sp1_id, family_id, husband_id, SpouseRole::Husband, 0)
        .await
        .unwrap();
    assert_eq!(sp1.role, SpouseRole::Husband);

    let sp2_id = Uuid::now_v7();
    FamilySpouseRepo::create(&db, sp2_id, family_id, wife_id, SpouseRole::Wife, 1)
        .await
        .unwrap();

    let spouses = FamilySpouseRepo::list_by_family(&db, family_id)
        .await
        .unwrap();
    assert_eq!(spouses.len(), 2);

    // Add child
    let fc_id = Uuid::now_v7();
    let fc = FamilyChildRepo::create(&db, fc_id, family_id, child_id, ChildType::Biological, 0)
        .await
        .unwrap();
    assert_eq!(fc.child_type, ChildType::Biological);

    let children = FamilyChildRepo::list_by_family(&db, family_id)
        .await
        .unwrap();
    assert_eq!(children.len(), 1);

    // Remove spouse
    FamilySpouseRepo::delete(&db, sp1_id).await.unwrap();
    let spouses2 = FamilySpouseRepo::list_by_family(&db, family_id)
        .await
        .unwrap();
    assert_eq!(spouses2.len(), 1);

    // Remove child
    FamilyChildRepo::delete(&db, fc_id).await.unwrap();
    let children2 = FamilyChildRepo::list_by_family(&db, family_id)
        .await
        .unwrap();
    assert_eq!(children2.len(), 0);

    // Soft-delete family
    FamilyRepo::delete(&db, family_id).await.unwrap();
    let err = FamilyRepo::get(&db, family_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Event tests ─────────────────────────

#[tokio::test]
async fn event_crud_and_filters() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let ev1_id = Uuid::now_v7();
    let ev1 = EventRepo::create(
        &db,
        ev1_id,
        tree_id,
        EventType::Birth,
        Some("1 JAN 1900".into()),
        Some(chrono::NaiveDate::from_ymd_opt(1900, 1, 1).unwrap()),
        None,
        Some(person_id),
        None,
        Some("Born in Paris".into()),
    )
    .await
    .unwrap();
    assert_eq!(ev1.event_type, EventType::Birth);

    let ev2_id = Uuid::now_v7();
    EventRepo::create(
        &db,
        ev2_id,
        tree_id,
        EventType::Death,
        None,
        None,
        None,
        Some(person_id),
        None,
        None,
    )
    .await
    .unwrap();

    // Get
    let fetched = EventRepo::get(&db, ev1_id).await.unwrap();
    assert_eq!(fetched.description.as_deref(), Some("Born in Paris"));

    // List all in tree
    let params = PaginationParams::default();
    let conn = EventRepo::list(&db, tree_id, &EventFilter::default(), &params)
        .await
        .unwrap();
    assert_eq!(conn.total_count, 2);

    // Filter by event_type
    let filter = EventFilter {
        event_type: Some(EventType::Birth),
        ..Default::default()
    };
    let conn2 = EventRepo::list(&db, tree_id, &filter, &params)
        .await
        .unwrap();
    assert_eq!(conn2.total_count, 1);

    // Filter by person_id
    let filter2 = EventFilter {
        person_id: Some(person_id),
        ..Default::default()
    };
    let conn3 = EventRepo::list(&db, tree_id, &filter2, &params)
        .await
        .unwrap();
    assert_eq!(conn3.total_count, 2);

    // Update
    let updated = EventRepo::update(
        &db,
        ev1_id,
        None,
        None,
        None,
        None,
        Some(Some("Updated description".into())),
    )
    .await
    .unwrap();
    assert_eq!(updated.description.as_deref(), Some("Updated description"));

    // Soft-delete
    EventRepo::delete(&db, ev1_id).await.unwrap();
    let err = EventRepo::get(&db, ev1_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Place tests ─────────────────────────

#[tokio::test]
async fn place_crud_and_search() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let p1_id = Uuid::now_v7();
    let place = PlaceRepo::create(
        &db,
        p1_id,
        tree_id,
        "Paris, France".into(),
        Some(48.8566),
        Some(2.3522),
    )
    .await
    .unwrap();
    assert_eq!(place.name, "Paris, France");
    assert_eq!(place.latitude, Some(48.8566));

    let p2_id = Uuid::now_v7();
    PlaceRepo::create(&db, p2_id, tree_id, "Lyon, France".into(), None, None)
        .await
        .unwrap();

    // Get
    let fetched = PlaceRepo::get(&db, p1_id).await.unwrap();
    assert_eq!(fetched.name, "Paris, France");

    // List all
    let params = PaginationParams::default();
    let conn = PlaceRepo::list(&db, tree_id, None, &params).await.unwrap();
    assert_eq!(conn.total_count, 2);

    // Search by name
    let conn2 = PlaceRepo::list(&db, tree_id, Some("Paris"), &params)
        .await
        .unwrap();
    assert_eq!(conn2.total_count, 1);
    assert_eq!(conn2.edges[0].node.name, "Paris, France");

    // Search for "France" matches both
    let conn3 = PlaceRepo::list(&db, tree_id, Some("France"), &params)
        .await
        .unwrap();
    assert_eq!(conn3.total_count, 2);

    // Update
    let updated = PlaceRepo::update(&db, p1_id, Some("Paris".into()), Some(None), None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Paris");
    assert!(updated.latitude.is_none()); // cleared
    assert_eq!(updated.longitude, Some(2.3522)); // unchanged

    // Hard-delete
    PlaceRepo::delete(&db, p1_id).await.unwrap();
    let err = PlaceRepo::get(&db, p1_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Source + Citation tests ─────────────────────────

#[tokio::test]
async fn source_and_citation_lifecycle() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    // Create source
    let src_id = Uuid::now_v7();
    let source = SourceRepo::create(
        &db,
        src_id,
        tree_id,
        "Parish Register".into(),
        Some("Church of Paris".into()),
        None,
        Some("PR".into()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(source.title, "Parish Register");
    assert_eq!(source.abbreviation.as_deref(), Some("PR"));

    // Update source
    let updated = SourceRepo::update(
        &db,
        src_id,
        Some("Updated Title".into()),
        None,
        Some(Some("Publisher X".into())),
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.publisher.as_deref(), Some("Publisher X"));
    assert_eq!(updated.author.as_deref(), Some("Church of Paris")); // unchanged

    // List sources
    let params = PaginationParams::default();
    let conn = SourceRepo::list(&db, tree_id, &params).await.unwrap();
    assert_eq!(conn.total_count, 1);

    // Create citation
    let cit_id = Uuid::now_v7();
    let person_id = create_person(&db, tree_id).await;
    let citation = CitationRepo::create(
        &db,
        cit_id,
        src_id,
        Some(person_id),
        None,
        None,
        Some("p. 42".into()),
        Confidence::High,
        Some("Baptism recorded".into()),
    )
    .await
    .unwrap();
    assert_eq!(citation.confidence, Confidence::High);
    assert_eq!(citation.page.as_deref(), Some("p. 42"));

    // List citations by source
    let citations = CitationRepo::list_by_source(&db, src_id).await.unwrap();
    assert_eq!(citations.len(), 1);

    // Update citation
    let updated_cit = CitationRepo::update(
        &db,
        cit_id,
        Some(Some("p. 43".into())),
        Some(Confidence::VeryHigh),
        None,
    )
    .await
    .unwrap();
    assert_eq!(updated_cit.page.as_deref(), Some("p. 43"));
    assert_eq!(updated_cit.confidence, Confidence::VeryHigh);

    // Hard-delete citation
    CitationRepo::delete(&db, cit_id).await.unwrap();
    let err = CitationRepo::get(&db, cit_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    // Soft-delete source
    SourceRepo::delete(&db, src_id).await.unwrap();
    let err = SourceRepo::get(&db, src_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Media + MediaLink tests ─────────────────────────

#[tokio::test]
async fn media_and_media_link_lifecycle() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    // Create media
    let media_id = Uuid::now_v7();
    let media = MediaRepo::create(
        &db,
        media_id,
        tree_id,
        "photo.jpg".into(),
        "image/jpeg".into(),
        "/uploads/photo.jpg".into(),
        1024,
        Some("Family Photo".into()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(media.file_name, "photo.jpg");
    assert_eq!(media.file_size, 1024);

    // Get
    let fetched = MediaRepo::get(&db, media_id).await.unwrap();
    assert_eq!(fetched.title.as_deref(), Some("Family Photo"));

    // Update
    let updated = MediaRepo::update(&db, media_id, None, Some(Some("A family gathering".into())))
        .await
        .unwrap();
    assert_eq!(updated.description.as_deref(), Some("A family gathering"));

    // Create media link
    let link_id = Uuid::now_v7();
    let link = MediaLinkRepo::create(&db, link_id, media_id, Some(person_id), None, None, None, 0)
        .await
        .unwrap();
    assert_eq!(link.media_id, media_id);
    assert_eq!(link.person_id, Some(person_id));

    // List by media
    let links = MediaLinkRepo::list_by_media(&db, media_id).await.unwrap();
    assert_eq!(links.len(), 1);

    // Delete link
    MediaLinkRepo::delete(&db, link_id).await.unwrap();
    let links2 = MediaLinkRepo::list_by_media(&db, media_id).await.unwrap();
    assert_eq!(links2.len(), 0);

    // List media in tree
    let params = PaginationParams::default();
    let conn = MediaRepo::list(&db, tree_id, &params).await.unwrap();
    assert_eq!(conn.total_count, 1);

    // Soft-delete media
    MediaRepo::delete(&db, media_id).await.unwrap();
    let err = MediaRepo::get(&db, media_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── Note tests ─────────────────────────

#[tokio::test]
async fn note_crud() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let note_id = Uuid::now_v7();
    let note = NoteRepo::create(
        &db,
        note_id,
        tree_id,
        "Some important note".into(),
        Some(person_id),
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(note.text, "Some important note");
    assert_eq!(note.person_id, Some(person_id));

    // Get
    let fetched = NoteRepo::get(&db, note_id).await.unwrap();
    assert_eq!(fetched.text, "Some important note");

    // List by entity (person)
    let notes = NoteRepo::list_by_entity(&db, tree_id, Some(person_id), None, None, None)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);

    // List by entity (no filter = all in tree)
    let notes_all = NoteRepo::list_by_entity(&db, tree_id, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(notes_all.len(), 1);

    // Update text
    let updated = NoteRepo::update(&db, note_id, Some("Updated note".into()))
        .await
        .unwrap();
    assert_eq!(updated.text, "Updated note");

    // Soft-delete
    NoteRepo::delete(&db, note_id).await.unwrap();
    let err = NoteRepo::get(&db, note_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

// ───────────────────────── PersonAncestry tests ─────────────────────────

#[tokio::test]
async fn person_ancestry_queries() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let grandparent_id = create_person(&db, tree_id).await;
    let parent_id = create_person(&db, tree_id).await;
    let child_id = create_person(&db, tree_id).await;

    // Self-references (depth 0)
    PersonAncestryRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        grandparent_id,
        grandparent_id,
        0,
    )
    .await
    .unwrap();
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, parent_id, parent_id, 0)
        .await
        .unwrap();
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, child_id, child_id, 0)
        .await
        .unwrap();

    // grandparent -> parent (depth 1)
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, grandparent_id, parent_id, 1)
        .await
        .unwrap();

    // grandparent -> child (depth 2)
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, grandparent_id, child_id, 2)
        .await
        .unwrap();

    // parent -> child (depth 1)
    PersonAncestryRepo::create(&db, Uuid::now_v7(), tree_id, parent_id, child_id, 1)
        .await
        .unwrap();

    // Ancestors of child (excludes self-reference)
    let ancestors = PersonAncestryRepo::ancestors(&db, child_id, None)
        .await
        .unwrap();
    assert_eq!(ancestors.len(), 2);
    // Ordered by depth: parent (1), grandparent (2)
    assert_eq!(ancestors[0].depth, 1);
    assert_eq!(ancestors[0].ancestor_id, parent_id);
    assert_eq!(ancestors[1].depth, 2);
    assert_eq!(ancestors[1].ancestor_id, grandparent_id);

    // Ancestors with max_depth=1
    let ancestors_limited = PersonAncestryRepo::ancestors(&db, child_id, Some(1))
        .await
        .unwrap();
    assert_eq!(ancestors_limited.len(), 1);
    assert_eq!(ancestors_limited[0].ancestor_id, parent_id);

    // Descendants of grandparent (excludes self-reference)
    let descendants = PersonAncestryRepo::descendants(&db, grandparent_id, None)
        .await
        .unwrap();
    assert_eq!(descendants.len(), 2);
    assert_eq!(descendants[0].depth, 1);
    assert_eq!(descendants[0].descendant_id, parent_id);
    assert_eq!(descendants[1].depth, 2);
    assert_eq!(descendants[1].descendant_id, child_id);

    // Delete ancestry for child (re-parenting scenario)
    let deleted = PersonAncestryRepo::delete_by_descendant(&db, child_id)
        .await
        .unwrap();
    assert_eq!(deleted, 3); // self-ref + parent + grandparent

    let ancestors_after = PersonAncestryRepo::ancestors(&db, child_id, None)
        .await
        .unwrap();
    assert_eq!(ancestors_after.len(), 0);
}

// ───────────────────────── Pagination edge cases ─────────────────────────

#[tokio::test]
async fn pagination_empty_result() {
    let db = setup_db().await;
    let params = PaginationParams::default();
    let conn = TreeRepo::list(&db, &params).await.unwrap();
    assert_eq!(conn.edges.len(), 0);
    assert_eq!(conn.total_count, 0);
    assert!(!conn.page_info.has_next_page);
    assert!(conn.page_info.end_cursor.is_none());
}

#[tokio::test]
async fn pagination_invalid_cursor() {
    let db = setup_db().await;
    let params = PaginationParams {
        first: 10,
        after: Some("not-a-uuid".into()),
    };
    let err = TreeRepo::list(&db, &params).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::Validation(_)));
}

#[tokio::test]
async fn pagination_clamps_page_size() {
    let db = setup_db().await;

    // Create 3 trees
    for _ in 0..3 {
        TreeRepo::create(&db, Uuid::now_v7(), "T".into(), None)
            .await
            .unwrap();
    }

    // first=0 should be clamped to 1
    let params = PaginationParams {
        first: 0,
        after: None,
    };
    let conn = TreeRepo::list(&db, &params).await.unwrap();
    assert_eq!(conn.edges.len(), 1);
    assert!(conn.page_info.has_next_page);

    // first=200 should be clamped to MAX_PAGE_SIZE (100)
    let params2 = PaginationParams {
        first: 200,
        after: None,
    };
    let conn2 = TreeRepo::list(&db, &params2).await.unwrap();
    assert_eq!(conn2.edges.len(), 3); // only 3 exist
}

// ───────────────────────── Delete non-existent returns NotFound ─────────────────────────

#[tokio::test]
async fn delete_nonexistent_returns_not_found() {
    let db = setup_db().await;
    let fake = Uuid::now_v7();

    let err = TreeRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = PersonRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = FamilyRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = PersonNameRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = FamilySpouseRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = FamilyChildRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = EventRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = PlaceRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = SourceRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = CitationRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = MediaRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = MediaLinkRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = NoteRepo::delete(&db, fake).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}
