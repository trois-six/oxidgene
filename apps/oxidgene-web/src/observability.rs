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
use tracing_subscriber::filter::{FilterExt as _, filter_fn};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

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
        .with(OpenTelemetryLayer::new(tracer).with_filter(filter_fn(|metadata| metadata.is_span())))
        .try_init()?;
    std::mem::forget(provider);
    Ok(())
}

struct BrowserSpanProcessor {
    sender: UnboundedSender<SpanData>,
}

impl BrowserSpanProcessor {
    fn new(endpoint: &str, resource: &Resource) -> Self {
        let (sender, mut receiver) = unbounded::<SpanData>();
        let endpoint = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
        let resource = ResourceAttributesWithSchema::from(resource);
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(span) = receiver.next().await {
                let request = ExportTraceServiceRequest {
                    resource_spans: group_spans_by_resource_and_scope(vec![span], &resource),
                };
                let _ = reqwest::Client::new()
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
