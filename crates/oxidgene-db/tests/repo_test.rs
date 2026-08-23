//! Integration tests for the repository layer.
//!
//! All tests run against an in-memory SQLite database.

use oxidgene_core::enums::{
    Calendar, ChildType, Confidence, DateQualifier, EventType, NameType, Sex, SpouseRole,
};
use oxidgene_core::error::OxidGeneError;
use oxidgene_db::repo::{
    AncestryRepo, CitationRepo, DictionaryRepo, EventFilter, EventRepo, FamilyChildRepo,
    FamilyRepo, FamilySpouseRepo, MediaLinkRepo, MediaPatch, MediaRepo, NoteRepo, PaginationParams,
    PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo, PersonRepo, PlaceRepo, SourceRepo,
    TreeRepo, connect, run_migrations,
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
    let updated = TreeRepo::update(&db, id, Some("Renamed".into()), None, None, None, None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.description.as_deref(), Some("desc")); // unchanged

    // Update description to None
    let updated2 = TreeRepo::update(&db, id, None, Some(None), None, None, None)
        .await
        .unwrap();
    assert!(updated2.description.is_none());

    // Delete — the flag alone is enough to hide the tree
    TreeRepo::soft_delete(&db, id).await.unwrap();

    // Get after delete returns NotFound
    let err = TreeRepo::get(&db, id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

/// A soft-deleted tree stays queued until it is actually purged, which is what
/// lets an interrupted purge resume after a restart.
#[tokio::test]
async fn soft_deleted_tree_stays_purgeable_until_purged() {
    let db = setup_db().await;
    let kept = create_tree(&db).await;
    let doomed = create_tree(&db).await;

    assert!(TreeRepo::list_purgeable(&db).await.unwrap().is_empty());

    TreeRepo::soft_delete(&db, doomed).await.unwrap();

    // Still queued — this is what the startup sweep picks up.
    assert_eq!(
        TreeRepo::list_purgeable(&db).await.unwrap(),
        vec![doomed],
        "a soft-deleted tree must remain purgeable until purged"
    );

    // Soft-deleting twice is a NotFound, so a double delete cannot enqueue twice.
    let err = TreeRepo::soft_delete(&db, doomed).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    TreeRepo::purge(&db, doomed).await.unwrap();
    assert!(TreeRepo::list_purgeable(&db).await.unwrap().is_empty());

    // Re-purging is a no-op, so resuming after a crash is safe.
    TreeRepo::purge(&db, doomed).await.unwrap();

    // The untouched tree is unaffected throughout.
    assert_eq!(TreeRepo::get(&db, kept).await.unwrap().id, kept);
}

#[tokio::test]
async fn tree_delete_cascades_to_children() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let event_id = Uuid::now_v7();
    EventRepo::create(
        &db,
        event_id,
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(person_id),
        None,
        Some("Cultivateur".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .expect("create occupation event");

    // The cascade only runs on the hard delete, not on the flag.
    TreeRepo::purge(&db, tree_id).await.unwrap();

    let err = PersonRepo::get(&db, person_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    let err = EventRepo::get(&db, event_id).await.unwrap_err();
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

    // Deleted trees are excluded from list
    TreeRepo::soft_delete(&db, ids[0]).await.unwrap();
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
    let updated = PersonRepo::update(&db, id, Some(Sex::Male), None)
        .await
        .unwrap();
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
        PersonNamePieces {
            given_names: Some("Jean".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
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
        PersonNamePiecesPatch {
            surname: Some(Some("Martin".into())),
            ..Default::default()
        },
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
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
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
        DateQualifier::default(),
        None,
        Calendar::default(),
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
        None,
        None,
        None,
        None,
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
        None,
        Some(Some("p. 43".into())),
        Some(Confidence::VeryHigh),
        None,
    )
    .await
    .unwrap();
    assert_eq!(updated_cit.page.as_deref(), Some("p. 43"));
    assert_eq!(updated_cit.confidence, Confidence::VeryHigh);
    assert_eq!(
        updated_cit.source_id, src_id,
        "source left alone when omitted"
    );

    // Repoint the citation at another source — the same statement about the
    // same fact, so it is edited rather than deleted and recreated.
    let other_src_id = Uuid::now_v7();
    SourceRepo::create(
        &db,
        other_src_id,
        tree_id,
        "Civil Register".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    let repointed = CitationRepo::update(&db, cit_id, Some(other_src_id), None, None, None)
        .await
        .unwrap();
    assert_eq!(repointed.id, cit_id, "same citation row");
    assert_eq!(repointed.source_id, other_src_id);
    assert_eq!(
        repointed.page.as_deref(),
        Some("p. 43"),
        "other fields kept"
    );
    assert!(
        CitationRepo::list_by_source(&db, src_id)
            .await
            .unwrap()
            .is_empty(),
        "no longer attached to the old source"
    );
    CitationRepo::update(&db, cit_id, Some(src_id), None, None, None)
        .await
        .unwrap();

    // Hard-delete citation
    CitationRepo::delete(&db, cit_id).await.unwrap();
    let err = CitationRepo::get(&db, cit_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    // Soft-delete source
    SourceRepo::delete(&db, src_id).await.unwrap();
    let err = SourceRepo::get(&db, src_id).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));
}

/// Free-text source entry mints a `Source` per distinct title, so a corrected
/// typo leaves its row behind. Collecting those must never take out a source
/// something still points at.
#[tokio::test]
async fn source_is_only_collected_once_nothing_points_at_it() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let new_source = async |title: &str| {
        let id = Uuid::now_v7();
        SourceRepo::create(&db, id, tree_id, title.into(), None, None, None, None)
            .await
            .unwrap();
        id
    };

    // Referenced by a citation — the required link.
    let cited = new_source("Parish Register").await;
    let cit_id = Uuid::now_v7();
    CitationRepo::create(
        &db,
        cit_id,
        cited,
        Some(person_id),
        None,
        None,
        None,
        Confidence::High,
        None,
    )
    .await
    .unwrap();
    assert!(
        !SourceRepo::delete_if_unused(&db, cited).await.unwrap(),
        "a cited source must be kept"
    );
    assert_eq!(SourceRepo::get(&db, cited).await.unwrap().id, cited);

    // Referenced by a note — an optional link, and just as binding.
    let noted = new_source("Census 1901").await;
    let note_id = Uuid::now_v7();
    NoteRepo::create(
        &db,
        note_id,
        tree_id,
        "transcription".into(),
        None,
        None,
        None,
        Some(noted),
        None,
    )
    .await
    .unwrap();
    assert!(
        !SourceRepo::delete_if_unused(&db, noted).await.unwrap(),
        "a source a note names must be kept"
    );

    // Nothing points at this one: this is the typo case.
    let orphan = new_source("Parrish Registre").await;
    assert!(SourceRepo::delete_if_unused(&db, orphan).await.unwrap());
    let err = SourceRepo::get(&db, orphan).await.unwrap_err();
    assert!(matches!(err, OxidGeneError::NotFound { .. }));

    // Already gone: the caller asked for it to be absent, and it is.
    assert!(
        !SourceRepo::delete_if_unused(&db, orphan).await.unwrap(),
        "collecting twice is a no-op, not an error"
    );

    // Dropping the last citation releases the source it was holding.
    CitationRepo::delete(&db, cit_id).await.unwrap();
    assert!(SourceRepo::delete_if_unused(&db, cited).await.unwrap());
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
    let updated = MediaRepo::update(
        &db,
        media_id,
        MediaPatch {
            description: Some(Some("A family gathering".into())),
            ..Default::default()
        },
    )
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
    let notes = NoteRepo::list_by_entity(&db, tree_id, Some(person_id), None, None, None, None)
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);

    // List by entity (no filter = all in tree)
    let notes_all = NoteRepo::list_by_entity(&db, tree_id, None, None, None, None, None)
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

// ───────────────────────── Ancestry traversal tests ─────────────────────────

/// Helper: create a family linking `parents` to `child`.
async fn link_parents(db: &DatabaseConnection, tree_id: Uuid, parents: &[Uuid], child: Uuid) {
    let family_id = Uuid::now_v7();
    FamilyRepo::create(db, family_id, tree_id)
        .await
        .expect("create family");
    for (i, &parent) in parents.iter().enumerate() {
        FamilySpouseRepo::create(
            db,
            Uuid::now_v7(),
            family_id,
            parent,
            SpouseRole::Husband,
            i as i32,
        )
        .await
        .expect("add spouse");
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
    .expect("add child");
}

#[tokio::test]
async fn ancestry_walks_the_family_links() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let grandparent = create_person(&db, tree_id).await;
    let parent = create_person(&db, tree_id).await;
    let child = create_person(&db, tree_id).await;

    link_parents(&db, tree_id, &[grandparent], parent).await;
    link_parents(&db, tree_id, &[parent], child).await;

    // Ancestors, ordered by depth and never including the person themself.
    let ancestors = AncestryRepo::ancestors(&db, child, None).await.unwrap();
    assert_eq!(ancestors.len(), 2);
    assert_eq!(ancestors[0].person_id, parent);
    assert_eq!(ancestors[0].depth, 1);
    assert_eq!(ancestors[1].person_id, grandparent);
    assert_eq!(ancestors[1].depth, 2);

    // max_depth stops the walk.
    let limited = AncestryRepo::ancestors(&db, child, Some(1)).await.unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].person_id, parent);

    // Descendants are the mirror image.
    let descendants = AncestryRepo::descendants(&db, grandparent, None)
        .await
        .unwrap();
    assert_eq!(descendants.len(), 2);
    assert_eq!(descendants[0].person_id, parent);
    assert_eq!(descendants[0].depth, 1);
    assert_eq!(descendants[1].person_id, child);
    assert_eq!(descendants[1].depth, 2);

    // A person with no links either way gets empty results, not an error.
    let orphan = create_person(&db, tree_id).await;
    assert!(
        AncestryRepo::ancestors(&db, orphan, None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        AncestryRepo::descendants(&db, orphan, None)
            .await
            .unwrap()
            .is_empty()
    );
}

/// Both parents of a couple are ancestors at the same depth.
#[tokio::test]
async fn ancestry_reports_both_parents() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let parent_a = create_person(&db, tree_id).await;
    let parent_b = create_person(&db, tree_id).await;
    let child = create_person(&db, tree_id).await;
    link_parents(&db, tree_id, &[parent_a, parent_b], child).await;

    let ancestors = AncestryRepo::ancestors(&db, child, None).await.unwrap();
    assert_eq!(ancestors.len(), 2);
    assert!(ancestors.iter().all(|a| a.depth == 1));
    let mut found: Vec<Uuid> = ancestors.iter().map(|a| a.person_id).collect();
    found.sort();
    let mut expected = vec![parent_a, parent_b];
    expected.sort();
    assert_eq!(found, expected);
}

/// Pedigree implex: when an ancestor is reachable by two paths of different
/// lengths, they appear once, at the shorter distance. The closure table this
/// replaced could only ever store one arbitrary depth per pair.
#[tokio::test]
async fn ancestry_reports_shortest_depth_on_implex() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    // shared is both the parent of branch_a and the grandparent of root,
    // so root reaches them at depth 2 (via branch_a) and depth 1 (directly).
    let shared = create_person(&db, tree_id).await;
    let branch_a = create_person(&db, tree_id).await;
    let root = create_person(&db, tree_id).await;

    link_parents(&db, tree_id, &[shared], branch_a).await;
    link_parents(&db, tree_id, &[branch_a, shared], root).await;

    let ancestors = AncestryRepo::ancestors(&db, root, None).await.unwrap();
    assert_eq!(ancestors.len(), 2, "each ancestor is reported once");
    let shared_link = ancestors
        .iter()
        .find(|a| a.person_id == shared)
        .expect("shared ancestor present");
    assert_eq!(shared_link.depth, 1, "the shortest path wins");
}

/// A cycle in the family links must not hang the walk. The schema does not
/// prevent one, and corrupt imports produce them.
#[tokio::test]
async fn ancestry_terminates_on_a_cycle() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let a = create_person(&db, tree_id).await;
    let b = create_person(&db, tree_id).await;
    link_parents(&db, tree_id, &[a], b).await;
    link_parents(&db, tree_id, &[b], a).await; // closes the loop

    // Bounded by MAX_GENERATIONS rather than recursing forever.
    let ancestors = AncestryRepo::ancestors(&db, a, None).await.unwrap();
    assert_eq!(ancestors.len(), 2, "both persons reachable, each once");
    let descendants = AncestryRepo::descendants(&db, a, None).await.unwrap();
    assert_eq!(descendants.len(), 2);
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

    let err = TreeRepo::soft_delete(&db, fake).await.unwrap_err();
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

// ───────────────────────── Dictionary aggregation tests ─────────────────────────

#[tokio::test]
async fn dictionary_family_names_groups_by_person_not_by_row() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    let p1 = create_person(&db, tree_id).await;
    let p2 = create_person(&db, tree_id).await;
    let p3 = create_person(&db, tree_id).await;

    // p1 has two names sharing the same surname (birth + nickname) — must
    // count as one person, not two rows.
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p1,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Jean".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p1,
        NameType::AlsoKnownAs,
        PersonNamePieces {
            given_names: Some("Jeannot".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        false,
        0,
    )
    .await
    .unwrap();

    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p2,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Marie".into()),
            surname: Some("Dupont".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();

    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p3,
        NameType::Birth,
        PersonNamePieces {
            given_names: Some("Paul".into()),
            surname: Some("Martin".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();

    // Soft-deleted person must be excluded entirely.
    let p4 = create_person(&db, tree_id).await;
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p4,
        NameType::Birth,
        PersonNamePieces {
            given_names: None,
            surname: Some("Ghost".into()),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .unwrap();
    PersonRepo::delete(&db, p4).await.unwrap();

    // Blank surname must be excluded.
    PersonNameRepo::create(
        &db,
        Uuid::now_v7(),
        p3,
        NameType::AlsoKnownAs,
        PersonNamePieces {
            given_names: Some("X".into()),
            surname: Some("   ".into()),
            ..Default::default()
        },
        false,
        0,
    )
    .await
    .unwrap();

    let entries = DictionaryRepo::family_names(&db, tree_id).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].value, "Dupont");
    assert_eq!(entries[0].count, 2); // p1 (2 rows) + p2 — not 3
    assert_eq!(entries[1].value, "Martin");
    assert_eq!(entries[1].count, 1);
}

/// Helper: give `person_id` a birth name already split into particle + root,
/// as an import would have stored it.
async fn create_split_name(
    db: &DatabaseConnection,
    person_id: Uuid,
    surname_prefix: Option<&str>,
    surname: &str,
) {
    PersonNameRepo::create(
        db,
        Uuid::now_v7(),
        person_id,
        NameType::Birth,
        PersonNamePieces {
            surname: Some(surname.into()),
            surname_prefix: surname_prefix.map(Into::into),
            ..Default::default()
        },
        true,
        0,
    )
    .await
    .expect("create name");
}

#[tokio::test]
async fn dictionary_set_family_name_particle_recuts_every_occurrence() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    // Three persons an import filed under a particle they do not have, plus a
    // fourth carrying a genuine one that must be left alone.
    for _ in 0..3 {
        let p = create_person(&db, tree_id).await;
        create_split_name(&db, p, Some("LE"), "BRANCH").await;
    }
    let untouched = create_person(&db, tree_id).await;
    create_split_name(&db, untouched, Some("de la"), "Cruz").await;

    let out = DictionaryRepo::set_family_name_particle(&db, tree_id, "LE BRANCH", "")
        .await
        .unwrap();
    assert_eq!(out.names_updated, 3);
    assert_eq!(out.persons_updated, 3);
    assert_eq!(out.surname_prefix, None);
    assert_eq!(out.surname, "LE BRANCH");

    // The dictionary still lists the same text — only the filing key moved,
    // from the root "branch" to the whole surname.
    let entries = DictionaryRepo::family_names(&db, tree_id).await.unwrap();
    let branch = entries.iter().find(|e| e.value == "LE BRANCH").unwrap();
    assert_eq!(branch.count, 3);
    assert_eq!(branch.sort_key, "le branch");

    // The genuine particle next door is untouched.
    let cruz = entries.iter().find(|e| e.value == "de la Cruz").unwrap();
    assert_eq!(cruz.sort_key, "cruz");

    // Drill-down still resolves the re-cut name to its people.
    let ids = DictionaryRepo::family_name_usage_person_ids(&db, tree_id, "LE BRANCH")
        .await
        .unwrap();
    assert_eq!(ids.len(), 3);
}

#[tokio::test]
async fn dictionary_set_family_name_particle_is_idempotent_and_can_narrow() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let p = create_person(&db, tree_id).await;
    create_split_name(&db, p, Some("de la"), "Cruz").await;

    // Narrowing a particle that went too far.
    let out = DictionaryRepo::set_family_name_particle(&db, tree_id, "de la Cruz", "de")
        .await
        .unwrap();
    assert_eq!(out.names_updated, 1);
    assert_eq!(out.surname_prefix.as_deref(), Some("de"));
    assert_eq!(out.surname, "la Cruz");
    // The name still displays — and so is still addressable — as it was.
    assert_eq!(out.value, "de la Cruz");

    // Applying the same cut again rewrites nothing.
    let out = DictionaryRepo::set_family_name_particle(&db, tree_id, "de la Cruz", "de")
        .await
        .unwrap();
    assert_eq!(out.names_updated, 0);
    assert_eq!(out.persons_updated, 0);
}

#[tokio::test]
async fn dictionary_set_family_name_particle_refuses_a_particle_that_is_not_there() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let p = create_person(&db, tree_id).await;
    create_split_name(&db, p, None, "Dupont").await;

    // Accepting this would prepend a word the tree never contained, and
    // clearing the particle afterwards could not take it back out.
    let err = DictionaryRepo::set_family_name_particle(&db, tree_id, "Dupont", "de")
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");

    // A particle that would swallow the whole surname is refused too.
    let err = DictionaryRepo::set_family_name_particle(&db, tree_id, "Dupont", "Dupont")
        .await
        .unwrap_err();
    assert!(matches!(err, OxidGeneError::Validation(_)), "got {err:?}");

    let entries = DictionaryRepo::family_names(&db, tree_id).await.unwrap();
    assert_eq!(entries[0].value, "Dupont");
    assert_eq!(entries[0].sort_key, "dupont");
}

