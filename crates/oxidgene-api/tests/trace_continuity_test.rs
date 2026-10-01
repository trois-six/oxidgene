//! One request is one trace, all the way down.
//!
//! The router is traced the way the server traces it — the native span
//! pipeline (`oxidgene_observability::span_export_layer` and `sampler`), the
//! HTTP trace layer and the request context — over an in-memory exporter.
//! Each request carries a W3C `traceparent`, as the browser client sends one,
//! and every span the request produces must belong to that trace and reach
//! the incoming parent through recorded spans: HTTP server span, service
//! spans, compute spans on the blocking pool, SeaORM calls. A span opened on
//! a thread or task that lost the request's context would start a trace of
//! its own, and the sampler below records every such root.
//!
//! The subscriber is global, so spans opened on any thread are seen. This file
//! therefore holds a single test: it owns its process's subscriber.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use opentelemetry::trace::{SpanId, TraceId, TracerProvider as _};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{
    InMemorySpanExporter, Sampler, SamplingResult, SdkTracerProvider, ShouldSample, SpanData,
};
use oxidgene_api::{AppState, build_router, request_context};
use oxidgene_db::repo::{connect, run_migrations};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt as _;
use uuid::Uuid;

/// The production sampler, noting every span it is asked about that has no
/// parent: a root trace. Requests carrying a `traceparent` must produce none.
#[derive(Debug, Clone)]
struct RootWatcher {
    inner: Sampler,
    roots: Arc<Mutex<Vec<String>>>,
}

impl ShouldSample for RootWatcher {
    fn should_sample(
        &self,
        parent_context: Option<&Context>,
        trace_id: TraceId,
        name: &str,
        span_kind: &opentelemetry::trace::SpanKind,
        attributes: &[KeyValue],
        links: &[opentelemetry::trace::Link],
    ) -> SamplingResult {
        use opentelemetry::trace::TraceContextExt as _;
        if !parent_context.is_some_and(Context::has_active_span) {
            self.roots
                .lock()
                .expect("roots lock")
                .push(name.to_string());
        }
        self.inner
            .should_sample(parent_context, trace_id, name, span_kind, attributes, links)
    }
}

struct Harness {
    app: axum::Router,
    exporter: InMemorySpanExporter,
    roots: Arc<Mutex<Vec<String>>>,
    /// For the background worker, which only follows the request's trace
    /// when the build carries trace context into the job table.
    #[cfg(feature = "telemetry-context")]
    state: AppState,
    media_root: std::path::PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.media_root);
    }
}

impl Harness {
    async fn new() -> Self {
        opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());
        let exporter = InMemorySpanExporter::default();
        let roots = Arc::new(Mutex::new(Vec::new()));
        let provider = SdkTracerProvider::builder()
            .with_sampler(RootWatcher {
                inner: oxidgene_observability::sampler(),
                roots: Arc::clone(&roots),
            })
            .with_simple_exporter(exporter.clone())
            .build();
        let tracer = provider.tracer("trace-continuity-test");
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(oxidgene_observability::span_export_layer(tracer)),
        )
        .expect("this test owns the process's subscriber");

        let db = connect("sqlite::memory:").await.expect("connect");
        run_migrations(&db).await.expect("migrations");
        let media_root =
            std::env::temp_dir().join(format!("oxidgene-trace-test-{}", Uuid::now_v7()));
        let state = AppState::new(db, &media_root);
        let app = request_context::wrap(build_router(state.clone())).layer(
            TraceLayer::new_for_http()
                .make_span_with(oxidgene_observability::make_http_span)
                .on_response(oxidgene_observability::on_http_response),
        );
        Self {
            app,
            exporter,
            roots,
            #[cfg(feature = "telemetry-context")]
            state,
            media_root,
        }
    }

    /// Forget what was exported and which roots were seen so far.
    fn reset(&self) {
        self.exporter.reset();
        self.roots.lock().expect("roots lock").clear();
    }

    async fn send(
        &self,
        method: Method,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
        traceparent: Option<&str>,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, content_type);
        if let Some(traceparent) = traceparent {
            request = request.header("traceparent", traceparent);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body)).expect("valid request"))
            .await
            .expect("infallible router");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        (status, bytes.to_vec())
    }

    async fn json(&self, method: Method, uri: &str, body: Option<Value>) -> Value {
        let body = body.map(|body| body.to_string().into_bytes());
        let (status, bytes) = self
            .send(
                method,
                uri,
                "application/json",
                body.unwrap_or_default(),
                None,
            )
            .await;
        assert!(status.is_success(), "{uri}: {status}");
        serde_json::from_slice(&bytes).expect("JSON body")
    }
}

/// A W3C trace context for one request: its trace and the client span that
/// sent it.
struct Incoming {
    trace_id: TraceId,
    parent: SpanId,
}

impl Incoming {
    fn new(seed: u8) -> Self {
        Self {
            trace_id: TraceId::from_bytes([seed; 16]),
            parent: SpanId::from_bytes([seed; 8]),
        }
    }

