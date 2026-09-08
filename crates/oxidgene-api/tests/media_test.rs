//! Integration tests for media upload, serving and vignettes (Sprint F.1).
//!
//! Each test gets its own SQLite database and its own media directory, so an
//! upload in one cannot be observed by another, and nothing is left on disk.

use std::io::Cursor;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::{connect, run_migrations};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

// ── Harness ─────────────────────────────────────────────────────────

/// A media directory that removes itself when the test ends.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("oxidgene-media-test-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).expect("create media root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A router, its media root, and a tree to hang media off.
struct Harness {
    app: axum::Router,
    db: sea_orm::DatabaseConnection,
    root: TempRoot,
    tree_id: Uuid,
}

async fn setup() -> Harness {
    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrations");
    let root = TempRoot::new();
    let app = build_router(AppState::new(db.clone(), &root.0));

    let (status, tree) = json_request(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({"name": "Test tree"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "tree setup failed: {tree}");
    let tree_id = Uuid::parse_str(tree["id"].as_str().unwrap()).unwrap();

    Harness {
        app,
        db,
        root,
        tree_id,
    }
}

async fn json_request(
    app: &axum::Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let body = match body {
        Some(json) => Body::from(serde_json::to_vec(&json).unwrap()),
        None => Body::empty(),
    };
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// Build a multipart body by hand — there is no multipart writer in the dev
/// dependencies, and the format is three lines of framing per part.
fn multipart(parts: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "----oxidgeneTestBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::new();
    for (name, file_name, content) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match file_name {
            Some(file_name) => body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n\
                     Content-Type: application/octet-stream\r\n\r\n"
                )
                .as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Create an empty document, the container every set of bytes is a page of.
async fn new_document(app: &axum::Router, tree_id: Uuid, title: Option<&str>) -> String {
    let (status, document) = json_request(
        app,
        Method::POST,
        &format!("/api/v1/trees/{tree_id}/media/document"),
        Some(json!({ "title": title })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "document setup failed: {document}"
    );
    document["id"].as_str().unwrap().to_string()
}

/// Upload a file as a page.
///
/// Every set of bytes is a page of a document, so a caller that names neither
/// a document nor an existing page to fill in gets a fresh single-page
/// document. Tests about a document's own behaviour pass their own
/// `document_id` part instead.
async fn upload(
    app: &axum::Router,
    tree_id: Uuid,
    parts: &[(&str, Option<&str>, &[u8])],
) -> (StatusCode, Value) {
    let named = parts
        .iter()
        .any(|(name, _, _)| *name == "document_id" || *name == "media_id");
    let document_id;
    let owned;
    let parts = if named {
        parts
    } else {
        document_id = new_document(app, tree_id, None).await;
        let mut all = parts.to_vec();
        all.push(("document_id", None, document_id.as_bytes()));
        owned = all;
        owned.as_slice()
    };
    let (content_type, body) = multipart(parts);
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/trees/{tree_id}/media/upload"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// A raw response, for the endpoints that return bytes rather than JSON.
async fn raw(
    app: &axum::Router,
    uri: &str,
    headers: &[(header::HeaderName, &str)],
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(Method::GET).uri(uri);
    for (name, value) in headers {
        builder = builder.header(name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let response_headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, response_headers, bytes.to_vec())
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]);
    }
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// Exercise the URL capability and its HTTP representation with the same inputs.
async fn download_access(
    h: &Harness,
    tree_id: Uuid,
    id: &str,
    archive: bool,
    expected: StatusCode,
) -> (axum::http::HeaderMap, Vec<u8>) {
    let suffix = if archive { "archive" } else { "download" };
    let url = format!("/api/v1/trees/{tree_id}/media/{id}/{suffix}");
    let (status, headers, bytes) = raw(&h.app, &url, &[]).await;
    assert_eq!(status, expected, "{url}");
    #[cfg(feature = "graphql")]
    {
        let field = if archive {
            "mediaArchive"
        } else {
            "mediaDownload"
        };
        let (_, result) = json_request(
            &h.app,
            Method::POST,
            "/graphql",
            Some(json!({"query": format!(
                "{{ {field}(treeId: \"{tree_id}\", id: \"{id}\") {{ url }} }}"
            )})),
        )
        .await;
        if expected.is_success() {
            assert!(result.get("errors").is_none(), "{result}");
            assert_eq!(result["data"][field]["url"], url, "{result}");
        } else {
            assert!(
                result["errors"]
                    .as_array()
                    .is_some_and(|errors| !errors.is_empty()),
                "{result}"
            );
            assert!(result["data"][field].is_null(), "{result}");
        }
    }
    (headers, bytes)
}

#[tokio::test]
async fn single_media_downloads_are_attachments_while_files_remain_inline() {
    let h = setup().await;
    for (name, content_type, content) in [
        ("scan.png", "image/png", png(12, 10)),
        (
            "document.pdf",
            "application/pdf",
            b"%PDF-1.4\nexample\n".to_vec(),
        ),
    ] {
        let (status, page) = upload(&h.app, h.tree_id, &[("file", Some(name), &content)]).await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        let id = page["id"].as_str().unwrap();
        let (headers, bytes) = download_access(&h, h.tree_id, id, false, StatusCode::OK).await;
        assert_eq!(bytes, content);
        assert_eq!(headers[header::CONTENT_TYPE], content_type);
        assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert!(
            headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("attachment;")
        );
        assert!(
            headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .contains(name)
        );

        let file_url = format!("/api/v1/trees/{}/media/{id}/file", h.tree_id);
        let (status, headers, bytes) = raw(&h.app, &file_url, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(bytes, content);
        assert!(
            headers[header::CONTENT_DISPOSITION]
                .to_str()
                .unwrap()
                .starts_with("inline;")
        );
        let etag = headers[header::ETAG].to_str().unwrap();
        let (status, _, bytes) = raw(&h.app, &file_url, &[(header::IF_NONE_MATCH, etag)]).await;
        assert_eq!(status, StatusCode::NOT_MODIFIED);
        assert!(bytes.is_empty());
        let (status, _, bytes) = raw(
            &h.app,
            &format!("/api/v1/trees/{}/media/{id}/download", h.tree_id),
            &[(header::IF_NONE_MATCH, etag)],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "explicit downloads always transfer the original"
        );
        assert_eq!(bytes, content);
    }
}

#[tokio::test]
async fn document_downloads_are_complete_stored_zips_in_reading_order() {
    use std::io::Read;

    let h = setup().await;
    let doc = document(&h, "../../Example register.pdf").await;
    let mut ids = Vec::new();
    let contents = [png(12, 10), b"%PDF-1.4\nexample\n".to_vec()];
    for content in &contents {
        let (status, page) = upload(
            &h.app,
            h.tree_id,
            &[
                ("file", Some("same.bin"), content),
                ("document_id", None, doc.as_bytes()),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{page}");
        ids.push(page["id"].as_str().unwrap().to_string());
    }
    let (status, _) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{doc}/pages", h.tree_id),
        Some(json!({"page_ids": [ids[1], ids[0]]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (headers, bytes) = download_access(&h, h.tree_id, &doc, true, StatusCode::OK).await;
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(headers[header::CONTENT_LENGTH], bytes.len().to_string());
    let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.starts_with("attachment;"), "{disposition}");
    assert!(
        disposition.contains("Example register.zip"),
        "{disposition}"
    );
    assert!(!disposition.contains("../"), "{disposition}");
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 2);
    for (index, expected) in contents.iter().rev().enumerate() {
        let mut entry = archive.by_index(index).unwrap();
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        assert_eq!(entry.name(), format!("{:03}_same.bin", index + 1));
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(&bytes, expected);
    }
}

#[tokio::test]
async fn media_downloads_reject_missing_foreign_and_deleted_records() {
    let h = setup().await;
    let doc = document(&h, "Example document").await;
    let id = add_page(&h, &doc, "page.png").await;
    let (_, other) = json_request(
        &h.app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({"name": "Other test tree"})),
    )
    .await;
    let other_tree = Uuid::parse_str(other["id"].as_str().unwrap()).unwrap();
    for (archive, target) in [(false, &id), (true, &doc)] {
        download_access(&h, other_tree, target, archive, StatusCode::NOT_FOUND).await;
        download_access(
            &h,
            h.tree_id,
            &Uuid::now_v7().to_string(),
            archive,
            StatusCode::NOT_FOUND,
        )
        .await;
        download_access(
            &h,
            h.tree_id,
            "not-a-uuid",
            archive,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{doc}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    download_access(&h, h.tree_id, &id, false, StatusCode::NOT_FOUND).await;
    download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
async fn media_downloads_never_fetch_remote_pages_or_skip_unheld_pages() {
    let h = setup().await;
    for path in ["media/missing.png", "https://example.invalid/remote.pdf"] {
        let doc = document(&h, "Example document").await;
        download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
        download_access(&h, h.tree_id, &doc, false, StatusCode::NOT_FOUND).await;
        add_page(&h, &doc, "held.png").await;
        let (status, stub) = json_request(
            &h.app,
            Method::POST,
            &format!("/api/v1/trees/{}/media", h.tree_id),
            Some(
                json!({"document_id": doc, "file_name": "missing", "file_path": path,
                "mime_type": "application/octet-stream", "file_size": 0}),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{stub}");
        download_access(
            &h,
            h.tree_id,
            stub["id"].as_str().unwrap(),
            false,
            StatusCode::NOT_FOUND,
        )
        .await;
        download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
    }
}

#[tokio::test]
async fn media_downloads_report_storage_loss_instead_of_returning_partial_archives() {
    let h = setup().await;
    let doc = document(&h, "Example document").await;
    add_page(&h, &doc, "held.png").await;
    let (status, page) = upload(
        &h.app,
        h.tree_id,
        &[
            ("file", Some("lost.pdf"), b"%PDF-1.4\nexample\n"),
            ("document_id", None, doc.as_bytes()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");
    std::fs::remove_file(h.root.0.join(page["storage_key"].as_str().unwrap())).unwrap();
    for (archive, id) in [(false, page["id"].as_str().unwrap()), (true, doc.as_str())] {
        let (headers, _) = download_access(
            &h,
            h.tree_id,
            id,
            archive,
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .await;
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert!(!headers.contains_key(header::CONTENT_DISPOSITION));
    }
}

#[tokio::test]
async fn media_downloads_reject_cross_tree_storage_keys_and_page_rows() {
    use oxidgene_db::entities::media;
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    let h = setup().await;
    let doc = document(&h, "Example document").await;
    let page = add_page(&h, &doc, "page.png").await;
    let (_, other) = json_request(
        &h.app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({"name": "Other test tree"})),
    )
    .await;
    let other_tree = Uuid::parse_str(other["id"].as_str().unwrap()).unwrap();
    let (_, foreign) = upload(
        &h.app,
        other_tree,
        &[("file", Some("other.png"), &png(4, 4))],
    )
    .await;
    let page_id = Uuid::parse_str(&page).unwrap();
    // Simulate corrupt persisted references without weakening production writes.
    media::ActiveModel {
        id: Set(page_id),
        storage_key: Set(Some(foreign["storage_key"].as_str().unwrap().to_string())),
        ..Default::default()
    }
    .update(&h.db)
    .await
    .unwrap();
    download_access(&h, h.tree_id, &page, false, StatusCode::NOT_FOUND).await;
    download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
    let (status, _, _) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{page}/file", h.tree_id),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    media::ActiveModel {
        id: Set(page_id),
        tree_id: Set(other_tree),
        ..Default::default()
    }
    .update(&h.db)
    .await
    .unwrap();
    download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
}

#[tokio::test]
async fn media_downloads_reject_soft_deleted_trees_and_parent_documents_before_purge() {
    use oxidgene_db::entities::{media, tree};
    use sea_orm::{ActiveModelTrait, ActiveValue::Set};

    for delete_tree in [false, true] {
        let h = setup().await;
        let doc = document(&h, "Example document").await;
        let page = add_page(&h, &doc, "page.png").await;
        // Keep the bytes and child rows in place to test the pre-purge window.
        if delete_tree {
            tree::ActiveModel {
                id: Set(h.tree_id),
                deleted_at: Set(Some(chrono::Utc::now())),
                ..Default::default()
            }
            .update(&h.db)
            .await
            .unwrap();
        } else {
            media::ActiveModel {
                id: Set(Uuid::parse_str(&doc).unwrap()),
                deleted_at: Set(Some(chrono::Utc::now())),
                ..Default::default()
            }
            .update(&h.db)
            .await
            .unwrap();
        }
        download_access(&h, h.tree_id, &page, false, StatusCode::NOT_FOUND).await;
        download_access(&h, h.tree_id, &doc, true, StatusCode::NOT_FOUND).await;
        let (status, _, _) = raw(
            &h.app,
            &format!("/api/v1/trees/{}/media/{page}/file", h.tree_id),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

// ── Upload ──────────────────────────────────────────────────────────

#[tokio::test]
async fn uploading_a_photo_creates_a_record_that_knows_its_own_shape() {
    let h = setup().await;
    let (status, media) = upload(
        &h.app,
        h.tree_id,
        &[
            ("file", Some("portrait.png"), &png(1200, 900)),
            ("title", None, b"Grandparents"),
            ("description", None, b"Taken in the garden"),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{media}");
    assert_eq!(media["file_name"], "portrait.png");
    assert_eq!(media["mime_type"], "image/png");
    assert_eq!(media["width"], 1200);
    assert_eq!(media["height"], 900);
    assert_eq!(media["page_count"], 1);
    assert_eq!(media["title"], "Grandparents");
    assert_eq!(media["description"], "Taken in the garden");
    assert_eq!(media["sha256"].as_str().unwrap().len(), 64);
    assert!(media["storage_key"].is_string());
    assert!(media["thumbnail_key"].is_string());
}

#[tokio::test]
async fn the_uploaded_bytes_actually_land_under_the_media_root() {
    let h = setup().await;
    let content = png(64, 64);
    let (_, media) = upload(&h.app, h.tree_id, &[("file", Some("photo.png"), &content)]).await;

    let key = media["storage_key"].as_str().unwrap();
    let path = h.root.0.join(key);
    assert!(path.exists(), "nothing at {}", path.display());
    assert_eq!(std::fs::read(&path).unwrap(), content);
    assert!(
        key.starts_with(&h.tree_id.to_string()),
        "keys are scoped per tree: {key}"
    );
}

#[tokio::test]
async fn a_file_type_we_do_not_accept_is_refused() {
    let h = setup().await;
    let (status, body) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("payload.png"), b"\x7fELF\x02\x01\x01\x00")],
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert!(
        !h.root.0.join(h.tree_id.to_string()).exists(),
        "a refused upload should leave nothing behind"
    );
}

#[tokio::test]
async fn a_form_without_a_file_part_is_refused() {
    let h = setup().await;
    let (status, body) = upload(&h.app, h.tree_id, &[("title", None, b"just a title")]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}

#[tokio::test]
async fn the_same_photo_uploaded_twice_is_two_records_over_one_file() {
    let h = setup().await;
    let content = png(200, 200);

    let (_, first) = upload(&h.app, h.tree_id, &[("file", Some("census.png"), &content)]).await;
    let (_, second) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("census-again.png"), &content)],
    )
    .await;

    assert_ne!(first["id"], second["id"], "each upload is its own record");
    assert_eq!(
        first["storage_key"], second["storage_key"],
        "identical bytes should share one file"
    );
    assert_eq!(second["file_name"], "census-again.png");
}

#[tokio::test]
async fn bytes_can_be_attached_to_a_record_that_had_none() {
    let h = setup().await;

    // The state a GEDCOM import leaves behind: a name, a path, no file.
    let doc = new_document(&h.app, h.tree_id, None).await;
    let (status, stub) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": doc,
            "file_name": "grandpere.jpg",
            "mime_type": "image/jpeg",
            "file_path": "D:\\Photos\\grandpere.jpg",
            "file_size": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{stub}");
    assert!(stub["storage_key"].is_null());

    let media_id = stub["id"].as_str().unwrap().to_string();
    let (status, filled) = upload(
        &h.app,
        h.tree_id,
        &[
            ("file", Some("grandpere.png"), &png(300, 400)),
            ("media_id", None, media_id.as_bytes()),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::OK, "attaching updates, not creates");
    assert_eq!(filled["id"], media_id.as_str());
    assert!(filled["storage_key"].is_string());
    assert_eq!(filled["height"], 400);
    assert_eq!(
        filled["file_path"], "D:\\Photos\\grandpere.jpg",
        "the GEDCOM path is what export round-trips, so it survives"
    );
}

// ── Serving ─────────────────────────────────────────────────────────

#[tokio::test]
async fn a_stored_file_is_served_back_byte_for_byte() {
    let h = setup().await;
    let content = png(500, 250);
    let (_, media) = upload(&h.app, h.tree_id, &[("file", Some("acte.png"), &content)]).await;
    let id = media["id"].as_str().unwrap();

    let (status, headers, bytes) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/file", h.tree_id),
        &[],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, content);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        headers[header::ETAG],
        format!("\"{}\"", media["sha256"].as_str().unwrap()).as_str()
    );
    assert!(
        headers[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("acte.png")
    );
}

#[tokio::test]
async fn a_client_that_already_has_the_file_gets_a_304() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("photo.png"), &png(120, 120))],
    )
    .await;
    let id = media["id"].as_str().unwrap();
    let etag = format!("\"{}\"", media["sha256"].as_str().unwrap());

    let (status, _, bytes) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/file", h.tree_id),
        &[(header::IF_NONE_MATCH, &etag)],
    )
    .await;

    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(bytes.is_empty(), "a 304 carries no body");
}

#[tokio::test]
async fn a_stale_etag_still_gets_the_file() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("photo.png"), &png(120, 120))],
    )
    .await;
    let id = media["id"].as_str().unwrap();

    let (status, _, bytes) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/file", h.tree_id),
        &[(header::IF_NONE_MATCH, "\"an-etag-from-a-different-file\"")],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn the_thumbnail_is_a_smaller_decodable_image() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("large.png"), &png(2000, 1000))],
    )
    .await;
    let id = media["id"].as_str().unwrap();

    let (status, headers, bytes) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/thumbnail", h.tree_id),
        &[],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    let thumb = image::load_from_memory(&bytes).expect("thumbnail decodes");
    assert!(thumb.width() <= 400 && thumb.height() <= 400, "not scaled");
    assert!(bytes.len() < png(2000, 1000).len(), "not actually smaller");
}

#[tokio::test]
async fn a_pdf_has_no_thumbnail_to_serve() {
    let h = setup().await;
    let (status, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("acte.pdf"), b"%PDF-1.4\nnot a real document\n")],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{media}");
    assert!(media["thumbnail_key"].is_null());
    let id = media["id"].as_str().unwrap();

    let (status, _, _) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/thumbnail", h.tree_id),
        &[],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the gallery falls back to an icon on this status alone"
    );
}

#[tokio::test]
async fn a_record_with_no_bytes_has_no_file_to_serve() {
    let h = setup().await;
    let (_, stub) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": new_document(&h.app, h.tree_id, None).await,
            "file_name": "missing.jpg",
            "mime_type": "image/jpeg",
            "file_path": "media/missing.jpg",
            "file_size": 0
        })),
    )
    .await;
    let id = stub["id"].as_str().unwrap();

    let (status, _, _) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{id}/file", h.tree_id),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ── Vignettes ───────────────────────────────────────────────────────