#[tokio::test]
async fn dictionary_occupations_groups_by_person_and_ignores_other_event_types() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let p1 = create_person(&db, tree_id).await;
    let p2 = create_person(&db, tree_id).await;

    // p1 has two "Farmer" occupation events (e.g. recorded at two censuses)
    // — must count as one person.
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(p1),
        None,
        Some("Farmer".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(p1),
        None,
        Some("Farmer".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();

    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(p2),
        None,
        Some("Baker".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();

    // Non-occupation event with a description must be ignored.
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Birth,
        None,
        None,
        None,
        Some(p2),
        None,
        Some("Farmer".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();

    // Blank-description occupation event must be ignored.
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(p2),
        None,
        Some("  ".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();

    // Soft-deleted occupation event must be ignored.
    let deleted_ev = Uuid::now_v7();
    EventRepo::create(
        &db,
        deleted_ev,
        tree_id,
        EventType::Occupation,
        None,
        None,
        None,
        Some(p2),
        None,
        Some("Blacksmith".into()),
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();
    EventRepo::delete(&db, deleted_ev).await.unwrap();

    let entries = DictionaryRepo::occupations(&db, tree_id).await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].value, "Baker");
    assert_eq!(entries[0].count, 1);
    assert_eq!(entries[1].value, "Farmer");
    assert_eq!(entries[1].count, 1); // p1 counted once despite 2 events

    let usage = DictionaryRepo::occupation_usage_person_ids(&db, tree_id, "Farmer")
        .await
        .unwrap();
    assert_eq!(usage, vec![p1]);
}

#[tokio::test]
async fn dictionary_sources_with_usage_counts_citations() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let cited_id = Uuid::now_v7();
    SourceRepo::create(
        &db,
        cited_id,
        tree_id,
        "Parish Register".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let uncited_id = Uuid::now_v7();
    SourceRepo::create(
        &db,
        uncited_id,
        tree_id,
        "Census".into(),
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    CitationRepo::create(
        &db,
        Uuid::now_v7(),
        cited_id,
        Some(person_id),
        None,
        None,
        None,
        Confidence::High,
        None,
    )
    .await
    .unwrap();

    let event_id = Uuid::now_v7();
    EventRepo::create(
        &db,
        event_id,
        tree_id,
        EventType::Birth,
        None,
        None,
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
    .unwrap();
    CitationRepo::create(
        &db,
        Uuid::now_v7(),
        cited_id,
        None,
        Some(event_id),
        None,
        None,
        Confidence::Medium,
        None,
    )
    .await
    .unwrap();

    let entries = DictionaryRepo::sources_with_usage(&db, tree_id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    let (census, cens_count) = entries.iter().find(|(s, _)| s.id == uncited_id).unwrap();
    assert_eq!(census.title, "Census");
    assert_eq!(*cens_count, 0);
    let (register, reg_count) = entries.iter().find(|(s, _)| s.id == cited_id).unwrap();
    assert_eq!(register.title, "Parish Register");
    assert_eq!(*reg_count, 2);

    // Usage drill-down resolves both the direct-person citation and the
    // event-linked citation back to the same person, deduplicated.
    let usage = DictionaryRepo::source_usage_person_ids(&db, cited_id)
        .await
        .unwrap();
    assert_eq!(usage, vec![person_id]);
}

#[tokio::test]
async fn dictionary_source_group_counts_drives_smart_drill_down() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    for title in [
        "AD44 - Actes d'\u{00e9}tat civil",
        "AD44 - Cadastre",
        "AD41 - Registres paroissiaux",
        "AN - Archives Nationales",
        "Biblioth\u{00e8}que municipale",
    ] {
        SourceRepo::create(
            &db,
            Uuid::now_v7(),
            tree_id,
            title.into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    }

    // Top level: only the single-character letters that actually occur
    // ("A" covers AD44/AD44/AD41/AN, "B" covers the library entry) — never
    // the unused 24 other letters.
    let top = DictionaryRepo::source_group_counts(&db, tree_id, "")
        .await
        .unwrap();
    let top: std::collections::HashMap<String, i64> = top.into_iter().collect();
    assert_eq!(top.get("A").copied(), Some(4));
    assert_eq!(top.get("B").copied(), Some(1));
    assert_eq!(top.len(), 2);

    // Drilling into "A" splits further by the next character.
    let under_a = DictionaryRepo::source_group_counts(&db, tree_id, "A")
        .await
        .unwrap();
    let under_a: std::collections::HashMap<String, i64> = under_a.into_iter().collect();
    assert_eq!(under_a.get("AD").copied(), Some(3));
    assert_eq!(under_a.get("AN").copied(), Some(1));
    assert_eq!(under_a.len(), 2);

    // Drilling into "AD" (AD4 is shared by all three "AD..." titles, so the
    // group is 3 chars here).
    let under_ad = DictionaryRepo::source_group_counts(&db, tree_id, "AD")
        .await
        .unwrap();
    let under_ad: std::collections::HashMap<String, i64> = under_ad.into_iter().collect();
    assert_eq!(under_ad.get("AD4").copied(), Some(3));
    assert_eq!(under_ad.len(), 1);

    let under_ad4 = DictionaryRepo::source_group_counts(&db, tree_id, "AD4")
        .await
        .unwrap();
    let under_ad4: std::collections::HashMap<String, i64> = under_ad4.into_iter().collect();
    assert_eq!(under_ad4.get("AD44").copied(), Some(2));
    assert_eq!(under_ad4.get("AD41").copied(), Some(1));

    // The final flat-list step: filtering by a fully-drilled prefix returns
    // exactly the matching sources, case-insensitively.
    let ad44_sources = DictionaryRepo::sources_with_usage_by_prefix(&db, tree_id, "ad44")
        .await
        .unwrap();
    assert_eq!(ad44_sources.len(), 2);
    assert!(
        ad44_sources
            .iter()
            .all(|(s, _)| s.title.to_uppercase().starts_with("AD44"))
    );

    // Empty prefix returns every source, same as `sources_with_usage`.
    let all = DictionaryRepo::sources_with_usage_by_prefix(&db, tree_id, "")
        .await
        .unwrap();
    assert_eq!(all.len(), 5);
}

#[tokio::test]
async fn dictionary_resolve_source_drill_down_skips_forced_single_choice_levels() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;

    // Two fictional towns with one record each ("ALPHA", "BETA"), plus a
    // third ("HOTEL") with six records split across two record types.
    // Every character shared by *all* currently-matching titles is a
    // forced, single-choice step; `resolve_source_drill_down` must skip
    // all of them and only stop where a real (multi-way) choice exists.
    for title in [
        "AD44 - ALPHA - A",
        "AD44 - BETA - A",
        "AD44 - HOTEL - (N) 1",
        "AD44 - HOTEL - (N) 2",
        "AD44 - HOTEL - (N) 3",
        "AD44 - HOTEL - (M) 1",
        "AD44 - HOTEL - (M) 2",
        "AD44 - HOTEL - (M) 3",
    ] {
        SourceRepo::create(
            &db,
            Uuid::now_v7(),
            tree_id,
            title.into(),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    }

    // A tiny threshold (5) keeps the fixture small while exercising the
    // same shape as ui-dictionary.md §8.10's example: a long run of
    // single-choice characters ("AD44" -> " " -> "-" -> " ") is skipped in
    // one resolve, landing directly on the next genuine branch point
    // ("AD44 - ", where town names A/B/H diverge) — not one click per
    // character.
    let (prefix, total, groups) = DictionaryRepo::resolve_source_drill_down(&db, tree_id, "", 5)
        .await
        .unwrap();
    assert_eq!(prefix, "AD44 - ");
    assert_eq!(total, 8);
    let labels: std::collections::HashSet<String> = groups.iter().map(|(l, _)| l.clone()).collect();
    assert_eq!(
        labels,
        std::collections::HashSet::from([
            "AD44 - A".to_string(),
            "AD44 - B".to_string(),
            "AD44 - H".to_string(),
        ])
    );

    // Drilling into the "H" branch (6 "HOTEL" sources, still above the
    // threshold) auto-skips every forced single-choice character in
    // "OTEL - (" and lands directly on the next real branch: record type N
    // vs M — not one click per letter.
    let (prefix, total, groups) =
        DictionaryRepo::resolve_source_drill_down(&db, tree_id, "AD44 - H", 5)
            .await
            .unwrap();
    assert_eq!(prefix, "AD44 - HOTEL - (");
    assert_eq!(total, 6);
    let labels: std::collections::HashSet<String> = groups.iter().map(|(l, _)| l.clone()).collect();
    assert_eq!(
        labels,
        std::collections::HashSet::from([
            "AD44 - HOTEL - (M".to_string(),
            "AD44 - HOTEL - (N".to_string(),
        ])
    );

    // Drilling into a branch whose count is already <= threshold resolves
    // immediately — no further groups, ready for the final flat list.
    let (prefix, total, groups) =
        DictionaryRepo::resolve_source_drill_down(&db, tree_id, "AD44 - A", 5)
            .await
            .unwrap();
    assert_eq!(prefix, "AD44 - A");
    assert_eq!(total, 1);
    assert!(groups.is_empty());
    let final_list = DictionaryRepo::sources_with_usage_by_prefix(&db, tree_id, &prefix)
        .await
        .unwrap();
    assert_eq!(final_list.len(), 1);
    assert_eq!(final_list[0].0.title, "AD44 - ALPHA - A");
}

#[tokio::test]
async fn dictionary_places_with_usage_counts_events() {
    let db = setup_db().await;
    let tree_id = create_tree(&db).await;
    let person_id = create_person(&db, tree_id).await;

    let used_place = Uuid::now_v7();
    PlaceRepo::create(&db, used_place, tree_id, "Paris, France".into(), None, None)
        .await
        .unwrap();
    let unused_place = Uuid::now_v7();
    PlaceRepo::create(
        &db,
        unused_place,
        tree_id,
        "Lyon, France".into(),
        None,
        None,
    )
    .await
    .unwrap();

    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Birth,
        None,
        None,
        Some(used_place),
        Some(person_id),
        None,
        None,
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();
    EventRepo::create(
        &db,
        Uuid::now_v7(),
        tree_id,
        EventType::Death,
        None,
        None,
        Some(used_place),
        Some(person_id),
        None,
        None,
        DateQualifier::default(),
        None,
        Calendar::default(),
        None,
    )
    .await
    .unwrap();

    let entries = DictionaryRepo::places_with_usage(&db, tree_id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    let (paris, paris_count) = entries.iter().find(|(p, _)| p.id == used_place).unwrap();
    assert_eq!(paris.name, "Paris, France");
    assert_eq!(*paris_count, 2);
    let (lyon, lyon_count) = entries.iter().find(|(p, _)| p.id == unused_place).unwrap();
    assert_eq!(lyon.name, "Lyon, France");
    assert_eq!(*lyon_count, 0);

    let usage = DictionaryRepo::place_usage_person_ids(&db, used_place)
        .await
        .unwrap();
    assert_eq!(usage, vec![person_id]);
}
