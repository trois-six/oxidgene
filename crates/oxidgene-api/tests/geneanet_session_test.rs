//! Session readers share the same external collection and archive contract.

use crate::common;

use axum::{body::Body, http::Request};
use base64::Engine as _;
use http_body_util::BodyExt;
use oxidgene_api::{AppState, build_router};
use oxidgene_db::repo::connect;
use serde_json::{Value, json};
use tower::ServiceExt;

use common::setup_db;

async fn session_contract(graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()).with_local_file_access());
    let collection = json!({"deposits": [], "references": [], "view_references": {}});
    let archive = oxidgene_geneanet::session::encode(&oxidgene_geneanet::session::Session {
        collection: collection.to_string(),
        ..Default::default()
    })
    .unwrap();
    let mut inline = collection.clone();
    inline["oxidgene_media"] = json!({"https://example.invalid/medium.jpg": "aGVsbG8="});
    for (bytes, accepted) in [
        (collection.to_string().into_bytes(), true),
        (archive, true),
        (inline.to_string().into_bytes(), false),
    ] {
        let (uri, body, content_type) = if graphql {
            ("/graphql", json!({
                "query": "mutation($archive: String!) { decodeGeneanetSession(archiveBase64: $archive) { collection photoCount media { url path } } }",
                "variables": {"archive": base64::engine::general_purpose::STANDARD.encode(bytes)}
            }).to_string().into_bytes(), "application/json")
        } else {
            (
                "/api/v1/geneanet/session/decode",
                bytes,
                "application/octet-stream",
            )
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let result = if graphql {
            assert!(status.is_success());
            assert_eq!(value.get("errors").is_none(), accepted, "{value}");
            &value["data"]["decodeGeneanetSession"]
        } else {
            assert_eq!(status.is_success(), accepted, "{value}");
            &value
        };
        if accepted {
            assert_eq!(result["media"], if graphql { json!([]) } else { json!({}) });
            let restored: Value =
                serde_json::from_str(result["collection"].as_str().unwrap()).unwrap();
            assert_eq!(restored["deposits"], collection["deposits"]);
        }
    }
}