/// The document a just-uploaded page belongs to.
///
/// A gallery lists documents and a link points at one; the page is where the
/// bytes and the crops live. Tests that attach a media to somebody want this,
/// tests that assert on the file itself want the page.
fn document_of(page: &Value) -> String {
    page["parent_media_id"]
        .as_str()
        .expect("an uploaded page belongs to a document")
        .to_string()
}

/// Upload a scan and return its id, for the vignette tests.
async fn scan(h: &Harness, width: u32, height: u32) -> String {
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("register.png"), &png(width, height))],
    )
    .await;
    media["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn several_entries_on_one_page_are_several_vignettes_over_one_scan() {
    let h = setup().await;
    let media_id = scan(&h, 1000, 800).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    for (index, y) in [0, 200, 400].iter().enumerate() {
        let (status, vignette) = json_request(
            &h.app,
            Method::POST,
            &format!("{base}/media/{media_id}/vignettes"),
            Some(json!({"x": 0, "y": y, "width": 1000, "height": 200,
                        "title": format!("entry {index}")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{vignette}");
    }

    let (status, listed) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/media/{media_id}/vignettes"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_crop_larger_than_its_scan_is_refused() {
    let h = setup().await;
    let media_id = scan(&h, 400, 300).await;

    let (status, body) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{media_id}/vignettes", h.tree_id),
        Some(json!({"x": 300, "y": 0, "width": 200, "height": 100})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "The request is invalid");
    assert!(body.get("request_id").is_none());
}

#[tokio::test]
async fn a_vignette_serves_the_cropped_region_as_its_own_image() {
    let h = setup().await;
    let media_id = scan(&h, 800, 600).await;

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{media_id}/vignettes", h.tree_id),
        Some(json!({"x": 100, "y": 50, "width": 320, "height": 240})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, headers, bytes) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/vignettes/{id}/image", h.tree_id),
        &[],
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    let cropped = image::load_from_memory(&bytes).expect("crop decodes");
    assert_eq!((cropped.width(), cropped.height()), (320, 240));
}

#[tokio::test]
async fn moving_a_vignette_re_checks_it_against_the_scan() {
    let h = setup().await;
    let media_id = scan(&h, 500, 500).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 100, "height": 100})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, moved) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/vignettes/{id}"),
        Some(json!({"x": 400, "y": 400, "width": 100, "height": 100})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["x"], 400);

    let (status, _) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/vignettes/{id}"),
        Some(json!({"x": 450, "y": 450, "width": 100, "height": 100})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "that one hangs off the edge"
    );
}

#[tokio::test]
async fn half_a_rectangle_is_not_a_move() {
    let h = setup().await;
    let media_id = scan(&h, 500, 500).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 100, "height": 100})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/vignettes/{id}"),
        Some(json!({"width": 200})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn vignettes_can_be_listed_by_who_they_show() {
    let h = setup().await;
    let media_id = scan(&h, 600, 600).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, person) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/persons"),
        Some(json!({"sex": "male"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{person}");
    let person_id = person["id"].as_str().unwrap().to_string();

    json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 100, "height": 100, "person_id": person_id})),
    )
    .await;
    json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 200, "y": 200, "width": 100, "height": 100})),
    )
    .await;

    let (status, listed) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/vignettes?person_id={person_id}"),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "the unattributed one should not be listed: {listed}"
    );
}