    fn header(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.parent)
    }
}

/// Every exported span belongs to `incoming`'s trace and reaches its parent
/// through exported spans; no root was opened; and `expected` spans exist.
fn assert_one_trace(
    harness: &Harness,
    route: &str,
    incoming: &Incoming,
    expected: &[&str],
) -> Vec<SpanData> {
    let spans = harness.exporter.get_finished_spans().expect("spans");
    let roots = harness.roots.lock().expect("roots lock").clone();
    assert!(
        roots.is_empty(),
        "{route}: spans opened a trace of their own: {roots:?}"
    );
    for span in &spans {
        assert_eq!(
            span.span_context.trace_id(),
            incoming.trace_id,
            "{route}: {} left the request's trace",
            span.name
        );
        let mut parent = span.parent_span_id;
        let mut hops = 0;
        while parent != incoming.parent {
            let Some(next) = spans.iter().find(|s| s.span_context.span_id() == parent) else {
                panic!(
                    "{route}: {} is not connected to the incoming parent: {parent} was never \
                     recorded",
                    span.name
                );
            };
            parent = next.parent_span_id;
            hops += 1;
            assert!(hops < 64, "{route}: parent cycle at {}", span.name);
        }
    }
    // The HTTP server span is exported under its display name, "<METHOD>
    // <route>", so it is recognised by its kind.
    let matches = |span: &SpanData, name: &str| {
        if name == "http.server.request" {
            span.span_kind == opentelemetry::trace::SpanKind::Server
        } else {
            span.name == name || span.name.starts_with(name)
        }
    };
    for name in expected {
        assert!(
            spans.iter().any(|span| matches(span, name)),
            "{route}: no {name} span among {:?}",
            spans.iter().map(|span| &span.name).collect::<Vec<_>>()
        );
    }
    spans
}

const GEDCOM: &str = "0 HEAD\n1 GEDC\n2 VERS 5.5.1\n2 FORM LINEAGE-LINKED\n1 CHAR UTF-8\n\
    0 @I1@ INDI\n1 NAME Alpha /Example/\n1 SEX M\n1 BIRT\n2 DATE 1 JAN 1900\n2 PLAC Placeville\n\
    0 @I2@ INDI\n1 NAME Beta /Sample/\n1 SEX F\n\
    0 @I3@ INDI\n1 NAME Gamma /Example/\n1 SEX F\n1 FAMC @F1@\n\
    0 @F1@ FAM\n1 HUSB @I1@\n1 WIFE @I2@\n1 CHIL @I3@\n1 MARR\n2 DATE 1925\n0 TRLR\n";

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .expect("encode PNG");
    out.into_inner()
}

