//! Shared tracing and OpenTelemetry initialization for native runtimes.

use std::env;
use std::error::Error;
use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::MatchedPath;
use axum::http::{HeaderMap, Request, Response};
use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::Histogram;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::{Span, field};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{FilterExt as _, LevelFilter, filter_fn};
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Serialize the current span as W3C Trace Context fields for durable transport.
#[must_use]
pub fn current_trace_context() -> (Option<String>, Option<String>) {
    let mut carrier = TraceContextCarrier::default();
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&Span::current().context(), &mut carrier);
    });
    (carrier.trace_parent, carrier.trace_state)
}

/// Restore W3C Trace Context fields as the parent of a tracing span.
pub fn set_parent_from_trace_context(
    span: &Span,
    trace_parent: Option<&str>,
    trace_state: Option<&str>,
) {
    let carrier = TraceContextCarrier {
        trace_parent: trace_parent.map(str::to_owned),
        trace_state: trace_state.map(str::to_owned),
    };
    let parent = global::get_text_map_propagator(|propagator| propagator.extract(&carrier));
    let _ = span.set_parent(parent);
}

#[derive(Default)]
struct TraceContextCarrier {
    trace_parent: Option<String>,
    trace_state: Option<String>,
}

impl Injector for TraceContextCarrier {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "traceparent" => self.trace_parent = Some(value),
            "tracestate" => self.trace_state = Some(value),
            _ => {}
        }
    }
}

impl Extractor for TraceContextCarrier {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "traceparent" => self.trace_parent.as_deref(),
            "tracestate" => self.trace_state.as_deref(),
            _ => None,
        }
    }

    fn keys(&self) -> Vec<&str> {
        let mut keys = Vec::with_capacity(2);
        if self.trace_parent.is_some() {
            keys.push("traceparent");
        }
        if self.trace_state.is_some() {
            keys.push("tracestate");
        }
        keys
    }
}

/// Keeps OpenTelemetry providers alive and flushes them when the runtime stops.
pub struct TelemetryGuard {
    logger_provider: Option<SdkLoggerProvider>,
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl TelemetryGuard {
    /// Flush queued telemetry before process termination.
    pub fn shutdown(self) {
        if let Some(provider) = self.logger_provider {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.meter_provider {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.tracer_provider {
            let _ = provider.shutdown();
        }
        if let Some(runtime) = self.runtime {
            runtime.shutdown_background();
        }
    }
}

/// Install structured logging and optional OTLP trace and metric exporters.
///
/// OTLP export is enabled only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
pub fn init(
    service_name: &'static str,
    service_version: &'static str,
    log_filter: &str,
) -> Result<TelemetryGuard, Box<dyn Error + Send + Sync>> {
    let Some(endpoint) = env::var_os(OTLP_ENDPOINT_ENV).filter(|value| !value.is_empty()) else {
        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_filter(
                runtime_filter(log_filter)?.and(filter_fn(|metadata| metadata.is_event())),
            ))
            .try_init()?;
        return Ok(TelemetryGuard {
            logger_provider: None,
            tracer_provider: None,
            meter_provider: None,
            runtime: None,
        });
    };
    let endpoint = endpoint
        .into_string()
        .map_err(|_| "OTEL_EXPORTER_OTLP_ENDPOINT must be valid UTF-8")?;
    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attribute(opentelemetry::KeyValue::new(
            "service.version",
            service_version,
        ))
        .build();

    let runtime = if tokio::runtime::Handle::try_current().is_err() {
        Some(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?,
        )
    } else {
        None
    };
    let _runtime_context = runtime.as_ref().map(tokio::runtime::Runtime::enter);

    global::set_text_map_propagator(TraceContextPropagator::new());

    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.clone())
        .build()?;
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
        .build();
    let tracer = tracer_provider.tracer(service_name);

    let metric_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.clone())
        .build()?;
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());

    let log_exporter = LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(log_exporter)
        .build();
    let log_layer = OpenTelemetryTracingBridge::new(&logger_provider);

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(
            runtime_filter(log_filter)?.and(filter_fn(|metadata| metadata.is_event())),
        ))
        .with(
            OpenTelemetryLayer::new(tracer)
                .with_filter(filter_fn(export_span).and(LevelFilter::INFO)),
        )
        .with(log_layer.with_filter(runtime_filter(log_filter)?))
        .try_init()?;

    tracing::info!(transport = "grpc", "OpenTelemetry export enabled");

    Ok(TelemetryGuard {
        logger_provider: Some(logger_provider),
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        runtime,
    })
}