#[tokio::test]
async fn listing_vignettes_without_a_filter_is_refused() {
    let h = setup().await;
    let (status, body) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/vignettes", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn deleting_a_vignette_leaves_the_scan_intact() {
    let h = setup().await;
    let media_id = scan(&h, 400, 400).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 100, "height": 100})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("{base}/vignettes/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = raw(&h.app, &format!("{base}/media/{media_id}/file"), &[]).await;
    assert_eq!(status, StatusCode::OK, "the scan is untouched");
}

#[tokio::test]
async fn a_pdf_cannot_be_cropped() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("acte.pdf"), b"%PDF-1.4\nnot a real document\n")],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 100, "height": 100})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, _, _) = raw(&h.app, &format!("{base}/vignettes/{id}/image"), &[]).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "rasterising a PDF needs a renderer we do not ship"
    );
}

// ── Tree purge ──────────────────────────────────────────────────────

#[tokio::test]
async fn deleting_an_isolated_media_removes_its_record_and_files() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("isolated.png"), &png(300, 300))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();
    let storage_key = media["storage_key"].as_str().unwrap();
    let thumbnail_key = media["thumbnail_key"].as_str().unwrap();
    assert!(h.root.0.join(storage_key).exists());
    assert!(h.root.0.join(thumbnail_key).exists());

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(!h.root.0.join(storage_key).exists());
    assert!(!h.root.0.join(thumbnail_key).exists());
    let (status, _) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_tree_takes_its_media_files_with_it() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("photo.png"), &png(100, 100))],
    )
    .await;
    let key = media["storage_key"].as_str().unwrap().to_string();
    assert!(h.root.0.join(&key).exists());

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Purging is a background worker, so give it a moment to run.
    for _ in 0..50 {
        if !h.root.0.join(&key).exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    assert!(
        !h.root.0.join(&key).exists(),
        "a purged tree should not leave its scans on disk"
    );
}

