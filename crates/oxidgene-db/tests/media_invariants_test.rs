//! Document/page and crop invariants enforced independently of API handlers.

use oxidgene_core::types::Portrait;
use oxidgene_core::{OxidGeneError, Sex};
use oxidgene_db::repo::{
    MediaLinkRepo, MediaPatch, MediaRepo, PersonRepo, TreeRepo, UploadedMedia, VignetteInput,
    VignettePatch, VignetteRepo, connect, run_migrations,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

async fn setup() -> (DatabaseConnection, Uuid, Uuid) {
    let db = connect("sqlite::memory:").await.unwrap();
    run_migrations(&db).await.unwrap();
    let tree = Uuid::now_v7();
    TreeRepo::create(&db, tree, "Fictional tree".into(), None)
        .await
        .unwrap();
    let document = Uuid::now_v7();
    MediaRepo::create_document(&db, document, tree, None, chrono::Utc::now())
        .await
        .unwrap();
    (db, tree, document)
}

/// A page we hold *and* have rasterised — the only stored shape a portrait can
/// actually be drawn from.
fn rasterised() -> UploadedMedia {
    UploadedMedia {
        thumbnail_key: Some("test/scan-thumb.jpg".into()),
        ..upload(100)
    }
}

/// A page whose file is somebody else's: a URL, and no bytes of ours.
async fn remote_page(
    db: &DatabaseConnection,
    tree: Uuid,
    document: Uuid,
    url: &str,
) -> oxidgene_core::types::Media {
    MediaRepo::create(
        db,
        Uuid::now_v7(),
        tree,
        Some(document),
        "medium.jpg".into(),
        "image/jpeg".into(),
        url.into(),
        0,
        None,
        None,
    )
    .await
    .unwrap()
}

fn upload(width: i32) -> UploadedMedia {
    UploadedMedia {
        file_name: "scan.png".into(),
        mime_type: "image/png".into(),
        storage_key: "test/scan.png".into(),
        sha256: "test-digest".into(),
        file_size: 1,
        thumbnail_key: None,
        width: Some(width),
        height: Some(100),
        page_count: 1,
        title: None,
        description: None,
        created_at: chrono::Utc::now(),
        metadata: Default::default(),
    }
}

async fn page(
    db: &DatabaseConnection,
    tree: Uuid,
    document: Option<Uuid>,
) -> Result<oxidgene_core::types::Media, OxidGeneError> {
    MediaRepo::create(
        db,
        Uuid::now_v7(),
        tree,
        document,
        "scan.png".into(),
        "image/png".into(),
        "scan.png".into(),
        0,
        None,
        None,
    )
    .await
}

#[tokio::test]
async fn pages_require_a_live_same_tree_document_and_update_its_count() {
    let (db, tree, document) = setup().await;
    let first = page(&db, tree, Some(document)).await.unwrap();
    assert_eq!(MediaRepo::get(&db, document).await.unwrap().page_count, 1);
    let foreign_tree = Uuid::now_v7();
    TreeRepo::create(&db, foreign_tree, "Other fictional tree".into(), None)
        .await
        .unwrap();
    for (tree_id, parent) in [
        (tree, None),
        (tree, Some(first.id)),
        (foreign_tree, Some(document)),
        (tree, Some(Uuid::now_v7())),
    ] {
        assert!(page(&db, tree_id, parent).await.is_err());
        assert!(
            MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree_id, parent, upload(100))
                .await
                .is_err()
        );
    }
    MediaRepo::delete(&db, document).await.unwrap();
    assert!(page(&db, tree, Some(document)).await.is_err());
    assert!(
        MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), upload(100))
            .await
            .is_err()
    );
    assert!(
        MediaRepo::attach_file(&db, first.id, upload(100))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn page_order_requires_a_permutation_and_generic_purge_closes_the_gap() {
    let (db, tree, document) = setup().await;
    let first = page(&db, tree, Some(document)).await.unwrap();
    let second = MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), upload(100))
        .await
        .unwrap();
    assert_eq!(MediaRepo::get(&db, document).await.unwrap().page_count, 2);
    for ids in [
        vec![first.id, first.id],
        vec![first.id],
        vec![first.id, Uuid::now_v7()],
    ] {
        assert!(MediaRepo::reorder_pages(&db, document, &ids).await.is_err());
    }
    assert!(MediaRepo::reorder_pages(&db, first.id, &[]).await.is_err());
    assert!(MediaRepo::refresh_page_count(&db, first.id).await.is_err());
    assert_eq!(MediaRepo::get(&db, second.id).await.unwrap().page_index, 1);
    let reordered = MediaRepo::reorder_pages(&db, document, &[second.id, first.id])
        .await
        .unwrap();
    assert_eq!(reordered[0].id, second.id);
    MediaRepo::purge(&db, second.id).await.unwrap();
    assert_eq!(MediaRepo::get(&db, first.id).await.unwrap().page_index, 0);
    assert_eq!(MediaRepo::get(&db, document).await.unwrap().page_count, 1);
    MediaRepo::delete_page(&db, document, first.id)
        .await
        .unwrap();
    assert_eq!(MediaRepo::get(&db, document).await.unwrap().page_count, 0);
}