fn multipart(parts: &[(&str, Option<&str>, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "----oxidgeneTraceBoundary";
    let mut body = Vec::new();
    for (name, file_name, content) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let disposition = match file_name {
            Some(file_name) => format!(
                "Content-Disposition: form-data; name=\"{name}\"; filename=\"{file_name}\"\r\n\
                 Content-Type: application/octet-stream\r\n\r\n"
            ),
            None => format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n"),
        };
        body.extend_from_slice(disposition.as_bytes());
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_span_of_a_request_belongs_to_the_incoming_trace() {
    let harness = Harness::new().await;

    // ── Fixtures, untraced ────────────────────────────────────────────
    let tree = harness
        .json(
            Method::POST,
            "/api/v1/trees",
            Some(json!({"name": "Trace tree"})),
        )
        .await;
    let tree_id = tree["id"].as_str().expect("tree id").to_string();
    harness
        .json(
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/gedcom/import"),
            Some(json!({ "gedcom": GEDCOM })),
        )
        .await;
    let persons = harness
        .json(
            Method::GET,
            &format!("/api/v1/trees/{tree_id}/persons"),
            None,
        )
        .await;
    let person_id = persons["edges"][0]["node"]["id"]
        .as_str()
        .expect("person id")
        .to_string();
    let document = harness
        .json(
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media/document"),
            Some(json!({ "title": null })),
        )
        .await;
    let document_id = document["id"].as_str().expect("document id").to_string();
    // The purge worker's startup sweep is a root of its own by design: let it
    // finish before watching for roots.
    for _ in 0..200 {
        let swept = harness
            .exporter
            .get_finished_spans()
            .expect("spans")
            .iter()
            .any(|span| span.name == "purge.sweep");
        if swept {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    // ── Upload: the thumbnail derivation runs on the blocking pool ────
    harness.reset();
    let incoming = Incoming::new(0x11);
    let picture = png(96, 64);
    let (content_type, body) = multipart(&[
        ("file", Some("picture.png"), &picture),
        ("document_id", None, document_id.as_bytes()),
    ]);
    let (status, page) = harness
        .send(
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media/upload"),
            &content_type,
            body,
            Some(&incoming.header()),
        )
        .await;
    assert!(status.is_success(), "upload: {status}");
    assert_one_trace(
        &harness,
        "media upload",
        &incoming,
        &["http.server.request", "media.derive", "sea_orm."],
    );
    let page: Value = serde_json::from_slice(&page).expect("page JSON");
    let media_id = page["id"].as_str().expect("media id").to_string();
    let vignette = harness
        .json(
            Method::POST,
            &format!("/api/v1/trees/{tree_id}/media/{media_id}/vignettes"),
            Some(json!({ "x": 8, "y": 8, "width": 32, "height": 24 })),
        )
        .await;
    let vignette_id = vignette["id"].as_str().expect("vignette id").to_string();

    // ── Read routes ──────────────────────────────────────────────────
    let reads: [(&str, String, &[&str]); 5] = [
        (
            "pedigree",
            format!(
                "/api/v1/trees/{tree_id}/pedigree/{person_id}?ancestor_depth=2&descendant_depth=1"
            ),
            &["http.server.request", "pedigree.build", "sea_orm."],
        ),
        (
            "person detail bundle",
            format!("/api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle"),
            &["http.server.request", "person_detail.load", "sea_orm."],
        ),
        (
            "statistics",
            format!("/api/v1/trees/{tree_id}/statistics"),
            &[
                "http.server.request",
                "statistics.load",
                "profile.decode",
                "statistics.compute",
                "sea_orm.",
            ],
        ),
        (
            "anomalies",
            format!("/api/v1/trees/{tree_id}/anomalies"),
            &[
                "http.server.request",
                "anomalies.load",
                "anomalies.compute",
                "sea_orm.",
            ],
        ),
        (
            "vignette crop",
            format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}/image"),
            &[
                "http.server.request",
                "media.crop",
                "media.decode",
                "media.encode",
                "sea_orm.",
            ],
        ),
    ];
    for (seed, (route, uri, expected)) in (0x21u8..).zip(reads) {
        harness.reset();
        let incoming = Incoming::new(seed);
        let (status, _) = harness
            .send(
                Method::GET,
                &uri,
                "application/json",
                Vec::new(),
                Some(&incoming.header()),
            )
            .await;
        assert!(status.is_success(), "{route}: {status}");
        assert_one_trace(&harness, route, &incoming, expected);
    }

    // ── GraphQL ──────────────────────────────────────────────────────
    #[cfg(feature = "graphql")]
    {
        harness.reset();
        let incoming = Incoming::new(0x31);
        let query = format!(
            r#"query {{ personDetailBundle(treeId: "{tree_id}", personId: "{person_id}") {{ persons {{ id }} }} }}"#
        );
        let (status, _) = harness
            .send(
                Method::POST,
                "/graphql",
                "application/json",
                json!({ "query": query }).to_string().into_bytes(),
                Some(&incoming.header()),
            )
            .await;
        assert!(status.is_success(), "graphql: {status}");
        assert_one_trace(
            &harness,
            "graphql",
            &incoming,
            &[
                "http.server.request",
                "graphql.execute",
                "graphql.resolve",
                "person_detail.load",
                "sea_orm.",
            ],
        );
    }

    // ── A queued import continues the request's trace in the worker ──
    #[cfg(feature = "telemetry-context")]
    {
        harness.reset();
        let incoming = Incoming::new(0x41);
        let (status, _) = harness
            .send(
                Method::POST,
                &format!("/api/v1/trees/{tree_id}/import-jobs?format=gedcom"),
                "application/octet-stream",
                GEDCOM.as_bytes().to_vec(),
                Some(&incoming.header()),
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "import job");
        let worker = oxidgene_api::service::background_job::BackgroundJobWorker::new(
            harness.state.db.clone(),
            Arc::clone(&harness.state.profiles),
            Arc::clone(&harness.state.media),
            "trace-test-worker",
        );
        assert!(worker.run_once().await.expect("run import job"));
        // Claiming the job happens before the job's context is known: those
        // polling queries are the worker's own, parentless, and dropped by the
        // production sampler. Everything after the claim is the request's.
        harness
            .roots
            .lock()
            .expect("roots lock")
            .retain(|name| !name.starts_with("sea_orm."));
        assert_one_trace(
            &harness,
            "import job",
            &incoming,
            &[
                "http.server.request",
                "background_job.process",
                "import.job",
                "import.persist",
                "sea_orm.",
            ],
        );

        // ── Deleting the tree: the purge runs later, on the purge worker,
        // and continues the deleting request's trace.
        harness.reset();
        let incoming = Incoming::new(0x51);
        let (status, _) = harness
            .send(
                Method::DELETE,
                &format!("/api/v1/trees/{tree_id}"),
                "application/json",
                Vec::new(),
                Some(&incoming.header()),
            )
            .await;
        assert!(status.is_success(), "tree deletion: {status}");
        let mut purged = false;
        for _ in 0..400 {
            purged = harness
                .exporter
                .get_finished_spans()
                .expect("spans")
                .iter()
                .any(|span| span.name == "purge.tree");
            if purged {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(purged, "the purge never ran");
        assert_one_trace(
            &harness,
            "tree purge",
            &incoming,
            &["http.server.request", "purge.tree", "sea_orm."],
        );
    }
}