// ── Gallery listing & profile photo (Sprint F.2) ────────────────────

/// Create a person and return its id.
async fn person(h: &Harness) -> String {
    let (status, person) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/persons", h.tree_id),
        Some(json!({"sex": "female"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{person}");
    person["id"].as_str().unwrap().to_string()
}

/// Upload a photo and attach it to `person_id`.
///
/// Returns (document id, page id, link id). The link names the document — that
/// is what a gallery lists — while a portrait names the page, because a
/// portrait is pixels and a document has none.
async fn attach_photo(h: &Harness, person_id: &str, name: &str) -> (String, String, String) {
    let (_, media) = upload(&h.app, h.tree_id, &[("file", Some(name), &png(240, 180))]).await;
    let media_id = document_of(&media);
    let page_id = media["id"].as_str().unwrap().to_string();

    let (status, link) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media-links", h.tree_id),
        Some(json!({"media_id": media_id, "person_id": person_id, "sort_order": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{link}");
    (media_id, page_id, link["id"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn one_request_returns_a_person_gallery_with_everything_a_tile_needs() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _page, link_id) = attach_photo(&h, &person_id, "portrait.png").await;

    let (status, listed) = json_request(
        &h.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/media-links?entity_type=person&entity_id={person_id}",
            h.tree_id
        ),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let rows = listed.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{listed}");
    let row = &rows[0];
    assert_eq!(row["link_id"], link_id.as_str());
    assert_eq!(row["id"], media_id.as_str(), "the media is flattened in");
    // A tile shows a document, so what it lists is the container: its type
    // says "document", and its one page is what supplies the picture.
    assert_eq!(row["mime_type"], oxidgene_core::types::DOCUMENT_MIME);
    assert!(row["parent_media_id"].is_null());
    assert_eq!(row["page_count"], 1);
}

#[tokio::test]
async fn a_gallery_does_not_show_another_persons_photos() {
    let h = setup().await;
    let a = person(&h).await;
    let b = person(&h).await;
    attach_photo(&h, &a, "a.png").await;

    let (_, listed) = json_request(
        &h.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/media-links?entity_type=person&entity_id={b}",
            h.tree_id
        ),
        None,
    )
    .await;
    assert!(listed.as_array().unwrap().is_empty(), "{listed}");
}

#[tokio::test]
async fn a_soft_deleted_media_leaves_the_gallery() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _page, _) = attach_photo(&h, &person_id, "photo.png").await;

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, listed) = json_request(
        &h.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/media-links?entity_type=person&entity_id={person_id}",
            h.tree_id
        ),
        None,
    )
    .await;
    assert!(
        listed.as_array().unwrap().is_empty(),
        "the link survives the soft delete; the tile must not: {listed}"
    );
}

#[tokio::test]
async fn choosing_a_portrait_replaces_the_previous_one() {
    let h = setup().await;
    let person_id = person(&h).await;
    // Chosen the way the gallery chooses: by the tile, which is a document.
    let (first_media, _page, _) = attach_photo(&h, &person_id, "first.png").await;
    let (second_media, second_page, _) = attach_photo(&h, &person_id, "second.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    for media in [&first_media, &second_media] {
        let (status, body) = json_request(
            &h.app,
            Method::PUT,
            &format!("{base}/persons/{person_id}/portrait"),
            Some(json!({"media_id": media})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }

    // One column, so "at most one portrait" needs no clearing pass and cannot
    // be left half-done by a failure between two statements.
    let (_, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    let rows = portraits.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{portraits}");
    assert_eq!(rows[0]["person_id"], person_id.as_str());
    // What comes back is where the pixels are: a document holds none, so
    // reporting it would hand a card an id whose file does not exist.
    assert_eq!(rows[0]["media_id"], second_page.as_str(), "{portraits}");
}

#[tokio::test]
async fn a_portrait_can_be_a_face_in_a_group_photograph() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_document_id, media_id, _) = attach_photo(&h, &person_id, "wedding.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    // The portrait most people in an old family archive actually have: a
    // region of a larger scan, stored as coordinates rather than as a copy.
    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 10, "y": 10, "width": 30, "height": 30, "person_id": person_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vignette}");

    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    let row = &portraits.as_array().unwrap()[0];
    assert_eq!(row["vignette_id"], vignette["id"]);
    assert!(row["media_id"].is_null(), "one or the other, never both");
    // The crop resolves through the scan it is on, so a caller knows there
    // are rasterised bytes behind it without asking twice.
    assert_eq!(row["has_thumbnail"], true);
}

#[tokio::test]
async fn portrait_images_are_loaded_and_filtered_in_one_request() {
    let h = setup().await;
    let thumbnail_person = person(&h).await;
    let vignette_person = person(&h).await;
    let unselected_person = person(&h).await;
    let (_thumbnail_document, thumbnail_media_id, _) =
        attach_photo(&h, &thumbnail_person, "thumbnail.png").await;
    attach_photo(&h, &unselected_person, "unselected.png").await;
    let (_group_document, media_id, _) = attach_photo(&h, &vignette_person, "group.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({
            "x": 10,
            "y": 10,
            "width": 30,
            "height": 30,
            "person_id": vignette_person
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vignette}");
    let (status, selected) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{vignette_person}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{selected}");

    let (status, images) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/portrait-images"),
        Some(json!({"person_ids": [thumbnail_person, vignette_person]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{images}");
    let images = images.as_array().unwrap();
    assert_eq!(images.len(), 2, "{images:?}");
    assert!(images.iter().all(|image| {
        image["source"]
            .as_str()
            .is_some_and(|source| source.starts_with("data:image/"))
    }));
    assert!(
        images
            .iter()
            .all(|image| image["person_id"] != unselected_person)
    );

    let (status, bundle) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/gallery-bundle"),
        Some(json!({
            "media_ids": [thumbnail_media_id, media_id],
            "vignette_ids": [vignette["id"]]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["media"].as_array().unwrap().len(), 2, "{bundle}");
    assert_eq!(bundle["vignettes"].as_array().unwrap().len(), 1, "{bundle}");
    assert!(bundle["media"].as_array().unwrap().iter().all(|item| {
        item["source"]
            .as_str()
            .is_some_and(|source| source.starts_with("data:image/"))
    }));
    assert!(
        bundle["vignettes"][0]["source"]
            .as_str()
            .unwrap()
            .starts_with("data:image/")
    );
}

#[tokio::test]
async fn a_portrait_can_be_cleared() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_document_id, page_id, _) = attach_photo(&h, &person_id, "photo.png").await;
    let media_id = page_id;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"media_id": media_id})),
    )
    .await;
    // Sending neither id is how "use the silhouette again" is said.
    let (status, cleared) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{cleared}");
    assert!(cleared["portrait_media_id"].is_null());

    // Cleared means "no explicit choice", not "draw nothing": the fallback
    // takes over, exactly as it does for a person who never chose.
    let (_, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    let rows = portraits.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{portraits}");
    assert_eq!(rows[0]["media_id"], media_id.as_str());
}

#[tokio::test]
async fn a_portrait_is_a_media_or_a_crop_but_never_both() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_, media_id, _) = attach_photo(&h, &person_id, "photo.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 20, "height": 20})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vignette}");
    assert_eq!(vignette["media_id"], media_id);

    let (status, chosen) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chosen}");

    // Refused rather than resolved: the model holds one answer, and silently
    // picking one of two would make the stored portrait differ from the one
    // that was asked for.
    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"media_id": media_id, "vignette_id": vignette["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, unchanged) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/persons/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{unchanged}");
    assert_eq!(unchanged["portrait_vignette_id"], vignette["id"]);
    assert!(unchanged["portrait_media_id"].is_null());
}