#[tokio::test]
async fn file_updates_preserve_shells_stored_types_and_existing_crops() {
    let (db, tree, document) = setup().await;
    assert!(
        MediaRepo::attach_file(&db, document, upload(100))
            .await
            .is_err()
    );
    let page = MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), upload(100))
        .await
        .unwrap();
    for id in [document, page.id] {
        for patch in [
            MediaPatch {
                file_path: Some("other.png".into()),
                ..Default::default()
            },
            MediaPatch {
                mime_type: Some("text/html".into()),
                ..Default::default()
            },
        ] {
            assert!(MediaRepo::update(&db, id, patch).await.is_err());
        }
        MediaRepo::update(
            &db,
            id,
            MediaPatch {
                title: Some(Some("Fictional scan".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    let crop = VignetteRepo::create(
        &db,
        Uuid::now_v7(),
        VignetteInput {
            media_id: page.id,
            x: 50,
            y: 0,
            width: 50,
            height: 100,
            person_id: None,
            event_id: None,
        },
    )
    .await
    .unwrap();
    assert!(
        MediaRepo::attach_file(&db, page.id, upload(80))
            .await
            .is_err()
    );
    assert_eq!(MediaRepo::get(&db, page.id).await.unwrap().width, Some(100));
    MediaRepo::attach_file(&db, page.id, upload(200))
        .await
        .unwrap();
    assert_eq!(VignetteRepo::get(&db, crop.id).await.unwrap(), crop);
}

#[tokio::test]
async fn vignette_writes_validate_pages_bounds_and_attribution() {
    let (db, tree, document) = setup().await;
    let page = MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), upload(100))
        .await
        .unwrap();
    let input = VignetteInput {
        media_id: page.id,
        x: 0,
        y: 0,
        width: 100,
        height: 100,
        person_id: None,
        event_id: None,
    };
    assert!(
        VignetteRepo::create(
            &db,
            Uuid::now_v7(),
            VignetteInput {
                media_id: document,
                ..input.clone()
            }
        )
        .await
        .is_err()
    );
    let crop = VignetteRepo::create(&db, Uuid::now_v7(), input.clone())
        .await
        .unwrap();
    assert!(
        VignetteRepo::update(
            &db,
            crop.id,
            VignettePatch {
                rect: Some((1, 0, 100, 100)),
                ..Default::default()
            }
        )
        .await
        .is_err()
    );
    let foreign_tree = Uuid::now_v7();
    TreeRepo::create(&db, foreign_tree, "Other fictional tree".into(), None)
        .await
        .unwrap();
    let foreign_person = Uuid::now_v7();
    PersonRepo::create(&db, foreign_person, foreign_tree, Sex::Unknown)
        .await
        .unwrap();
    for id in [foreign_person, Uuid::now_v7()] {
        assert!(
            VignetteRepo::create(
                &db,
                Uuid::now_v7(),
                VignetteInput {
                    person_id: Some(id),
                    ..input.clone()
                }
            )
            .await
            .is_err()
        );
        assert!(
            VignetteRepo::update(
                &db,
                crop.id,
                VignettePatch {
                    person_id: Some(Some(id)),
                    ..Default::default()
                }
            )
            .await
            .is_err()
        );
    }
    assert_eq!(VignetteRepo::get(&db, crop.id).await.unwrap(), crop);
    MediaRepo::delete(&db, page.id).await.unwrap();
    assert!(
        VignetteRepo::update(&db, crop.id, VignettePatch::default())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_page_we_do_not_hold_learns_its_size_and_a_page_we_hold_does_not() {
    // Nothing here ever opened a remote file, so the browser that displayed it
    // is the only witness to how big it is — and without that, a region of it
    // cannot be drawn at the right scale. For our own copy the size came out
    // of the bytes, and a second answer could only disagree with the first.
    let (db, tree, document) = setup().await;
    let remote = remote_page(
        &db,
        tree,
        document,
        "https://archives.example.invalid/42.jpg",
    )
    .await;
    let ours = MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), upload(100))
        .await
        .unwrap();

    let sized = MediaRepo::update(
        &db,
        remote.id,
        MediaPatch {
            dimensions: Some((1600, 1200)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!((sized.width, sized.height), (Some(1600), Some(1200)));

    for (id, dimensions) in [
        (ours.id, (1600, 1200)),
        (remote.id, (0, 1200)),
        (remote.id, (1600, -1)),
        (document, (1600, 1200)),
    ] {
        assert!(
            MediaRepo::update(
                &db,
                id,
                MediaPatch {
                    dimensions: Some(dimensions),
                    ..Default::default()
                },
            )
            .await
            .is_err()
        );
    }
}

#[tokio::test]
async fn learning_a_size_re_checks_the_crops_drawn_before_it_was_known() {
    // A region drawn on a page of unknown size was accepted without bounds —
    // there were none to check against. The moment the size arrives is the
    // first moment they can be checked, and a size that contradicts a region
    // already on the page is the wrong size.
    let (db, tree, document) = setup().await;
    let page = remote_page(
        &db,
        tree,
        document,
        "https://archives.example.invalid/42.jpg",
    )
    .await;
    VignetteRepo::create(
        &db,
        Uuid::now_v7(),
        VignetteInput {
            media_id: page.id,
            x: 100,
            y: 100,
            width: 400,
            height: 400,
            person_id: None,
            event_id: None,
        },
    )
    .await
    .unwrap();

    assert!(
        MediaRepo::update(
            &db,
            page.id,
            MediaPatch {
                dimensions: Some((300, 300)),
                ..Default::default()
            },
        )
        .await
        .is_err(),
        "the region reaches past the size being claimed"
    );
    MediaRepo::update(
        &db,
        page.id,
        MediaPatch {
            dimensions: Some((1600, 1200)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn a_chosen_portrait_resolves_to_the_page_that_holds_the_pixels() {
    // What a reader picks in a gallery is a tile, and a tile is a document. A
    // document holds no bytes, so reporting it would hand the caller an id
    // whose file does not exist — a silhouette where a photograph was chosen.
    let (db, tree, document) = setup().await;
    let cover = MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), rasterised())
        .await
        .unwrap();
    MediaRepo::create_uploaded(&db, Uuid::now_v7(), tree, Some(document), rasterised())
        .await
        .unwrap();
    let person = Uuid::now_v7();
    PersonRepo::create(&db, person, tree, Sex::Unknown)
        .await
        .unwrap();
    PersonRepo::set_portrait(&db, person, Portrait::Media(document))
        .await
        .unwrap();

    let rows = PersonRepo::list_portraits_for(&db, tree, &[person])
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].media_id, Some(cover.id), "the first page, in order");
    assert!(rows[0].has_thumbnail);
}

#[tokio::test]
async fn a_chosen_portrait_may_be_a_page_we_only_have_a_url_for() {
    // A `.gw` or a GEDCOM routinely names a photograph it does not carry. The
    // browser can fetch it perfectly well, so it represents somebody as
    // readily as a file of ours — and the document above it still has no
    // pixels of its own.
    let (db, tree, document) = setup().await;
    let url = "https://archives.example.invalid/scan/42.jpg";
    let page = remote_page(&db, tree, document, url).await;
    let person = Uuid::now_v7();
    PersonRepo::create(&db, person, tree, Sex::Unknown)
        .await
        .unwrap();
    PersonRepo::set_portrait(&db, person, Portrait::Media(document))
        .await
        .unwrap();

    let rows = PersonRepo::list_portraits_for(&db, tree, &[person])
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].media_id, Some(page.id));
    assert_eq!(rows[0].file_path, url);
    assert!(!rows[0].has_thumbnail, "nothing of ours was rasterised");
}

#[tokio::test]
async fn a_portrait_nobody_chose_is_still_the_first_photograph_linked() {
    // The fallback and the chosen path now run the same page resolution, so
    // this pins that the fallback still finds a document's page through a
    // link — and that the two cannot drift apart.
    let (db, tree, document) = setup().await;
    let page = remote_page(
        &db,
        tree,
        document,
        "https://archives.example.invalid/scan/7.jpg",
    )
    .await;
    let person = Uuid::now_v7();
    PersonRepo::create(&db, person, tree, Sex::Unknown)
        .await
        .unwrap();
    MediaLinkRepo::create(
        &db,
        Uuid::now_v7(),
        document,
        Some(person),
        None,
        None,
        None,
        0,
    )
    .await
    .unwrap();

    let rows = PersonRepo::list_portraits_for(&db, tree, &[person])
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].media_id, Some(page.id));
}
