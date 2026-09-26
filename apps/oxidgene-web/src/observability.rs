use std::fmt;
use std::time::Duration;

use futures_channel::mpsc::{UnboundedSender, unbounded};
use futures_util::StreamExt as _;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::transform::common::tonic::ResourceAttributesWithSchema;
use opentelemetry_proto::transform::trace::tonic::group_spans_by_resource_and_scope;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanProcessor};
use prost::Message as _;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{FilterExt as _, LevelFilter, filter_fn};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use wasm_bindgen::JsCast as _;

pub fn init(log_level: &str, endpoint: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let log_filter = tracing_subscriber::EnvFilter::try_new(log_level)?;
    let console = tracing_wasm::WASMLayer::new(tracing_wasm::WASMLayerConfigBuilder::new().build())
        .with_filter(log_filter.and(filter_fn(|metadata| metadata.is_event())));

    let Some(endpoint) = endpoint.filter(|endpoint| !endpoint.is_empty()) else {
        tracing_subscriber::registry().with(console).try_init()?;
        return Ok(());
    };

    global::set_text_map_propagator(TraceContextPropagator::new());
    let resource = Resource::builder()
        .with_service_name("oxidgene-web")
        .with_attribute(opentelemetry::KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .build();
    let processor = BrowserSpanProcessor::new(endpoint, &resource);
    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_span_processor(processor)
        .build();
    let tracer = provider.tracer("oxidgene-web");

    tracing_subscriber::registry()
        .with(console)
        // The application's own spans only, as the native builds export.
        // Unfiltered, every span of every dependency went out — each Dioxus
        // scope render, diff and task poll — which was most of the traffic and
        // a large share of the main thread while telemetry was on.
        .with(
            OpenTelemetryLayer::new(tracer).with_filter(filter_fn(|metadata| {
                metadata.is_span()
                    && LevelFilter::INFO >= *metadata.level()
                    && metadata.target().starts_with("oxidgene_")
            })),
        )
        .try_init()?;
    std::mem::forget(provider);
    Ok(())
}

/// How long ended spans gather before one export request carries them.
const EXPORT_DELAY_MS: i32 = 1_000;

/// Most spans in one export request.
const EXPORT_BATCH: usize = 512;

struct BrowserSpanProcessor {
    sender: UnboundedSender<SpanData>,
}

impl BrowserSpanProcessor {
    fn new(endpoint: &str, resource: &Resource) -> Self {
        let (sender, mut receiver) = unbounded::<SpanData>();
        let endpoint = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
        let resource = ResourceAttributesWithSchema::from(resource);
        wasm_bindgen_futures::spawn_local(async move {
            // One request per burst, not per span: a page load ends dozens of
            // spans within a few frames, and each request is a fetch plus a
            // protobuf encoding on the only thread the page has.
            let client = reqwest::Client::new();
            while let Some(first) = receiver.next().await {
                pause(EXPORT_DELAY_MS).await;
                let mut batch = vec![first];
                while batch.len() < EXPORT_BATCH
                    && let Ok(span) = receiver.try_recv()
                {
                    batch.push(span);
                }
                let request = ExportTraceServiceRequest {
                    resource_spans: group_spans_by_resource_and_scope(batch, &resource),
                };
                let _ = client
                    .post(&endpoint)
                    .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
                    .body(request.encode_to_vec())
                    .send()
                    .await;
            }
        });
        Self { sender }
    }
}

impl fmt::Debug for BrowserSpanProcessor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("BrowserSpanProcessor").finish()
    }
}

impl SpanProcessor for BrowserSpanProcessor {
    fn on_start(
        &self,
        _span: &mut opentelemetry_sdk::trace::Span,
        _context: &opentelemetry::Context,
    ) {
    }

    fn on_end(&self, span: SpanData) {
        let _ = self.sender.unbounded_send(span);
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: Duration) -> OTelSdkResult {
        self.sender.close_channel();
        Ok(())
    }
}

/// Resolve after `ms` milliseconds, through the page's own timer.
async fn pause(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let set_timeout = js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into())
            .ok()
            .and_then(|function| function.dyn_into::<js_sys::Function>().ok());
        match set_timeout {
            Some(set_timeout) => {
                let _ = set_timeout.call2(&wasm_bindgen::JsValue::NULL, &resolve, &ms.into());
            }
            None => {
                let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
            }
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}