fn runtime_filter(log_filter: &str) -> Result<EnvFilter, tracing_subscriber::filter::ParseError> {
    EnvFilter::try_new(log_filter)
}

fn export_span(metadata: &tracing::Metadata<'_>) -> bool {
    metadata.is_span() && export_span_target(metadata.target())
}

fn export_span_target(target: &str) -> bool {
    target.starts_with("oxidgene_") || target == "sea_orm" || target.starts_with("sea_orm::")
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}

/// Create a server span without recording raw URIs or query strings.
pub fn make_http_span<B>(request: &Request<B>) -> Span {
    let method = request.method().as_str();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched");
    let span = tracing::info_span!(
        "http.server.request",
        otel.name = %format_args!("{method} {route}"),
        otel.kind = "server",
        http.request.method = method,
        http.route = route,
        http.response.status_code = field::Empty,
        otel.status_code = field::Empty,
    );
    let parent = global::get_text_map_propagator(|propagator| {
        propagator.extract(&HeaderExtractor(request.headers()))
    });
    let _ = span.set_parent(parent);
    span
}

/// Complete an HTTP span and record an aggregate request-duration metric.
pub fn on_http_response<B>(response: &Response<B>, latency: Duration, span: &Span) {
    let status = response.status();
    span.record("http.response.status_code", status.as_u16());
    if status.is_server_error() {
        span.record("otel.status_code", "ERROR");
    }

    static DURATION: OnceLock<Histogram<f64>> = OnceLock::new();
    let duration = DURATION.get_or_init(|| {
        global::meter("oxidgene")
            .f64_histogram("http.server.request.duration")
            .with_description("Duration of inbound HTTP requests in seconds")
            .build()
    });
    duration.record(
        latency.as_secs_f64(),
        &[KeyValue::new(
            "http.response.status_code",
            i64::from(status.as_u16()),
        )],
    );
}

#[cfg(test)]
mod tests {
    use opentelemetry::trace::TraceContextExt as _;

    use super::*;

    #[test]
    fn span_export_keeps_application_boundaries_without_runtime_internals() {
        for target in [
            "oxidgene_ui::ui_observability",
            "oxidgene_api::service::tree",
            "oxidgene_observability",
            "sea_orm::driver::sqlx_sqlite",
        ] {
            assert!(
                export_span_target(target),
                "expected {target} to be exported"
            );
        }

        for target in [
            "tokio_util::codec::framed_write",
            "h2::codec::framed_write",
            "hyper::proto::h1",
            "sqlx_core::pool::connection",
        ] {
            assert!(
                !export_span_target(target),
                "expected {target} to be excluded"
            );
        }
    }

    #[tokio::test]
    async fn telemetry_runtime_can_stop_inside_an_async_context() {
        let guard = TelemetryGuard {
            logger_provider: None,
            tracer_provider: None,
            meter_provider: None,
            runtime: Some(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("telemetry runtime"),
            ),
        };

        guard.shutdown();
    }

    #[test]
    fn trace_context_round_trips_as_a_remote_parent() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryLayer::new(provider.tracer("test")));

        tracing::subscriber::with_default(subscriber, || {
            let source = tracing::info_span!("source");
            let (trace_parent, trace_state) = source.in_scope(current_trace_context);
            let trace_parent = trace_parent.expect("active span should produce traceparent");
            assert_eq!(trace_parent.len(), 55);
            assert!(trace_parent.starts_with("00-"));

            let child = tracing::info_span!("child");
            set_parent_from_trace_context(&child, Some(&trace_parent), trace_state.as_deref());
            let child_context = child.context();
            let child_span = child_context.span();
            assert_eq!(
                child_span.span_context().trace_id().to_string(),
                trace_parent[3..35]
            );
        });
    }

    #[test]
    fn http_span_continues_the_incoming_trace() {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let provider = SdkTracerProvider::builder().build();
        let subscriber =
            tracing_subscriber::registry().with(OpenTelemetryLayer::new(provider.tracer("test")));

        tracing::subscriber::with_default(subscriber, || {
            let request = Request::builder()
                .method("GET")
                .uri("/api/v1/trees/private-value")
                .header(
                    "traceparent",
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                )
                .body(())
                .expect("valid request");

            let span = make_http_span(&request);
            let context = span.context();
            assert_eq!(
                context.span().span_context().trace_id().to_string(),
                "4bf92f3577b34da6a3ce929d0e0e4736"
            );
        });
    }
}
