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
    root: TempRoot,
    tree_id: Uuid,
}

async fn setup() -> Harness {
    let db = connect("sqlite::memory:").await.expect("connect");
    run_migrations(&db).await.expect("migrations");
    let root = TempRoot::new();
    let app = build_router(AppState::new(db, &root.0));

    let (status, tree) = json_request(
        &app,
        Method::POST,
        "/api/v1/trees",
        Some(json!({"name": "Test tree"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "tree setup failed: {tree}");
    let tree_id = Uuid::parse_str(tree["id"].as_str().unwrap()).unwrap();

    Harness { app, root, tree_id }
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

async fn upload(
    app: &axum::Router,
    tree_id: Uuid,
    parts: &[(&str, Option<&str>, &[u8])],
) -> (StatusCode, Value) {
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
    let (status, stub) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
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
    assert!(
        body["message"].as_str().unwrap().contains("400×300"),
        "the message should say what it did not fit in: {body}"
    );
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
async fn retitling_a_vignette_leaves_its_rectangle_alone() {
    let h = setup().await;
    let media_id = scan(&h, 500, 500).await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 10, "y": 20, "width": 100, "height": 100, "title": "before"})),
    )
    .await;
    let id = vignette["id"].as_str().unwrap();

    let (status, updated) = json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/vignettes/{id}"),
        Some(json!({"title": "after"})),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "after");
    assert_eq!(
        (updated["x"].as_i64(), updated["y"].as_i64()),
        (Some(10), Some(20))
    );
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

/// Upload a photo, attach it to `person_id`, and return (media_id, link_id).
async fn attach_photo(h: &Harness, person_id: &str, name: &str) -> (String, String) {
    let (_, media) = upload(&h.app, h.tree_id, &[("file", Some(name), &png(240, 180))]).await;
    let media_id = media["id"].as_str().unwrap().to_string();

    let (status, link) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media-links", h.tree_id),
        Some(json!({"media_id": media_id, "person_id": person_id, "sort_order": 0})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{link}");
    (media_id, link["id"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn one_request_returns_a_person_gallery_with_everything_a_tile_needs() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, link_id) = attach_photo(&h, &person_id, "portrait.png").await;

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
    // The three things a tile cannot be drawn without.
    assert_eq!(row["mime_type"], "image/png");
    assert!(row["thumbnail_key"].is_string());
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
    let (media_id, _) = attach_photo(&h, &person_id, "photo.png").await;

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
    let (first_media, _) = attach_photo(&h, &person_id, "first.png").await;
    let (second_media, _) = attach_photo(&h, &person_id, "second.png").await;
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
    assert_eq!(rows[0]["media_id"], second_media.as_str());
}

#[tokio::test]
async fn a_portrait_can_be_a_face_in_a_group_photograph() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _) = attach_photo(&h, &person_id, "wedding.png").await;
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
async fn a_portrait_can_be_cleared() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _) = attach_photo(&h, &person_id, "photo.png").await;
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
    let (_, portraits) =
        json_request(&h.app, Method::GET, &format!("{base}/portraits"), None).await;
    assert!(portraits.as_array().unwrap().is_empty(), "{portraits}");
}

#[tokio::test]
async fn a_portrait_is_a_media_or_a_crop_but_never_both() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _) = attach_photo(&h, &person_id, "photo.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({"x": 0, "y": 0, "width": 20, "height": 20})),
    )
    .await;

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
    let (media_id, link_id) = attach_photo(&h, &person_id, "shared.png").await;
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
    assert_eq!(doc["is_document"], true);
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
async fn detaching_a_page_closes_the_gap_and_keeps_the_scan() {
    let h = setup().await;
    let doc = document(&h, "Register").await;
    add_page(&h, &doc, "a.png").await;
    let middle = add_page(&h, &doc, "b.png").await;
    add_page(&h, &doc, "c.png").await;

    let (status, detached) = json_request(
        &h.app,
        Method::DELETE,
        &format!("/api/v1/trees/{}/media/{doc}/pages/{middle}", h.tree_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detached}");
    assert!(detached["parent_media_id"].is_null());

    let pages = pages_of(&h, &doc).await;
    let indexes: Vec<i64> = pages
        .iter()
        .map(|p| p["page_index"].as_i64().unwrap())
        .collect();
    assert_eq!(indexes, [0, 1], "page 3 of a 2-page document is not a page");

    // The scan itself survives: it is a document somebody made.
    let (status, _, _) = raw(
        &h.app,
        &format!("/api/v1/trees/{}/media/{middle}/file", h.tree_id),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
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
async fn a_record_with_no_bytes_can_be_repointed_at_a_url() {
    let h = setup().await;
    let (_, stub) = json_request(
        &h.app,
        Method::POST,
        &format!("/api/v1/trees/{}/media", h.tree_id),
        Some(json!({
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
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
}

#[tokio::test]
async fn a_crop_portrait_reaches_the_read_projection() {
    let h = setup().await;
    let person_id = person(&h).await;
    let (media_id, _) = attach_photo(&h, &person_id, "wedding.png").await;
    let base = format!("/api/v1/trees/{}", h.tree_id);

    let (_, vignette) = json_request(
        &h.app,
        Method::POST,
        &format!("{base}/media/{media_id}/vignettes"),
        Some(json!({
            "x": 10, "y": 10, "width": 30, "height": 30,
            "person_id": person_id, "title": "second from the left"
        })),
    )
    .await;
    json_request(
        &h.app,
        Method::PUT,
        &format!("{base}/persons/{person_id}/portrait"),
        Some(json!({"vignette_id": vignette["id"]})),
    )
    .await;

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
    // The crop's title says more than the scan's caption ever could.
    assert_eq!(portrait["title"], "second from the left");
}

#[tokio::test]
async fn the_projection_draws_the_portrait_that_was_chosen() {
    let h = setup().await;
    let person_id = person(&h).await;
    // Attached first, so it holds the lowest sort_order. The projection used
    // to take that one and ignore the stored choice entirely, so a person
    // could star a photograph and have their card go on drawing another.
    let (first, _) = attach_photo(&h, &person_id, "first.png").await;
    let (second, _) = attach_photo(&h, &person_id, "second.png").await;
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
    assert_eq!(profile["primary_media"]["media_id"], second.as_str());
    assert_ne!(profile["primary_media"]["media_id"], first.as_str());
}