#[tokio::test]
async fn an_unknown_entity_type_is_refused() {
    let h = setup().await;
    let (status, body) = json_request(
        &h.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/media-links?entity_type=elephant&entity_id={}",
            h.tree_id, h.tree_id
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn the_unfiltered_list_is_still_the_tree_wide_one() {
    let h = setup().await;
    let person_id = person(&h).await;
    attach_photo(&h, &person_id, "photo.png").await;

    let (status, listed) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media-links", h.tree_id),
        None,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let rows = listed.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["entity_type"], "person",
        "the pedigree canvas reads this shape: {listed}"
    );
}

#[tokio::test]
async fn detaching_a_media_leaves_the_file_alone() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_document_id, media_id, link_id) = attach_photo(&h, &person_id, "shared.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("{base}/media-links/{link_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = raw(&h.app, &format!("{base}/media/{media_id}/file"), &[]).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the file may document three other people"
    );
}

// ── Multi-page documents (Sprint F.3) ───────────────────────────────

/// Create a document and return its id.
async fn document(h: &Harness, title: &str) -> String {
    let (status, doc) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/document", h.tree_id),
        Some(json!({ "title": title })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{doc}");
    assert_eq!(
        doc["parent_media_id"],
        Value::Null,
        "a document has no parent"
    );
    assert_eq!(doc["page_count"], 0, "a new document has no pages yet");
    doc["id"].as_str().unwrap().to_string()
}

/// Upload an image as the next page of `document_id`.
async fn add_page(h: &Harness, document_id: &str, name: &str) -> String {
    let (status, page) = upload(
        &h.app,
        h.tree_id,
        &[
            ("file", Some(name), &png(300, 400)),
            ("document_id", None, document_id.as_bytes()),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");
    page["id"].as_str().unwrap().to_string()
}

async fn pages_of(h: &Harness, document_id: &str) -> Vec<Value> {
    let (status, pages) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media/{document_id}/pages", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    pages.as_array().unwrap().clone()
}

#[tokio::test]
async fn pages_arrive_in_upload_order_and_count_themselves() {
    let h = setup().await;
    let doc = document(&h, "Parish register 1872").await;
    for name in ["p1.png", "p2.png", "p3.png"] {
        add_page(&h, &doc, name).await;
    }

    let pages = pages_of(&h, &doc).await;
    assert_eq!(pages.len(), 3);
    let names: Vec<&str> = pages
        .iter()
        .map(|p| p["file_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["p1.png", "p2.png", "p3.png"]);
    let indexes: Vec<i64> = pages
        .iter()
        .map(|p| p["page_index"].as_i64().unwrap())
        .collect();
    assert_eq!(indexes, [0, 1, 2]);

    let (_, doc_row) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media/{doc}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(
        doc_row["page_count"], 3,
        "the count is derived, not guessed"
    );
}

#[tokio::test]
async fn a_gallery_shows_the_document_not_its_pages() {
    let h = setup().await;
    let person_id = person(&h).await;
    let doc = document(&h, "Notarial act").await;
    add_page(&h, &doc, "a.png").await;
    add_page(&h, &doc, "b.png").await;

    json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media-links", h.tree_id),
        Some(json!({"media_id": doc, "person_id": person_id, "sort_order": 0})),
    )
    .await;

    let (_, listed) = json_request(
        &h.app,
        Method::GET,
        &format!(
            "/api/v1/trees/{}/media-links?entity_type=person&entity_id={person_id}",
            h.tree_id
        ),
        None,
    )
    .await;
    assert_eq!(
        listed.as_array().unwrap().len(),
        1,
        "a two-page act is one tile, not three: {listed}"
    );

    // The tree-wide media list must not show the pages loose either.
    let (_, all) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        None,
    )
    .await;
    assert_eq!(all["edges"].as_array().unwrap().len(), 1, "{all}");
}

#[tokio::test]
async fn pages_can_be_reordered() {
    let h = setup().await;
    let doc = document(&h, "Register").await;
    let first = add_page(&h, &doc, "first.png").await;
    let second = add_page(&h, &doc, "second.png").await;

    let (status, reordered) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{doc}/pages", h.tree_id),
        Some(json!({ "page_ids": [second, first] })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{reordered}");
    let names: Vec<&str> = reordered
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["file_name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["second.png", "first.png"]);
}

#[tokio::test]
async fn a_partial_page_order_is_refused() {
    let h = setup().await;
    let doc = document(&h, "Register").await;
    let first = add_page(&h, &doc, "a.png").await;
    add_page(&h, &doc, "b.png").await;

    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{doc}/pages", h.tree_id),
        Some(json!({ "page_ids": [first] })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "half an order would silently drop a page: {body}"
    );
}

#[tokio::test]
async fn removing_a_page_destroys_it_and_closes_the_gap() {
    let h = setup().await;
    let person_id = person(&h).await;
    let doc = document(&h, "Register").await;
    add_page(&h, &doc, "a.png").await;
    let middle = add_page(&h, &doc, "b.png").await;
    add_page(&h, &doc, "c.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);
    json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media-links"),
        Some(json!({"media_id": middle, "person_id": person_id, "sort_order": 0})),
    )
    .await;
    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{middle}/vignettes"),
        Some(json!({"x": 10, "y": 10, "width": 50, "height": 60, "person_id": person_id})),
    )
    .await;
    json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;

    let (status, removed) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{doc}/pages/{middle}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{removed}");

    let pages = pages_of(&h, &doc).await;
    let indexes: Vec<i64> = pages
        .iter()
        .map(|p| p["page_index"].as_i64().unwrap())
        .collect();
    assert_eq!(indexes, [0, 1], "page 3 of a 2-page document is not a page");

    // The page is gone, bytes included: a page belongs to a document, and a
    // page belonging to nothing is not a shape this model holds.
    let (status, _, _) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{middle}/file", h.tree_id),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, links) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/media-links?person_id={person_id}"),
        None,
    )
    .await;
    assert!(
        links.as_array().unwrap().is_empty(),
        "the removed page took its links with it: {links}"
    );
    let (_, vignettes) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/vignettes?person_id={person_id}"),
        None,
    )
    .await;
    assert!(vignettes.as_array().unwrap().is_empty(), "{vignettes}");
    let (_, person) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/persons/{person_id}"),
        None,
    )
    .await;
    assert!(person["portrait_media_id"].is_null(), "{person}");
    assert!(person["portrait_vignette_id"].is_null(), "{person}");
}