#[tokio::test]
async fn rest_session_contract() {
    session_contract(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_session_contract() {
    session_contract(true).await;
}

/// A session whose deposit size is past the signed range is refused, never
/// wrapped negative — on both surfaces.
#[tokio::test]
async fn a_session_deposit_size_past_the_signed_range_is_refused() {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()).with_local_file_access());
    let archive = oxidgene_geneanet::session::encode(&oxidgene_geneanet::session::Session {
        collection: json!({"deposits": [], "references": [], "view_references": {}}).to_string(),
        deposit_sizes: std::collections::HashMap::from([(1, u64::MAX)]),
        ..Default::default()
    })
    .unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/geneanet/session/decode")
                .header("content-type", "application/octet-stream")
                .body(Body::from(archive.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST, "{value}");
    assert_eq!(value["error"], "validation_error", "{value}");

    #[cfg(feature = "graphql")]
    {
        let response = common::gql(
            &app,
            "mutation($archive: String!) { decodeGeneanetSession(archiveBase64: $archive) { depositSizes { size } } }",
            json!({ "archive": base64::engine::general_purpose::STANDARD.encode(archive) }),
        )
        .await;
        assert_eq!(
            common::gql_error_code(&response),
            "VALIDATION_ERROR",
            "{response}"
        );
    }
}

#[tokio::test]
async fn session_archive_can_exceed_the_metadata_body_limit() {
    use std::io::Write as _;

    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("session.json", options).unwrap();
    archive
        .write_all(br#"{"deposits":[],"references":[],"view_references":{},"oxidgene_media":{"https://example.invalid/media.bin":"media/00000.bin"}}"#)
        .unwrap();
    archive.start_file("media/00000.bin", options).unwrap();
    let chunk = [0x5a; 64 * 1024];
    for _ in 0..513 {
        archive.write_all(&chunk).unwrap();
    }
    let bytes = archive.finish().unwrap().into_inner();
    assert!(bytes.len() > 32 * 1024 * 1024);
    #[cfg(feature = "graphql")]
    {
        let body = json!({
            "query": "mutation($archive: String!) { decodeGeneanetSession(archiveBase64: $archive) { media { path } } }",
            "variables": {"archive": base64::engine::general_purpose::STANDARD.encode(&bytes)}
        }).to_string();
        check_archive(Body::from(body), Some(513 * chunk.len()), true).await;
    }
    check_archive(
        streamed_body(std::io::Cursor::new(bytes)),
        Some(513 * chunk.len()),
        false,
    )
    .await;
}

fn streamed_body(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static) -> Body {
    use tokio::io::AsyncReadExt as _;
    Body::from_stream(futures_util::stream::try_unfold(
        reader,
        |mut reader| async move {
            let mut chunk = vec![0; 64 * 1024];
            let size = reader.read(&mut chunk).await?;
            chunk.truncate(size);
            Ok::<_, std::io::Error>((size != 0).then_some((chunk, reader)))
        },
    ))
}

async fn check_archive(body: Body, expected_media_size: Option<usize>, graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()).with_local_file_access());
    let (uri, content_type) = if graphql {
        ("/graphql", "application/json")
    } else {
        ("/api/v1/geneanet/session/decode", "application/zip")
    };
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", content_type)
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    let paths: Vec<_> = if graphql {
        assert!(result.get("errors").is_none());
        result["data"]["decodeGeneanetSession"]["media"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["path"].as_str().unwrap())
            .collect()
    } else {
        result["media"]
            .as_object()
            .unwrap()
            .values()
            .map(|path| path.as_str().unwrap())
            .collect()
    };
    assert!(!paths.is_empty());
    for path in paths {
        let metadata = std::fs::metadata(path).unwrap();
        if let Some(expected) = expected_media_size {
            assert_eq!(metadata.len(), expected as u64);
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[tokio::test]
#[ignore = "needs a private session archive: run by `just session-check`"]
async fn supplied_session_archive_loads_without_logging_its_contents() {
    let path = std::env::var_os("OXIDGENE_GENEANET_SESSION")
        .expect("set OXIDGENE_GENEANET_SESSION to a session archive");
    check_archive(
        streamed_body(tokio::fs::File::open(path).await.unwrap()),
        None,
        false,
    )
    .await;
}

#[tokio::test]
async fn standalone_server_rejects_session_upload_before_reading_it() {
    let db = connect("sqlite::memory:").await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()));
    let body = Body::from_stream(futures_util::stream::poll_fn(
        |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
            panic!("a disabled local-file capability must not consume the body");
        },
    ));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/geneanet/session/decode")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

/// POST `body` as JSON to `uri` and return the status with the JSON answer.
async fn post_json(app: &axum::Router, uri: &str, body: Value) -> (axum::http::StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A wizard closed without importing releases the photos its session staged;
/// a path the backend did not stage is ignored.
async fn released_session_media_are_deleted(graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()).with_local_file_access());
    let archive = oxidgene_geneanet::session::encode(&oxidgene_geneanet::session::Session {
        collection: json!({"deposits": [], "references": [], "view_references": {}}).to_string(),
        media: std::collections::HashMap::from([(
            "https://example.invalid/medium.jpg".to_string(),
            "aGVsbG8=".to_string(),
        )]),
        ..Default::default()
    })
    .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/geneanet/session/decode")
                .header("content-type", "application/octet-stream")
                .body(Body::from(archive))
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let decoded: Value = serde_json::from_slice(&bytes).unwrap();
    let staged = decoded["media"]["https://example.invalid/medium.jpg"]
        .as_str()
        .expect("a staged path")
        .to_string();
    assert!(std::path::Path::new(&staged).exists());
    let unrelated = root.path().join("unrelated-fixture");
    std::fs::write(&unrelated, b"kept").unwrap();
    let paths = json!([staged, unrelated.to_string_lossy()]);

    if graphql {
        let (status, value) = post_json(
            &app,
            "/graphql",
            json!({
                "query": "mutation($paths: [String!]!) { releaseGeneanetSessionMedia(paths: $paths) }",
                "variables": {"paths": paths},
            }),
        )
        .await;
        assert!(status.is_success());
        assert_eq!(
            value["data"]["releaseGeneanetSessionMedia"], true,
            "{value}"
        );
    } else {
        let (status, _) = post_json(
            &app,
            "/api/v1/geneanet/session/release",
            json!({ "paths": paths }),
        )
        .await;
        assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    }
    assert!(!std::path::Path::new(&staged).exists());
    assert!(unrelated.exists());
}

