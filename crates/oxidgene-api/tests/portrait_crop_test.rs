//! Stored crops (docs/architecture.md §4.2): the picture cut out of a page
//! for a vignette is stored on its first view and read back afterwards,
//! byte for byte what a fresh cut gives; every write that makes it
//! unreachable deletes it; the maintenance sweep deletes what is left.
//!
//! Each test has a media root of its own, so what the store holds is the
//! test's alone. The pictures are generated gradients: nobody's photograph.

use crate::common;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use oxidgene_api::media::store::{BlobStream, StoredCrop, StoredObject};
use oxidgene_api::media::thumbnail::{self, Rendition};
use oxidgene_api::media::{FsStore, MediaStore};
use oxidgene_api::profile::ProfileService;
use oxidgene_api::service::background_job::BackgroundJobWorker;
use oxidgene_api::{AppState, build_router};
use oxidgene_core::OxidGeneError;
use oxidgene_db::sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{new_person, send, send_bytes, setup_db, stored_crops};

/// A media directory that removes itself when the test ends.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("oxidgene-crop-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("create media root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A router over its own media root, a tree, and a page holding a
/// generated photograph.
struct Harness {
    app: axum::Router,
    db: DatabaseConnection,
    store: Arc<dyn MediaStore>,
    root: TempRoot,
    tree_id: String,
    document_id: String,
    page_id: String,
    original: Vec<u8>,
}

impl Harness {
    async fn new() -> Self {
        let root = TempRoot::new();
        Self::with_store(root, |path| Arc::new(FsStore::new(path))).await
    }