#[tokio::test]
async fn deleting_a_simple_media_removes_all_attachments_and_identifications() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _page, _) = attach_photo(&h, &person_id, "portrait.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);
    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 10, "y": 10, "width": 50, "height": 60, "person_id": person_id})),
    )
    .await;
    json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("{base}/media/{media_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, gallery) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/media-links?entity_type=person&entity_id={person_id}"),
        None,
    )
    .await;
    assert!(gallery.as_array().unwrap().is_empty(), "{gallery}");
    let (_, vignettes) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/vignettes?person_id={person_id}"),
        None,
    )
    .await;
    assert!(vignettes.as_array().unwrap().is_empty(), "{vignettes}");
}

// ── Media fields: date, place, URL, note ────────────────────────────

#[tokio::test]
async fn a_media_carries_a_date_with_its_qualifier_and_calendar() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("portrait.png"), &png(200, 200))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();

    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        Some(json!({
            "date_value": "1890",
            "date_qualifier": "about",
            "calendar": "gregorian",
            "description": "In the garden"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["date_value"], "1890");
    assert_eq!(updated["date_qualifier"], "about");
    assert_eq!(
        updated["date_sort"], "1890-01-01",
        "the server derives the sort key; the client never sends it"
    );
}

#[tokio::test]
async fn media_tags_are_added_and_removed_independently() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("archive.png"), &png(200, 200))],
    )
    .await;
    let media_id = document_of(&media);

    let (status, updated) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{media_id}/tags", h.tree_id),
        Some(json!({ "tag": " archives " })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["tags"], json!(["archives"]));

    let (status, updated) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{media_id}/tags", h.tree_id),
        Some(json!({ "tag": "Civil record" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["tags"], json!(["archives", "Civil record"]));

    let (status, updated) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{media_id}/tags", h.tree_id),
        Some(json!({ "tag": "ARCHIVES" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["tags"], json!(["archives", "Civil record"]));

    let (status, _) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{media_id}/tags", h.tree_id),
        Some(json!({ "tag": "ARCHIVES" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, updated) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["tags"], json!(["Civil record"]));
}

#[tokio::test]
async fn a_record_with_no_bytes_can_be_repointed_at_a_url() {
    let h = setup().await;
    let doc = new_document(&h.app, h.tree_id, None).await;
    let (_, stub) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": doc,
            "file_name": "unknown.jpg",
            "mime_type": "application/octet-stream",
            "file_path": "media/unknown.jpg",
            "file_size": 0
        })),
    )
    .await;
    let media_id = stub["id"].as_str().unwrap();

    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        Some(json!({ "file_path": "https://archives.example.org/scan/42.jpg" })),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(
        updated["file_path"],
        "https://archives.example.org/scan/42.jpg"
    );
    assert_eq!(
        updated["mime_type"], "image/jpeg",
        "guessed from the URL, since we never fetch it"
    );
    assert_eq!(
        updated["file_name"], "42.jpg",
        "the caption follows the path when the path is all we have"
    );
}