#[tokio::test]
async fn rest_releases_staged_session_media() {
    released_session_media_are_deleted(false).await;
}

#[tokio::test]
async fn graphql_releases_staged_session_media() {
    released_session_media_are_deleted(true).await;
}

#[tokio::test]
async fn standalone_server_refuses_to_release_session_media() {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()));
    let (status, _) = post_json(
        &app,
        "/api/v1/geneanet/session/release",
        json!({ "paths": ["/tmp/oxidgene-geneanet-fixture"] }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let (_, value) = post_json(
        &app,
        "/graphql",
        json!({ "query": "mutation { releaseGeneanetSessionMedia(paths: []) }" }),
    )
    .await;
    assert!(value.get("errors").is_some(), "{value}");
}

/// Saving a session streams each medium from its local path, and a path
/// that cannot be read fails the save instead of leaving the medium out.
async fn encoded_session_carries_every_medium(graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()).with_local_file_access());
    let collection = json!({"deposits": [], "references": [], "view_references": {}}).to_string();
    let medium = root.path().join("medium.jpg");
    std::fs::write(&medium, b"picture").unwrap();
    let missing = root.path().join("missing.jpg");
    let url = "https://example.invalid/medium.jpg";

    for (path, saved) in [(&medium, true), (&missing, false)] {
        let path = path.to_string_lossy().into_owned();
        let archive = if graphql {
            let (status, value) = post_json(
                &app,
                "/graphql",
                json!({
                    "query": "mutation($input: GeneanetSessionEncodeInput!) { encodeGeneanetSession(input: $input) { archiveBase64 } }",
                    "variables": {"input": {"collection": collection, "media": [{"url": url, "path": path}]}},
                }),
            )
            .await;
            assert!(status.is_success());
            assert_eq!(value.get("errors").is_none(), saved, "{value}");
            value["data"]["encodeGeneanetSession"]["archiveBase64"]
                .as_str()
                .map(|archive| {
                    base64::engine::general_purpose::STANDARD
                        .decode(archive)
                        .unwrap()
                })
        } else {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/geneanet/session/encode")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({"collection": collection, "media": {url: path}}).to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().is_success(), saved);
            if !saved {
                assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
            }
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            saved.then(|| bytes.to_vec())
        };
        if let Some(archive) = archive {
            let session = oxidgene_geneanet::session::decode(&archive).unwrap();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&session.media[url])
                .unwrap();
            assert_eq!(bytes, b"picture");
        }
    }
}

#[tokio::test]
async fn rest_encoded_session_carries_every_medium() {
    encoded_session_carries_every_medium(false).await;
}

#[tokio::test]
async fn graphql_encoded_session_carries_every_medium() {
    encoded_session_carries_every_medium(true).await;
}

// ── The web build's session import: no path exchanged ─────────────────