    async fn with_store(root: TempRoot, store: impl FnOnce(&Path) -> Arc<dyn MediaStore>) -> Self {
        let db = setup_db().await;
        let store = store(&root.0);
        let app = build_router(common::offline(AppState::with_media_store(
            db.clone(),
            Arc::clone(&store),
        )));
        let tree_id = common::new_tree(&app, "Crops").await;
        let (status, document) = send(
            &app,
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media/document"),
            Some(json!({ "title": "Group photograph" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{document}");
        let document_id = document["id"].as_str().unwrap().to_string();
        let original = photograph(640, 480, 0);
        let mut harness = Self {
            app,
            db,
            store,
            root,
            tree_id,
            document_id,
            page_id: String::new(),
            original,
        };
        let page = harness
            .upload(
                &harness.original.clone(),
                ("document_id", &harness.document_id.clone()),
            )
            .await;
        harness.page_id = page["id"].as_str().unwrap().to_string();
        harness
    }

    fn base(&self) -> String {
        format!("/api/v1/trees/{}", self.tree_id)
    }

    /// Upload `bytes`, as a new page of a document or onto a page, as
    /// `target` says.
    async fn upload(&self, bytes: &[u8], target: (&str, &str)) -> Value {
        let boundary = "----oxidgeneCropBoundary";
        let mut body = Vec::new();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"group.jpg\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n{}\r\n--{boundary}--\r\n",
                target.0, target.1
            )
            .as_bytes(),
        );
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("{}/media/upload", self.base()))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(status.is_success(), "upload: {status}");
        serde_json::from_slice(&bytes).unwrap()
    }

    /// A vignette of the page, attributed to `person_id` when given.
    async fn vignette(&self, rect: (i32, i32, i32, i32), person_id: Option<&str>) -> String {
        let (status, vignette) = send(
            &self.app,
            Method::POST,
            &format!("{}/media/{}/vignettes", self.base(), self.page_id),
            Some(json!({
                "x": rect.0, "y": rect.1, "width": rect.2, "height": rect.3,
                "person_id": person_id,
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{vignette}");
        vignette["id"].as_str().unwrap().to_string()
    }

    /// `GET` the vignette's crop at `size` (`card` or `image`).
    async fn view(&self, vignette: &str, size: &str) -> (StatusCode, Vec<u8>) {
        send_bytes(
            &self.app,
            &format!("{}/vignettes/{vignette}/{size}", self.base()),
        )
        .await
    }

    /// View both sizes, requiring them served; the card and the image.
    async fn warm(&self, vignette: &str) -> (Vec<u8>, Vec<u8>) {
        let (status, card) = self.view(vignette, "card").await;
        assert_eq!(status, StatusCode::OK);
        let (status, image) = self.view(vignette, "image").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(self.crops(vignette).len(), 2, "both sizes stored");
        (card, image)
    }

    fn crops(&self, vignette: &str) -> Vec<PathBuf> {
        stored_crops(&self.root.0, vignette)
    }

    /// Every crop file under the media root.
    fn all_crops(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for tree in std::fs::read_dir(&self.root.0)
            .into_iter()
            .flatten()
            .flatten()
        {
            for vignette in std::fs::read_dir(tree.path().join("crops"))
                .into_iter()
                .flatten()
                .flatten()
            {
                found.extend(
                    std::fs::read_dir(vignette.path())
                        .into_iter()
                        .flatten()
                        .flatten()
                        .map(|file| file.path()),
                );
            }
        }
        found
    }
}

/// A generated photograph: a gradient with a pattern, so a crop of it has
/// content to encode. `seed` makes two of them differ.
fn photograph(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            ((x + seed) % 256) as u8,
            ((y * 2 + seed) % 256) as u8,
            (((x ^ y) + seed) % 256) as u8,
        ])
    });
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

/// What a fresh cut of `rect` out of `original` gives at `rendition`.
fn fresh_cut(original: &[u8], rect: (i32, i32, i32, i32), rendition: Rendition) -> Vec<u8> {
    thumbnail::crop(original, rect, rendition).unwrap().unwrap()
}

const RECT: (i32, i32, i32, i32) = (100, 80, 240, 200);

/// One `GET uri`: the status, the headers and the body, a redirect
/// answered as it is.
async fn get(app: &axum::Router, uri: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let request = Request::builder().method(Method::GET).uri(uri);
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes.to_vec())
}

/// The vignette's version, as its address carries it: what the route
/// redirects an address without one to.
async fn version(app: &axum::Router, uri: &str) -> String {
    let (status, headers, _) = get(app, uri).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    let location = headers[header::LOCATION].to_str().unwrap();
    location.split_once("?v=").unwrap().1.to_string()
}

/// A crop is immutable at its version, the same for both sizes, and a moved
/// rectangle or a replaced file is a new version: the old address redirects
/// there and never holds the new picture. The version names nothing; a
/// vignette gone answers 404 whatever version is asked
/// (docs/architecture.md §4.3).
#[tokio::test]
async fn a_crop_is_immutable_at_its_version_which_moves_with_it() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    let card = format!("{}/vignettes/{vignette}/card", h.base());
    let image = format!("{}/vignettes/{vignette}/image", h.base());
    let first = version(&h.app, &card).await;
    assert_eq!(first.len(), 16, "{first}");
    assert!(!h.page_id.contains(&first) && !vignette.contains(&first));
    assert_eq!(
        version(&h.app, &image).await,
        first,
        "one version, both sizes"
    );
    for uri in [&card, &image] {
        let (status, headers, body) = get(&h.app, &format!("{uri}?v={first}")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.is_empty());
        assert_eq!(
            headers[header::CACHE_CONTROL],
            "private, max-age=31536000, immutable"
        );
        assert!(headers.get(header::ETAG).is_none());
    }

    let moved = (0, 0, 320, 320);
    let (status, _) = send(
        &h.app,
        Method::PUT,
        &format!("{}/vignettes/{vignette}", h.base()),
        Some(json!({ "x": moved.0, "y": moved.1, "width": moved.2, "height": moved.3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, headers, body) = get(&h.app, &format!("{card}?v={first}")).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "a moved rectangle");
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert!(body.is_empty());
    let second = version(&h.app, &card).await;
    assert_ne!(second, first);
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        format!("{card}?v={second}")
    );
    let (_, _, body) = get(&h.app, &format!("{card}?v={second}")).await;
    assert_eq!(body, fresh_cut(&h.original, moved, Rendition::Card));

    let replacement = photograph(640, 480, 33);
    h.upload(&replacement, ("media_id", &h.page_id)).await;
    let (status, _, _) = get(&h.app, &format!("{card}?v={second}")).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "a replaced file");
    let third = version(&h.app, &card).await;
    assert_ne!(third, second);

