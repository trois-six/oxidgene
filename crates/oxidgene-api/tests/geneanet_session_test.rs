//! Session readers share the same external collection and archive contract.

mod common;

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
#[ignore = "requires an explicitly supplied private session archive"]
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