/// A PNG of a few pixels, which the import decodes and stores.
fn png() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(8, 8, image::Rgb([120, 90, 60]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

const RENDITION_URL: &str = "https://example.invalid/normal.jpg";

/// A saved session naming one photograph of `Ada FICTIF`, its
/// rendition inside.
fn session_with_one_photo() -> Vec<u8> {
    let collection = json!({
        "deposits": [{"id": 1, "views": [{"id": 10, "files": {"normal": RENDITION_URL}}]}],
        "references": [{
            "deposit": {"id": 1, "views": [{"id": 10}]},
            "firstname": "Ada",
            "lastname": "FICTIF",
            "reference_extra_geneweb": {"ref": "fictif|ada|"},
        }],
    });
    oxidgene_geneanet::session::encode(&oxidgene_geneanet::session::Session {
        collection: collection.to_string(),
        media: std::collections::HashMap::from([(
            RENDITION_URL.to_string(),
            base64::engine::general_purpose::STANDARD.encode(png()),
        )]),
        ..Default::default()
    })
    .unwrap()
}

const GENEWEB: &str = "encoding: utf-8\n\nfam FICTIF Ada.0 + EXEMPLE Basile.0\n";

/// A server without local file access reads a saved session: its contents
/// and the addresses of its media, nothing kept; and refuses what is no
/// session.
async fn a_session_is_read_without_local_files(graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()));
    for (bytes, accepted) in [
        (session_with_one_photo(), true),
        (b"not a session".to_vec(), false),
    ] {
        let result = if graphql {
            let response = common::gql(
                &app,
                "query($archive: String!) { readGeneanetSession(archiveBase64: $archive) { collection photoCount mediaUrls } }",
                json!({ "archive": base64::engine::general_purpose::STANDARD.encode(&bytes) }),
            )
            .await;
            assert_eq!(response.get("errors").is_none(), accepted, "{response}");
            response["data"]["readGeneanetSession"].clone()
        } else {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/geneanet/session/read")
                        .header("content-type", "application/octet-stream")
                        .body(Body::from(bytes))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(status.is_success(), accepted, "{value}");
            json!({
                "collection": value["collection"],
                "photoCount": value["photo_count"],
                "mediaUrls": value["media_urls"],
            })
        };
        if accepted {
            assert_eq!(result["mediaUrls"], json!([RENDITION_URL]));
            assert_eq!(result["photoCount"], 1);
            assert!(result["collection"].as_str().unwrap().contains("FICTIF"));
        }
    }
    // Nothing of the session stays in the working directory.
    assert_no_extracted_media(root.path());
}