    let (status, _) = send(
        &h.app,
        Method::DELETE,
        &format!("{}/vignettes/{vignette}", h.base()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _, _) = get(&h.app, &format!("{card}?v={third}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_stored_crop_is_a_fresh_cut_and_is_read_back_from_the_store() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    assert!(
        h.crops(&vignette).is_empty(),
        "nothing is cut before a view"
    );

    let (card, image) = h.warm(&vignette).await;
    assert_eq!(card, fresh_cut(&h.original, RECT, Rendition::Card));
    assert_eq!(image, fresh_cut(&h.original, RECT, Rendition::Thumbnail));
    for file in h.crops(&vignette) {
        let stored = std::fs::read(&file).unwrap();
        assert!(stored == card || stored == image, "{}", file.display());
    }

    // A second view reads the store, not the page: a marker written over
    // the stored card is what comes back.
    let card_file = h
        .crops(&vignette)
        .into_iter()
        .find(|file| file.to_string_lossy().ends_with("-card.jpg"))
        .unwrap();
    let marker = photograph(8, 8, 7);
    std::fs::write(&card_file, &marker).unwrap();
    assert_eq!(h.view(&vignette, "card").await.1, marker);
}

#[tokio::test]
async fn moving_the_rectangle_deletes_its_crops_and_the_next_view_cuts_the_new_one() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    h.warm(&vignette).await;
    let before = h.crops(&vignette);

    // Re-attributing it changes no picture: the crops stay.
    let person = new_person(&h.app, &h.tree_id).await;
    let (status, _) = send(
        &h.app,
        Method::PUT,
        &format!("{}/vignettes/{vignette}", h.base()),
        Some(json!({ "person_id": person })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h.crops(&vignette), before);

    let moved = (10, 10, 300, 300);
    let (status, _) = send(
        &h.app,
        Method::PUT,
        &format!("{}/vignettes/{vignette}", h.base()),
        Some(json!({ "x": moved.0, "y": moved.1, "width": moved.2, "height": moved.3 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(h.crops(&vignette).is_empty(), "the old crops are deleted");

    let (card, _) = h.warm(&vignette).await;
    assert_eq!(card, fresh_cut(&h.original, moved, Rendition::Card));
    assert!(
        h.crops(&vignette).iter().all(|file| !before.contains(file)),
        "the new crops have keys of their own"
    );
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_writes_delete_the_crops_as_rest_does() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    h.warm(&vignette).await;
    let moved = common::gql(
        &h.app,
        "mutation($tree: ID!, $id: ID!) { updateVignette(treeId: $tree, id: $id, \
         input: { x: 0, y: 0, width: 50, height: 50 }) { id x } }",
        json!({ "tree": h.tree_id, "id": vignette }),
    )
    .await;
    assert_eq!(moved["data"]["updateVignette"]["x"], 0, "{moved}");
    assert!(h.crops(&vignette).is_empty());

    h.warm(&vignette).await;
    let deleted = common::gql(
        &h.app,
        "mutation($tree: ID!, $id: ID!) { deleteVignette(treeId: $tree, id: $id) }",
        json!({ "tree": h.tree_id, "id": vignette }),
    )
    .await;
    assert_eq!(deleted["data"]["deleteVignette"], true, "{deleted}");
    assert!(h.crops(&vignette).is_empty());
}

#[tokio::test]
async fn deleting_the_vignette_deletes_its_crops_and_answers_404_after() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    let kept = h.vignette((0, 0, 64, 64), None).await;
    h.warm(&vignette).await;
    h.warm(&kept).await;

    let (status, _) = send(
        &h.app,
        Method::DELETE,
        &format!("{}/vignettes/{vignette}", h.base()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(h.crops(&vignette).is_empty());
    assert_eq!(h.crops(&kept).len(), 2, "another vignette keeps its own");
    assert_eq!(h.view(&vignette, "card").await.0, StatusCode::NOT_FOUND);
    assert_eq!(h.view(&vignette, "image").await.0, StatusCode::NOT_FOUND);
    assert!(
        h.crops(&vignette).is_empty(),
        "a refused view stores nothing"
    );
}

#[tokio::test]
async fn replacing_the_file_deletes_the_crops_cut_from_the_old_one() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    let (old_card, _) = h.warm(&vignette).await;

    let replacement = photograph(640, 480, 91);
    let page = h.upload(&replacement, ("media_id", &h.page_id)).await;
    assert_eq!(page["id"], h.page_id.as_str());
    assert!(h.crops(&vignette).is_empty(), "the old crops are deleted");

    let (card, image) = h.warm(&vignette).await;
    assert_ne!(card, old_card);
    assert_eq!(card, fresh_cut(&replacement, RECT, Rendition::Card));
    assert_eq!(image, fresh_cut(&replacement, RECT, Rendition::Thumbnail));
}

#[tokio::test]
async fn deleting_the_page_or_the_document_deletes_the_crops_of_its_vignettes() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    h.warm(&vignette).await;
    let (status, _) = send(
        &h.app,
        Method::DELETE,
        &format!("{}/media/{}/pages/{}", h.base(), h.document_id, h.page_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(h.crops(&vignette).is_empty());
    assert_eq!(h.view(&vignette, "card").await.0, StatusCode::NOT_FOUND);

    // A document deleted whole takes its pages' vignettes and crops.
    let page = h
        .upload(&photograph(320, 240, 5), ("document_id", &h.document_id))
        .await;
    let page_id = page["id"].as_str().unwrap();
    let (_, other) = send(
        &h.app,
        Method::POST,
        &format!("{}/media/{page_id}/vignettes", h.base()),
        Some(json!({ "x": 0, "y": 0, "width": 100, "height": 100 })),
    )
    .await;
    let other = other["id"].as_str().unwrap().to_string();
    h.warm(&other).await;
    let (status, _) = send(
        &h.app,
        Method::DELETE,
        &format!("{}/media/{}", h.base(), h.document_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(h.all_crops().is_empty(), "{:?}", h.all_crops());
    assert_eq!(h.view(&other, "image").await.0, StatusCode::NOT_FOUND);
}

/// Deleting or merging away the person a vignette shows removes no
/// vignette: the crop lives as long as its vignette, its rectangle and its
/// page's file, and is still served for the vignette.
#[tokio::test]
async fn deleting_or_merging_the_person_shown_keeps_the_vignette_and_its_crops() {
    let h = Harness::new().await;
    let shown = new_person(&h.app, &h.tree_id).await;
    let kept = new_person(&h.app, &h.tree_id).await;
    let vignette = h.vignette(RECT, Some(&shown)).await;
    h.warm(&vignette).await;
    let stored = h.crops(&vignette);

    let (status, _) = send(
        &h.app,
        Method::POST,
        &format!("{}/persons/{kept}/merge", h.base()),
        Some(json!({ "duplicate_id": shown })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h.crops(&vignette), stored);
    let (status, _) = send(
        &h.app,
        Method::DELETE,
        &format!("{}/persons/{kept}", h.base()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(h.crops(&vignette), stored);
    assert_eq!(h.view(&vignette, "card").await.0, StatusCode::OK);
}

#[tokio::test]
async fn deleting_the_tree_purges_its_crops() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    h.warm(&vignette).await;

    let (status, _) = send(&h.app, Method::DELETE, &h.base(), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    for _ in 0..200 {
        if h.all_crops().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(h.all_crops().is_empty(), "{:?}", h.all_crops());
}

#[tokio::test]
async fn a_soft_deleted_page_serves_no_crop() {
    use oxidgene_db::sea_orm::{ConnectionTrait, DbBackend, Statement};
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    h.warm(&vignette).await;

    // Nothing in the application soft-deletes a medium, but every read
    // excludes a flagged row, and so must a stored crop.
    h.db.execute_raw(Statement::from_sql_and_values(
        h.db.get_database_backend(),
        match h.db.get_database_backend() {
            DbBackend::Postgres => "UPDATE media SET deleted_at = $1 WHERE id = $2",
            _ => "UPDATE media SET deleted_at = ? WHERE id = ?",
        },
        [
            chrono::Utc::now().into(),
            Uuid::parse_str(&h.page_id).unwrap().into(),
        ],
    ))
    .await
    .unwrap();
    assert_eq!(h.view(&vignette, "card").await.0, StatusCode::NOT_FOUND);
    assert_eq!(h.view(&vignette, "image").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_export_never_carries_a_crop() {
    let h = Harness::new().await;
    let vignette = h.vignette(RECT, None).await;
    let (card, image) = h.warm(&vignette).await;
    // Linked to somebody, so the export carries the photograph.
    let person = new_person(&h.app, &h.tree_id).await;
    let (status, link) = send(
        &h.app,
        Method::POST,
        &format!("{}/media-links", h.base()),
        Some(json!({ "media_id": h.document_id, "person_id": person })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{link}");

    let (status, archive) =
        send_bytes(&h.app, &format!("{}/gedcom/export?format=gedzip", h.base())).await;
    assert_eq!(status, StatusCode::OK);
    let mut archive = zip::ZipArchive::new(Cursor::new(archive)).expect("a GEDZIP");
    let mut originals = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let name = entry
            .name()
            .map(|name| name.into_owned())
            .unwrap_or_default();
        assert!(!name.contains("crops"), "{name}");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).unwrap();
        assert!(bytes != card && bytes != image, "{name} is a crop");
        originals += usize::from(bytes == h.original);
    }
    assert_eq!(originals, 1, "the original is exported, once");
}

/// The maintenance sweep deletes what a crash, a failed deletion or a race
/// left: the crops of a vignette gone without its deletion running, a crop
/// whose key is no longer its vignette's, the crops of a soft-deleted tree,
/// and those of a tree this database does not hold once its id is a day
/// old — a database recreated beside its store, as a reimport after a
/// schema change does.
#[tokio::test]
async fn the_maintenance_sweep_deletes_every_crop_no_read_reaches() {
    let h = Harness::new().await;
    let current = h.vignette(RECT, None).await;
    let vanished = h.vignette((0, 0, 64, 64), None).await;
    h.warm(&current).await;
    h.warm(&vanished).await;
    let current_files = h.crops(&current);

    // A vignette deleted behind the service's back, as a crash after the
    // commit and before the deletion leaves it.
    oxidgene_db::repo::VignetteRepo::delete(&h.db, Uuid::parse_str(&vanished).unwrap())
        .await
        .unwrap();
    // A crop cut for a rectangle the vignette no longer has.
    let tree_id = Uuid::parse_str(&h.tree_id).unwrap();
    let stale = oxidgene_api::media::store::crop_key(
        tree_id,
        Uuid::parse_str(&current).unwrap(),
        &"0".repeat(64),
        "card",
    );
    h.store
        .put_crop(&stale, Bytes::from_static(b"stale"))
        .await
        .unwrap();
    // A tree of another database sharing this store.
    let elsewhere = Harness::with_store(TempRoot(h.root.0.clone()), |_| Arc::clone(&h.store)).await;
    let foreign = elsewhere.vignette(RECT, None).await;
    elsewhere.warm(&foreign).await;
    std::mem::forget(elsewhere.root);

    let worker = BackgroundJobWorker::new(
        h.db.clone(),
        Arc::new(ProfileService::new(h.db.clone())),
        Arc::clone(&h.store),
        "crop-sweep-test",
    );
    let now = chrono::Utc::now();
    worker.maintain(now).await;
    assert_eq!(h.crops(&current), current_files, "current crops stay");
    assert!(h.crops(&vanished).is_empty(), "a vanished vignette's go");
    assert_eq!(
        h.crops(&foreign).len(),
        2,
        "another database's recent tree is left alone"
    );
    worker.maintain(now + chrono::Duration::hours(25)).await;
    assert!(h.crops(&foreign).is_empty(), "a day later its crops go");
    assert_eq!(h.crops(&current), current_files);

    // A soft-deleted tree awaiting its purge keeps no crop.
    oxidgene_db::repo::TreeRepo::soft_delete(&h.db, tree_id)
        .await
        .unwrap();
    worker.maintain(now).await;
    assert!(h.all_crops().is_empty(), "{:?}", h.all_crops());
}

/// A store over a directory counting the crops written through it.
#[derive(Debug)]
struct CountingStore {
    inner: FsStore,
    crops_written: AtomicUsize,
}

#[async_trait::async_trait]
impl MediaStore for CountingStore {
    async fn put_content(
        &self,
        key: &str,
        sha256: String,
        bytes: Bytes,
    ) -> Result<StoredObject, OxidGeneError> {
        self.inner.put_content(key, sha256, bytes).await
    }
    async fn put_job_bytes(&self, key: &str, bytes: &[u8]) -> Result<(), OxidGeneError> {
        self.inner.put_job_bytes(key, bytes).await
    }
    #[cfg(feature = "auth")]
    async fn put_user_bytes(&self, key: &str, bytes: &[u8]) -> Result<(), OxidGeneError> {
        self.inner.put_user_bytes(key, bytes).await
    }
    async fn job_keys(&self, job_id: Uuid) -> Result<Vec<String>, OxidGeneError> {
        self.inner.job_keys(job_id).await
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>, OxidGeneError> {
        self.inner.get(key).await
    }
    async fn put_file(&self, key: &str, path: &Path) -> Result<StoredObject, OxidGeneError> {
        self.inner.put_file(key, path).await
    }
    async fn get_to_file(&self, key: &str, path: &Path) -> Result<(), OxidGeneError> {
        self.inner.get_to_file(key, path).await
    }
    async fn get_stream(&self, key: &str) -> Result<BlobStream, OxidGeneError> {
        self.inner.get_stream(key).await
    }
    async fn exists(&self, key: &str) -> bool {
        self.inner.exists(key).await
    }
    async fn delete(&self, key: &str) -> Result<(), OxidGeneError> {
        self.inner.delete(key).await
    }
    async fn delete_tree(&self, tree_id: Uuid) -> Result<(), OxidGeneError> {
        self.inner.delete_tree(tree_id).await
    }
    async fn delete_job(&self, job_id: Uuid) -> Result<(), OxidGeneError> {
        self.inner.delete_job(job_id).await
    }
    async fn job_ids(&self) -> Result<Vec<Uuid>, OxidGeneError> {
        self.inner.job_ids().await
    }
    async fn put_crop(&self, key: &str, bytes: Bytes) -> Result<(), OxidGeneError> {
        self.crops_written.fetch_add(1, Ordering::SeqCst);
        self.inner.put_crop(key, bytes).await
    }
    async fn crop_objects(&self, tree_id: Uuid) -> Result<Vec<StoredCrop>, OxidGeneError> {
        self.inner.crop_objects(tree_id).await
    }
    async fn delete_crops(
        &self,
        tree_id: Uuid,
        vignette_id: Option<Uuid>,
    ) -> Result<(), OxidGeneError> {
        self.inner.delete_crops(tree_id, vignette_id).await
    }
    async fn tree_ids(&self) -> Result<Vec<Uuid>, OxidGeneError> {
        self.inner.tree_ids().await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_views_cut_and_write_once() {
    let root = TempRoot::new();
    let counting = Arc::new(CountingStore {
        inner: FsStore::new(&root.0),
        crops_written: AtomicUsize::new(0),
    });
    let store: Arc<dyn MediaStore> = counting.clone();
    let h = Harness::with_store(root, |_| store).await;
    let vignette = h.vignette(RECT, None).await;

    let views = (0..8).map(|_| {
        let app = h.app.clone();
        let uri = format!("{}/vignettes/{vignette}/card", h.base());
        tokio::spawn(async move { send_bytes(&app, &uri).await })
    });
    let answers = futures_util::future::try_join_all(views).await.unwrap();

    let fresh = fresh_cut(&h.original, RECT, Rendition::Card);
    assert!(
        answers
            .iter()
            .all(|(status, bytes)| *status == StatusCode::OK && *bytes == fresh)
    );
    assert_eq!(counting.crops_written.load(Ordering::SeqCst), 1);
    assert_eq!(h.crops(&vignette).len(), 1);
}

/// The same lifecycle on the web's S3 store: a crop stored on its first
/// view and read back from the bucket, the crops of a moved rectangle
/// deleted, and the maintenance sweep's deletions.
#[cfg(feature = "s3")]
#[tokio::test]
#[ignore = "needs the RustFS service of docker/docker-compose.yml: run by `just test-s3`"]
async fn crops_live_and_die_on_the_s3_store_as_on_disk() {
    use oxidgene_api::media::{S3Store, S3StoreConfig};

    let s3 = S3Store::new(S3StoreConfig {
        bucket: "oxidgene-media".to_string(),
        region: "us-east-1".to_string(),
        endpoint: Some("http://127.0.0.1:9000".to_string()),
        access_key_id: "oxidgene-admin".to_string(),
        secret_access_key: "oxidgene-development-secret".to_string(),
    })
    .expect("build RustFS store");
    let h = Harness::with_store(TempRoot::new(), |_| Arc::new(s3)).await;
    let tree_id = Uuid::parse_str(&h.tree_id).unwrap();
    let keys_of = |vignette: &str| {
        let vignette = Uuid::parse_str(vignette).unwrap();
        let store = Arc::clone(&h.store);
        async move {
            let mut keys: Vec<String> = store
                .crop_objects(tree_id)
                .await
                .unwrap()
                .into_iter()
                .filter(|crop| crop.vignette_id == vignette)
                .map(|crop| crop.key)
                .collect();
            keys.sort();
            keys
        }
    };

    // Stored on the first view, read back from the bucket afterwards.
    let vignette = h.vignette(RECT, None).await;
    let (status, card) = h.view(&vignette, "card").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(card, fresh_cut(&h.original, RECT, Rendition::Card));
    let (status, _) = h.view(&vignette, "image").await;
    assert_eq!(status, StatusCode::OK);
    let stored = keys_of(&vignette).await;
    assert_eq!(stored.len(), 2, "{stored:?}");
    let card_key = stored
        .iter()
        .find(|key| key.ends_with("-card.jpg"))
        .unwrap();
    assert_eq!(h.store.get(card_key).await.unwrap(), card);
    let marker = photograph(8, 8, 3);
    h.store
        .put_crop(card_key, Bytes::from(marker.clone()))
        .await
        .unwrap();
    assert_eq!(
        h.view(&vignette, "card").await.1,
        marker,
        "read from the bucket"
    );

    // Moving the rectangle deletes them; the next view cuts the new one.
    let (status, _) = send(
        &h.app,
        Method::PUT,
        &format!("{}/vignettes/{vignette}", h.base()),
        Some(json!({ "x": 0, "y": 0, "width": 200, "height": 200 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(keys_of(&vignette).await.is_empty());
    let (_, moved) = h.view(&vignette, "card").await;
    assert_eq!(
        moved,
        fresh_cut(&h.original, (0, 0, 200, 200), Rendition::Card)
    );
    let current = keys_of(&vignette).await;
    assert_eq!(current.len(), 1);

    // The sweep: a vanished vignette's crops and a stale key go, the
    // current crop stays; a soft-deleted tree keeps none.
    let vanished = h.vignette((0, 0, 64, 64), None).await;
    h.view(&vanished, "card").await;
    oxidgene_db::repo::VignetteRepo::delete(&h.db, Uuid::parse_str(&vanished).unwrap())
        .await
        .unwrap();
    let stale = oxidgene_api::media::store::crop_key(
        tree_id,
        Uuid::parse_str(&vignette).unwrap(),
        &"0".repeat(64),
        "card",
    );
    h.store
        .put_crop(&stale, Bytes::from_static(b"stale"))
        .await
        .unwrap();
    let worker = BackgroundJobWorker::new(
        h.db.clone(),
        Arc::new(ProfileService::new(h.db.clone())),
        Arc::clone(&h.store),
        "crop-sweep-s3-test",
    );
    worker.maintain(chrono::Utc::now()).await;
    assert_eq!(keys_of(&vignette).await, current);
    assert!(keys_of(&vanished).await.is_empty());
    oxidgene_db::repo::TreeRepo::soft_delete(&h.db, tree_id)
        .await
        .unwrap();
    worker.maintain(chrono::Utc::now()).await;
    assert!(h.store.crop_objects(tree_id).await.unwrap().is_empty());

    h.store.delete_tree(tree_id).await.unwrap();
}

/// Processor time this process has used, user and system.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn cpu_time() -> std::time::Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `getrusage` fills the whole struct for `RUSAGE_SELF` and
    // reports failure only for an invalid `who`, which this is not.
    let usage = unsafe {
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()), 0);
        usage.assume_init()
    };
    let time = |value: libc::timeval| {
        std::time::Duration::from_secs(u64::try_from(value.tv_sec).unwrap_or_default())
            + std::time::Duration::from_micros(u64::try_from(value.tv_usec).unwrap_or_default())
    };
    time(usage.ru_utime) + time(usage.ru_stime)
}

/// A generated photograph of `width` × `height` that weighs what a camera's
/// does: a gradient under pseudo-random grain, `seed` making each differ.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn camera_photograph(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let grain = (state % 96) as u8;
        image::Rgb([
            ((x / 12 + seed) % 160) as u8 + grain,
            ((y / 9 + seed * 3) % 160) as u8 + grain / 2,
            (((x + y) / 16) % 160) as u8 + grain / 3,
        ])
    });
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82)
        .encode_image(&image)
        .unwrap();
    out.into_inner()
}

/// What a pedigree view costs in portraits, cold (every crop cut from its
/// original) and warm (every crop stored), on a generated tree of a few
/// hundred portraits cut out of camera-sized photographs. A view asks for
/// the card crops of twenty persons, as a ten-generation chart's first
/// screen does. Reports figures; asserts only that a warm
/// view is cheaper.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "timing on generated photographs: run by `just crop-bench` in release mode"]
async fn a_pedigree_view_reads_stored_crops_instead_of_cutting_them() {
    const PORTRAITS: usize = 240;
    const PER_VIEW: usize = 20;
    let root = TempRoot(
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("crop-bench-{}", Uuid::now_v7())),
    );
    std::fs::create_dir_all(&root.0).unwrap();
    let h = Harness::with_store(root, |path| Arc::new(FsStore::new(path))).await;

    let started = std::time::Instant::now();
    let mut original_bytes = 0;
    let mut vignettes = Vec::with_capacity(PORTRAITS);
    for index in 0..PORTRAITS {
        let seed = u32::try_from(index).unwrap();
        let photograph = camera_photograph(3000, 2250, seed);
        original_bytes += photograph.len();
        let page = h.upload(&photograph, ("document_id", &h.document_id)).await;
        let page_id = page["id"].as_str().unwrap();
        // A face in a group: a fifth of the picture's width, somewhere in it.
        let x = 300 + (index % 7) * 300;
        let y = 200 + (index % 5) * 250;
        let (status, vignette) = send(
            &h.app,
            Method::POST,
            &format!("{}/media/{page_id}/vignettes", h.base()),
            Some(json!({ "x": x, "y": y, "width": 600, "height": 720 })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{vignette}");
        vignettes.push(vignette["id"].as_str().unwrap().to_string());
    }
    eprintln!(
        "fixture: {PORTRAITS} photographs 3000x2250, {:.2} MB each on average, built in {:.1} s",
        original_bytes as f64 / PORTRAITS as f64 / 1e6,
        started.elapsed().as_secs_f64()
    );

    let address = |id: &String, size: &str| format!("{}/vignettes/{id}/{size}", h.base());
    let mut cards = Vec::with_capacity(vignettes.len());
    for id in &vignettes {
        cards.push(common::current_address(&h.app, &address(id, "card")).await);
    }
    let measure = |pass: &'static str| {
        let app = h.app.clone();
        let views: Vec<Vec<String>> = cards.chunks(PER_VIEW).map(<[String]>::to_vec).collect();
        async move {
            let (mut wall, mut cpu) = (Vec::new(), Vec::new());
            for view in views {
                let (wall_before, cpu_before) = (std::time::Instant::now(), cpu_time());
                for card in &view {
                    let (status, _) = send_bytes(&app, card).await;
                    assert_eq!(status, StatusCode::OK);
                }
                wall.push(wall_before.elapsed());
                cpu.push(cpu_time() - cpu_before);
            }
            let mean = |times: &[std::time::Duration]| {
                times.iter().sum::<std::time::Duration>().as_secs_f64() * 1e3 / times.len() as f64
            };
            let (wall, cpu) = (mean(&wall), mean(&cpu));
            eprintln!(
                "{pass} pedigree view ({PER_VIEW} card crops): wall {wall:.1} ms, CPU {cpu:.1} ms"
            );
            (wall, cpu)
        }
    };
    let (cold_wall, cold_cpu) = measure("cold").await;
    let (warm_wall, warm_cpu) = measure("warm").await;
    eprintln!(
        "saved per view: wall {:.1} ms, CPU {:.1} ms; first-view cost per crop: wall {:.1} ms, CPU {:.1} ms",
        cold_wall - warm_wall,
        cold_cpu - warm_cpu,
        (cold_wall - warm_wall) / PER_VIEW as f64,
        (cold_cpu - warm_cpu) / PER_VIEW as f64,
    );

    // The thumbnail size, which profiles and galleries draw, stored too.
    for id in &vignettes {
        let (status, _) = send_bytes(&h.app, &address(id, "image")).await;
        assert_eq!(status, StatusCode::OK);
    }
    let stored = h
        .store
        .crop_objects(Uuid::parse_str(&h.tree_id).unwrap())
        .await
        .unwrap();
    for size in ["card", "thumbnail"] {
        let sizes: Vec<u64> = stored
            .iter()
            .filter(|crop| crop.key.ends_with(&format!("-{size}.jpg")))
            .map(|crop| crop.size)
            .collect();
        assert_eq!(sizes.len(), PORTRAITS);
        let total: u64 = sizes.iter().sum();
        eprintln!(
            "{size} crops: {PORTRAITS} stored, {:.1} KB each on average ({:.1}-{:.1} KB), {:.2} MB in all",
            total as f64 / PORTRAITS as f64 / 1e3,
            *sizes.iter().min().unwrap() as f64 / 1e3,
            *sizes.iter().max().unwrap() as f64 / 1e3,
            total as f64 / 1e6,
        );
    }
    assert!(
        warm_cpu < cold_cpu,
        "a warm view costs less than a cold one"
    );
}