#[tokio::test]
async fn a_document_tile_previews_a_page_we_only_have_a_url_for() {
    // We never fetch a remote file, so there is no thumbnail to send — but the
    // browser can draw it perfectly well from its own URL. Sending nothing
    // leaves an imported photograph showing a file icon in every gallery.
    let h = setup().await;
    let document = new_document(&h.app, h.tree_id, None).await;
    let url = "https://archives.example.invalid/scan/42.jpg";
    let (status, page) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": document,
            "file_name": "42.jpg",
            "mime_type": "image/jpeg",
            "file_path": url,
            "file_size": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");

    let (status, bundle) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/gallery-bundle", h.tree_id),
        Some(json!({"media_ids": [document], "vignette_ids": []})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(
        bundle["media"][0]["document_previews"],
        json!([url]),
        "the page's own address is what the tile draws: {bundle}"
    );
    assert!(
        bundle["media"][0]["source"].is_null(),
        "a document holds no bytes of its own: {bundle}"
    );
}

/// A remote page carrying a region of somebody, ready to be read back.
///
/// Returns `(page_id, vignette_id, person_id)`. `measured` is what a browser
/// reported the picture's size to be — `None` leaves the page unmeasured, which
/// is a page whose regions cannot be placed.
async fn remote_identification(
    h: &Harness,
    url: &str,
    measured: Option<(i64, i64)>,
) -> (String, String, String) {
    let document = new_document(&h.app, h.tree_id, None).await;
    let (_, page) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": document,
            "file_name": "7.jpg",
            "mime_type": "image/jpeg",
            "file_path": url,
            "file_size": 0
        })),
    )
    .await;
    let page_id = page["id"].as_str().unwrap().to_string();
    if let Some((width, height)) = measured {
        let (status, sized) = json_request(
            &h.app,
            Method::PUT,
            &format!("/api/v1/trees/{}/media/{page_id}", h.tree_id),
            Some(json!({"width": width, "height": height})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{sized}");
    }
    let person_id = person(h).await;
    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media/{page_id}/vignettes", h.tree_id),
        Some(json!({"x": 120, "y": 40, "width": 200, "height": 260, "person_id": person_id})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a region may be drawn on a page we do not hold: {vignette}"
    );
    (
        page_id,
        vignette["id"].as_str().unwrap().to_string(),
        person_id,
    )
}

#[tokio::test]
async fn a_region_of_a_remote_page_travels_as_the_picture_and_the_rectangle() {
    // Somebody can be identified on a photograph we do not hold: the region and
    // its attribution are ours. Cutting it is not — that means re-decoding our
    // own copy — so the whole picture goes out with the rectangle to take from
    // it, and the client does the cutting.
    let h = setup().await;
    let url = "https://archives.example.invalid/group/7.jpg";
    let (_, vignette_id, person_id) = remote_identification(&h, url, Some((1600, 1200))).await;

    let (status, bundle) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/gallery-bundle", h.tree_id),
        Some(json!({"media_ids": [], "vignette_ids": [vignette_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["vignettes"][0]["source"], url, "{bundle}");
    assert_eq!(
        bundle["vignettes"][0]["crop"],
        json!({"x": 120, "y": 40, "width": 200, "height": 260,
               "source_width": 1600, "source_height": 1200}),
        "{bundle}"
    );

    // The same region as somebody's portrait reaches every card the same way.
    let (status, portrait) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/persons/{person_id}/portrait", h.tree_id),
        Some(json!({"vignette_id": vignette_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{portrait}");
    let (status, images) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/portrait-images", h.tree_id),
        Some(json!({"person_ids": [person_id]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{images}");
    assert_eq!(images[0]["source"], url, "{images}");
    assert_eq!(images[0]["crop"]["source_width"], 1600, "{images}");
    assert_eq!(images[0]["crop"]["width"], 200, "{images}");
}

#[tokio::test]
async fn a_region_of_a_picture_nobody_measured_falls_back_to_the_whole_of_it() {
    // Without the size the rectangle was measured against there is no scale to
    // cut at. The whole picture is honest; a guess would put the box somewhere
    // it never was.
    let h = setup().await;
    let url = "https://archives.example.invalid/group/8.jpg";
    let (_, vignette_id, _) = remote_identification(&h, url, None).await;

    let (status, bundle) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/gallery-bundle", h.tree_id),
        Some(json!({"media_ids": [], "vignette_ids": [vignette_id]})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(bundle["vignettes"][0]["source"], url, "{bundle}");
    assert!(bundle["vignettes"][0]["crop"].is_null(), "{bundle}");
}

#[tokio::test]
async fn only_a_page_we_do_not_hold_is_told_how_big_it_is() {
    // Our own copy was decoded on upload; a caller's claim about its size is a
    // second answer that can only disagree with the first.
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("ours.png"), &png(80, 80))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/media/{media_id}"),
        Some(json!({"width": 1600, "height": 1200})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // Half a size is not a size, whoever sends it.
    let document = new_document(&h.app, h.tree_id, None).await;
    let (_, page) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media"),
        Some(json!({
            "document_id": document,
            "file_name": "9.jpg",
            "mime_type": "image/jpeg",
            "file_path": "https://archives.example.invalid/9.jpg",
            "file_size": 0
        })),
    )
    .await;
    let page_id = page["id"].as_str().unwrap();
    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/media/{page_id}"),
        Some(json!({"width": 1600})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn a_remote_page_that_is_not_a_picture_is_not_previewed() {
    // An `<img>` pointed at a PDF draws the broken-image glyph, which is worse
    // than the labelled icon the tile falls back to.
    let h = setup().await;
    let document = new_document(&h.app, h.tree_id, None).await;
    let (status, page) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
            "document_id": document,
            "file_name": "register.pdf",
            "mime_type": "application/pdf",
            "file_path": "https://archives.example.invalid/register.pdf",
            "file_size": 0
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{page}");

    let (status, bundle) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/gallery-bundle", h.tree_id),
        Some(json!({"media_ids": [document], "vignette_ids": []})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{bundle}");
    assert_eq!(
        bundle["media"][0]["document_previews"],
        json!([]),
        "{bundle}"
    );
}

#[tokio::test]
async fn a_stored_media_cannot_be_repointed_elsewhere() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("ours.png"), &png(80, 80))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();

    let (status, body) = json_request(
        &h.app,
        Method::PUT,
        &format!("/api/v1/trees/{}/media/{media_id}", h.tree_id),
        Some(json!({ "file_path": "https://example.org/other.jpg" })),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the GEDCOM export would then describe a file we are not serving: {body}"
    );
}

#[tokio::test]
async fn a_note_can_be_about_a_document_rather_than_a_person() {
    let h = setup().await;
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("scan.png"), &png(100, 100))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, note) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/notes"),
        Some(json!({
            "text": "The left-hand column is water-damaged.",
            "media_id": media_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{note}");

    let (_, listed) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/notes?media_id={media_id}"),
        None,
    )
    .await;
    assert_eq!(listed["edges"].as_array().unwrap().len(), 1, "{listed}");
}

#[tokio::test]
async fn a_crop_portrait_reaches_the_read_projection() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_, media_id, _) = attach_photo(&h, &person_id, "wedding.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({
            "x": 10, "y": 10, "width": 30, "height": 30,
            "person_id": person_id
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vignette}");
    assert_eq!(vignette["media_id"], media_id);
    let (status, chosen) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{chosen}");

    let (status, profile) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/profiles/{person_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{profile}");

    // The crop's own id travels, so a card asks for the cropped image rather
    // than the whole wedding party; the media id is the scan it sits on.
    let portrait = &profile["primary_media"];
    assert_eq!(portrait["vignette_id"], vignette["id"], "{profile}");
    assert_eq!(portrait["media_id"], media_id.as_str());
    assert!(portrait["title"].is_null());
}

#[tokio::test]
async fn the_projection_draws_the_portrait_that_was_chosen() {
    let h = setup().await;
    let person_id = person(&h).await;
    // Attached first, so it holds the lowest sort_order. The projection used
    // to take that one and ignore the stored choice entirely, so a person
    // could star a photograph and have their card go on drawing another.
    let (first, first_page, _) = attach_photo(&h, &person_id, "first.png").await;
    let (second, second_page, _) = attach_photo(&h, &person_id, "second.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"media_id": second})),
    )
    .await;

    let (_, profile) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/profiles/{person_id}"),
        None,
    )
    .await;
    // The chosen document, resolved to the page that holds its pixels — the
    // same rule the portrait query applies, so a card and an avatar cannot
    // disagree about the same person.
    assert_eq!(profile["primary_media"]["media_id"], second_page.as_str());
    assert_ne!(profile["primary_media"]["media_id"], first.as_str());
    assert_ne!(profile["primary_media"]["media_id"], first_page.as_str());
}

#[tokio::test]
async fn a_couple_and_a_document_each_carry_their_own_privacy() {
    let h = setup().await;
    let base = format!("/api/v1/trees/{}", h.tree_id);
    let (_, media) = upload(
        &h.app,
        h.tree_id,
        &[("file", Some("living.png"), &png(60, 60))],
    )
    .await;
    let media_id = media["id"].as_str().unwrap();
    let (_, family) = json_request(&h.app, Method::POST, &format!("{base}/families"), None).await;

    // Nothing enforces this yet — but the choice is stored, so classifying a
    // tree today does not have to be redone when enforcement arrives.
    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/media/{media_id}"),
        Some(json!({"privacy": "private"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["privacy"], "private");

    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/families/{}", family["id"].as_str().unwrap()),
        Some(json!({"privacy": "private"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["privacy"], "private");
}

#[tokio::test]
async fn a_couple_defaults_to_following_the_tree() {
    let h = setup().await;
    let base = format!("/api/v1/trees/{}", h.tree_id);
    let (_, family) = json_request(&h.app, Method::POST, &format!("{base}/families"), None).await;
    // Not `public`: a tree that has said nothing about a couple has not said
    // the couple may be published.
    assert_eq!(family["privacy"], "default");
}

#[tokio::test]
async fn a_tree_says_what_default_privacy_means_and_starts_by_withholding() {
    let h = setup().await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    // A genealogy holds living people, and a tree nobody has classified has
    // not been cleared for publication. Publishing is the deliberate act.
    let (_, tree) = json_request(&h.app, Method::GET, &base, None).await;
    assert_eq!(tree["default_privacy"], "private");

    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &base,
        Some(json!({"default_privacy": "public"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["default_privacy"], "public");

    // The tree's own setting is not a record's: one that has chosen keeps its
    // choice, which is the whole reason both fields exist.
    let (_, family) = json_request(&h.app, Method::POST, &format!("{base}/families"), None).await;
    assert_eq!(family["privacy"], "default");
}

#[tokio::test]
async fn a_person_who_chose_no_portrait_still_shows_one_of_their_photographs() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (_first_document, first, _) = attach_photo(&h, &person_id, "first.png").await;
    attach_photo(&h, &person_id, "second.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    // No import sets a portrait — neither GEDCOM nor a `.gw` says which of
    // somebody's pictures represents them — so without a fallback a freshly
    // imported tree draws silhouettes for everyone who has photographs.
    let (status, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    assert_eq!(status, StatusCode::OK, "{portraits}");
    let rows = portraits.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{portraits}");
    assert_eq!(
        rows[0]["media_id"],
        first.as_str(),
        "the first by sort order"
    );

    // And the projection agrees, or a card and an avatar disagree.
    let (_, profile) = json_request(
        &h.app,
        Method::GET,
        &format!("{base}/profiles/{person_id}"),
        None,
    )
    .await;
    assert_eq!(profile["primary_media"]["media_id"], first.as_str());
}

#[tokio::test]
async fn a_record_naming_a_file_nobody_uploaded_is_not_a_portrait_by_default() {
    let h = setup().await;
    let person_id = person(&h).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    // A GEDCOM-imported row: a name, no bytes, nothing to draw. Falling back
    // to it would put a broken image on the card.
    let (_, media) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media"),
        Some(json!({"file_name": "scan.jpg", "file_path": "C:\\Photos\\scan.jpg"})),
    )
    .await;
    json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media-links"),
        Some(json!({"media_id": media["id"], "person_id": person_id, "sort_order": 0})),
    )
    .await;

    let (_, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    assert!(portraits.as_array().unwrap().is_empty(), "{portraits}");
}

#[tokio::test]
async fn a_gedzip_round_trip_carries_photographs_and_identifications_into_the_new_tree() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, page_id, _) = attach_photo(&h, &person_id, "portrait.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);
    let (status, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{page_id}/vignettes"),
        Some(json!({
            "x": 12,
            "y": 18,
            "width": 24,
            "height": 30,
            "person_id": person_id,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{vignette}");
    assert_eq!(vignette["media_id"], page_id);

    let (status, _, archive) =
        raw(&h.app, &format!("{base}/gedcom/export?format=gedzip"), &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!archive.is_empty());

    // Import it back as a second tree. This is the whole promise of the
    // format: the pictures travel with the genealogy.
    let (status, imported) = json_request(
        &h.app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({"name": "round trip"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{imported}");
    let new_tree = imported["id"].as_str().unwrap().to_string();

    let response = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/trees/{new_tree}/gedzip/import"))
                .header(header::CONTENT_TYPE, "application/zip")
                .body(Body::from(archive))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let summary: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    assert_eq!(status, StatusCode::CREATED, "{summary}");
    assert_eq!(summary["warnings"], json!([]), "{summary}");

    let (_, media) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{new_tree}/media"),
        None,
    )
    .await;
    let rows = media["edges"]
        .as_array()
        .unwrap_or_else(|| panic!("unexpected list shape: {media}"));
    assert_eq!(rows.len(), 1, "a gallery lists documents: {media}");
    let document_id = rows[0]["node"]["id"].as_str().expect("document id");
    assert_ne!(document_id, media_id);

    // The point: bytes, not just a record naming a file. They live on the
    // page, which is what the archive carried and what a crop is drawn on.
    let (_, pages) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{new_tree}/media/{document_id}/pages"),
        None,
    )
    .await;
    let pages = pages.as_array().expect("page list");
    assert_eq!(pages.len(), 1, "{pages:?}");
    let imported = &pages[0];
    assert!(
        imported["storage_key"].is_string(),
        "the photograph arrived without its bytes: {imported}"
    );
    assert!(imported["thumbnail_key"].is_string());
    assert_ne!(imported["id"], page_id.as_str(), "a new record, new tree");
    let imported_media_id = imported["id"].as_str().expect("media id");

    let (status, vignettes) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{new_tree}/media/{imported_media_id}/vignettes"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{vignettes}");
    let rows = vignettes.as_array().expect("vignette list");
    assert_eq!(rows.len(), 1, "{vignettes}");
    assert_eq!(rows[0]["media_id"], imported_media_id);
    assert_eq!(rows[0]["x"], 12);
    assert_eq!(rows[0]["y"], 18);
    assert_eq!(rows[0]["width"], 24);
    assert_eq!(rows[0]["height"], 30);
    assert!(rows[0]["person_id"].is_string(), "{vignettes}");
    assert_ne!(rows[0]["person_id"], person_id, "a new person, new tree");

    let (status, links) = json_request(
        &h.app,
        Method::GET,
        &format!("/api/v1/trees/{new_tree}/media-links?media_id={document_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{links}");
    let links = links.as_array().expect("media links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["media_id"], document_id);
    assert_eq!(links[0]["person_id"], rows[0]["person_id"]);
}