/// Every file under `root`.
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[tokio::test]
async fn rest_reads_a_session_without_local_files() {
    a_session_is_read_without_local_files(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_reads_a_session_without_local_files() {
    a_session_is_read_without_local_files(true).await;
}

/// The multipart body of a session import keeping `fidelity`'s media.
fn session_import_form(gw: &[u8], session: &[u8], fidelity: &str) -> (String, Vec<u8>) {
    let boundary = "oxidgene-test-boundary";
    let mut body = Vec::new();
    let mut part = |headers: &str, content: &[u8]| {
        body.extend_from_slice(format!("--{boundary}\r\n{headers}\r\n\r\n").as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    };
    part(
        "Content-Disposition: form-data; name=\"gw\"; filename=\"family.gw\"\r\nContent-Type: application/octet-stream",
        gw,
    );
    part(
        "Content-Disposition: form-data; name=\"media_fidelity\"",
        fidelity.as_bytes(),
    );
    part(
        "Content-Disposition: form-data; name=\"session\"; filename=\"geneanet-session.zip\"\r\nContent-Type: application/zip",
        session,
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

/// Start a session import of [`GENEWEB`] and `session` into `tree_id`
/// through REST or GraphQL, keeping `fidelity`'s media (`renditions` or
/// `originals`), answering the job's id.
async fn start_session_import(
    app: &axum::Router,
    tree_id: &str,
    graphql: bool,
    session: &[u8],
    fidelity: &str,
) -> String {
    if graphql {
        let response = common::gql(
            app,
            "mutation($tree: ID!, $input: GeneanetSessionImportInput!) { importGeneanetSession(treeId: $tree, input: $input) { jobId } }",
            json!({
                "tree": tree_id,
                "input": {
                    "gwBase64": base64::engine::general_purpose::STANDARD.encode(GENEWEB),
                    "fileName": "family.gw",
                    "sessionBase64": base64::engine::general_purpose::STANDARD.encode(session),
                    "mediaFidelity": fidelity.to_uppercase(),
                },
            }),
        )
        .await;
        assert!(response.get("errors").is_none(), "{response}");
        response["data"]["importGeneanetSession"]["jobId"]
            .as_str()
            .unwrap()
            .to_string()
    } else {
        let (content_type, body) = session_import_form(GENEWEB.as_bytes(), session, fidelity);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/trees/{tree_id}/geneanet/session-import"))
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status, axum::http::StatusCode::ACCEPTED, "{value}");
        value["job_id"].as_str().unwrap().to_string()
    }
}

/// No medium extracted from a session is left under `root`.
fn assert_no_extracted_media(root: &std::path::Path) {
    let leftovers: Vec<_> = walk(root)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("geneanet-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// A new tree's id.
async fn create_tree(app: &axum::Router) -> String {
    let (status, tree) = common::send(
        app,
        axum::http::Method::POST,
        "/api/v1/trees",
        Some(json!({ "name": "Session import" })),
    )
    .await;
    assert!(status.is_success(), "{tree}");
    tree["id"].as_str().unwrap().to_string()
}

/// The completed import `status` of [`GENEWEB`] stored the session's one
/// photograph on the person it names.
fn assert_one_photo_imported(status: &Value) {
    assert_eq!(status["phase"], "completed", "{status}");
    let result = &status["geneanet_result"];
    assert_eq!(result["persons_count"], 2, "{status}");
    assert_eq!(result["images_count"], 1, "{status}");
    assert_eq!(result["links_count"], 1, "{status}");
    // On a person of the file, not one created for an unknown name.
    assert_eq!(result["isolated_count"], 0, "{status}");
}

/// A server without local file access imports a `.gw` with the media of a
/// saved session sent beside it: the photograph lands on its person, and
/// neither the job's inputs nor the extracted media outlive the import.
async fn a_session_import_runs_without_local_files(graphql: bool) {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(db, root.path());
    let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "geneanet-session-import-worker",
    );
    let app = build_router(state.clone());
    let tree_id = create_tree(&app).await;

    let job_id = start_session_import(
        &app,
        &tree_id,
        graphql,
        &session_with_one_photo(),
        "renditions",
    )
    .await;
    let job: uuid::Uuid = job_id.parse().unwrap();
    // The session's medium is already in job storage, the extracted file gone.
    assert!(
        state
            .media
            .exists(&oxidgene_api::media::store::job_input_blob_key(job, 0))
            .await
    );
    assert_no_extracted_media(root.path());

    assert!(worker.run_once().await.expect("run the session import"));
    let (status, completed) = common::send(
        &app,
        axum::http::Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_one_photo_imported(&completed);
    assert!(
        !state
            .media
            .exists(&oxidgene_api::media::store::job_input_blob_key(job, 0))
            .await
    );
}

#[tokio::test]
async fn rest_imports_a_session_without_local_files() {
    a_session_import_runs_without_local_files(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_imports_a_session_without_local_files() {
    a_session_import_runs_without_local_files(true).await;
}

/// The session file the desktop saves with the data `archive` through REST
/// or GraphQL, for `collection` whose deposits all measure `size` bytes.
async fn save_session_with_archive(
    collection: &str,
    size: usize,
    archive: &std::path::Path,
    graphql: bool,
) -> Vec<u8> {
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(setup_db().await, root.path()).with_local_file_access());
    let archive = archive.to_string_lossy().into_owned();
    if graphql {
        let (status, value) = post_json(
            &app,
            "/graphql",
            json!({
                "query": "mutation($input: GeneanetSessionEncodeInput!) { encodeGeneanetSession(input: $input) { archiveBase64 } }",
                "variables": {"input": {
                    "collection": collection,
                    "depositSizes": [{"depositId": 1, "size": size}, {"depositId": 2, "size": size}],
                    "archivePaths": [archive],
                }},
            }),
        )
        .await;
        assert!(
            status.is_success() && value.get("errors").is_none(),
            "{value}"
        );
        let archive = value["data"]["encodeGeneanetSession"]["archiveBase64"]
            .as_str()
            .unwrap();
        base64::engine::general_purpose::STANDARD
            .decode(archive)
            .unwrap()
    } else {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/geneanet/session/encode")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "collection": collection,
                            "deposit_sizes": {"1": size, "2": size},
                            "archive_paths": [archive],
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }
}

/// A session saved with the data archives carries the original of each
/// single-page deposit they hold, under that original's URL — a document's
/// pages, which the import never reads from an archive by length, stay out
/// — and a server given no archive and no local file imports the photograph
/// from that file alone.
async fn a_session_saved_with_archives_brings_their_originals(graphql: bool) {
    use std::io::Write as _;

    let photo = png();
    let work = tempfile::tempdir().unwrap();
    let archive = work.path().join("export.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    zip.start_file("media/photo.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&photo).unwrap();
    zip.finish().unwrap();
    let collection = json!({
        "deposits": [
            {"id": 1, "views": [{"id": 10, "files": {"normal": RENDITION_URL}}]},
            {"id": 2, "views": [
                {"id": 20, "page": 1, "files": {"normal": "https://example.invalid/2-1.jpg"}},
                {"id": 21, "page": 2, "files": {"normal": "https://example.invalid/2-2.jpg"}}
            ]}
        ],
        "references": [{
            "deposit": {"id": 1, "views": [{"id": 10}]},
            "firstname": "Ada",
            "lastname": "FICTIF",
            "reference_extra_geneweb": {"ref": "fictif|ada|"},
        }],
        "view_references": {}
    })
    .to_string();
    let saved = save_session_with_archive(&collection, photo.len(), &archive, graphql).await;
    let session = oxidgene_geneanet::session::decode(&saved).unwrap();
    let manifest = oxidgene_geneanet::manifest_from_collection(&collection).unwrap();
    let original = &manifest.deposits[0].original;
    assert_eq!(
        session.media.keys().collect::<Vec<_>>(),
        [original],
        "the one original the archive holds"
    );

    let root = tempfile::tempdir().unwrap();
    let state = AppState::new(setup_db().await, root.path());
    let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
        state.db.clone(),
        std::sync::Arc::clone(&state.profiles),
        std::sync::Arc::clone(&state.media),
        "geneanet-session-originals-worker",
    );
    let app = build_router(state);
    let tree_id = create_tree(&app).await;
    let job_id = start_session_import(&app, &tree_id, graphql, &saved, "originals").await;
    assert!(worker.run_once().await.expect("run the session import"));
    let (status, completed) = common::send(
        &app,
        axum::http::Method::GET,
        &format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_one_photo_imported(&completed);
    assert_eq!(
        completed["geneanet_result"]["skipped"],
        json!([]),
        "{completed}"
    );
}

#[tokio::test]
async fn rest_session_saved_with_archives_brings_their_originals() {
    a_session_saved_with_archives_brings_their_originals(false).await;
}

#[cfg(feature = "graphql")]
#[tokio::test]
async fn graphql_session_saved_with_archives_brings_their_originals() {
    a_session_saved_with_archives_brings_their_originals(true).await;
}

/// The preview and the plan read no path unless they name an archive: one
/// naming none is served without local file access, one naming an archive
/// is refused there.
#[tokio::test]
async fn a_preview_names_a_path_only_with_an_archive() {
    let db = setup_db().await;
    let root = tempfile::tempdir().unwrap();
    let app = build_router(AppState::new(db, root.path()));
    for (archive_paths, accepted) in [(json!([]), true), (json!(["/does/not/exist.zip"]), false)] {
        let body = json!({
            "gw_base64": base64::engine::general_purpose::STANDARD.encode(GENEWEB),
            "file_name": "family.gw",
            "collection": json!({"deposits": [], "references": [], "view_references": {}}).to_string(),
            "archive_paths": archive_paths,
        });
        for uri in ["/api/v1/geneanet/preview", "/api/v1/geneanet/plan"] {
            let (status, value) = post_json(&app, uri, body.clone()).await;
            assert_eq!(status.is_success(), accepted, "{uri}: {value}");
        }
        #[cfg(feature = "graphql")]
        {
            let response = common::gql(
                &app,
                "query($input: GeneanetPreviewInput!) { geneanetPreview(input: $input) { personCount } geneanetPlan(input: $input) { url } }",
                json!({ "input": {
                    "gwBase64": base64::engine::general_purpose::STANDARD.encode(GENEWEB),
                    "fileName": "family.gw",
                    "collection": json!({"deposits": [], "references": [], "view_references": {}}).to_string(),
                    "archivePaths": archive_paths,
                }}),
            )
            .await;
            assert_eq!(response.get("errors").is_none(), accepted, "{response}");
        }
    }
}
