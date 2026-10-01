//! Shared tracing and OpenTelemetry initialization for native runtimes.

use std::env;
use std::error::Error;
use std::fmt;
use std::io::IsTerminal as _;
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::MatchedPath;
use axum::http::{HeaderMap, Method, Request, Response};
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
use opentelemetry_sdk::trace::{
    Sampler, SamplingDecision, SamplingResult, SdkTracerProvider, ShouldSample,
};
use tracing::{Span, field};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use tracing_subscriber::filter::{FilterExt as _, LevelFilter, filter_fn};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer, Registry};

const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// How console log lines are written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable lines, coloured only when written to a terminal.
    #[default]
    Text,
    /// One JSON object per event, for log collectors.
    Json,
}

/// A log format other than `text` or `json`.
///
/// Its message deliberately omits the rejected value: configuration failures
/// report a stable category, never what was supplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLogFormat;

impl fmt::Display for InvalidLogFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid log format: expected text or json")
    }
}

impl Error for InvalidLogFormat {}

impl FromStr for LogFormat {
    type Err = InvalidLogFormat;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            _ => Err(InvalidLogFormat),
        }
    }
}

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

/// The console output of a runtime: where its lines go and how they look.
struct Console<W> {
    writer: W,
    format: LogFormat,
    /// Whether the destination is a terminal, the only place ANSI colour
    /// codes are wanted: a container runtime or a file stores them verbatim.
    terminal: bool,
}

/// Install structured logging and optional OTLP trace and metric exporters.
///
/// Logs are written to standard output in `log_format`. OTLP export is
/// enabled only when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
pub fn init(
    service_name: &'static str,
    service_version: &'static str,
    log_filter: &str,
    log_format: LogFormat,
) -> Result<TelemetryGuard, Box<dyn Error + Send + Sync>> {
    let console = Console {
        writer: std::io::stdout,
        format: log_format,
        terminal: std::io::stdout().is_terminal(),
    };
    install(service_name, service_version, log_filter, console)
}

/// [`init`], with logs written to standard error.
///
/// For a process whose standard output is a protocol stream, such as an MCP
/// server over stdio, where a single log line would corrupt the session.
pub fn init_to_stderr(
    service_name: &'static str,
    service_version: &'static str,
    log_filter: &str,
    log_format: LogFormat,
) -> Result<TelemetryGuard, Box<dyn Error + Send + Sync>> {
    let console = Console {
        writer: std::io::stderr,
        format: log_format,
        terminal: std::io::stderr().is_terminal(),
    };
    install(service_name, service_version, log_filter, console)
}

/// The console layer: log events only, never spans, in the chosen format.
///
/// Spans stay off the console so that, with OTLP export disabled, no layer
/// enables a span callsite and spans cost nothing. A JSON line therefore
/// carries the event's own fields and no span context; an event that needs
/// request or job context records it as fields of its own.
fn console_layer<W>(
    console: Console<W>,
    filter: EnvFilter,
) -> Box<dyn Layer<Registry> + Send + Sync>
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let events_only = filter.and(filter_fn(|metadata| metadata.is_event()));
    match console.format {
        LogFormat::Text => tracing_subscriber::fmt::layer()
            .with_writer(console.writer)
            .with_ansi(console.terminal)
            .with_filter(events_only)
            .boxed(),
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .with_span_list(false)
            .with_target(true)
            .with_ansi(false)
            .with_writer(console.writer)
            .with_filter(events_only)
            .boxed(),
    }
}

fn install<W>(
    service_name: &'static str,
    service_version: &'static str,
    log_filter: &str,
    console: Console<W>,
) -> Result<TelemetryGuard, Box<dyn Error + Send + Sync>>
where
    W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static,
{
    let console = console_layer(console, runtime_filter(log_filter)?);
    let Some(endpoint) = env::var_os(OTLP_ENDPOINT_ENV).filter(|value| !value.is_empty()) else {
        tracing_subscriber::registry().with(console).try_init()?;
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
        .with_sampler(sampler())
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
        .with(console)
        .with(
            OpenTelemetryLayer::new(tracer)
                .with_filter(filter_fn(export_span).and(LevelFilter::INFO)),
        )
        .with(
            log_layer.with_filter(runtime_filter(log_filter)?.and(filter_fn(|metadata| {
                metadata.is_event() && export_log_target(metadata.target())
            }))),
        )
        .with(connection_wait_layer(connection_wait_histogram()))
        .try_init()?;

    tracing::info!(transport = "grpc", "OpenTelemetry export enabled");

    Ok(TelemetryGuard {
        logger_provider: Some(logger_provider),
        tracer_provider: Some(tracer_provider),
        meter_provider: Some(meter_provider),
        runtime,
    })
}

/// The target of the connection-pool timing events SQLx emits once a
/// connection is acquired, when the pool is configured to (see
/// `oxidgene_db::repo::connect`).
const POOL_ACQUIRE_TARGET: &str = "sqlx::pool::acquire";

/// The field of those events holding the time spent acquiring, in seconds.
const ACQUIRED_AFTER_FIELD: &str = "acquired_after_secs";

/// Records how long each database call waited for a pooled connection.
///
/// SeaORM's spans include the wait — the pool is acquired inside them — but
/// cannot tell it apart from the statement, and a transaction acquires its
/// connection before its `sea_orm.begin` span opens. The SQLite pool holds a
/// single connection, so waiting for it is the main source of latency under
/// concurrent requests. SQLx measures every acquisition itself; this layer
/// turns those measurements into the `db.client.connection.wait_time`
/// histogram and nothing else (no log line, no span).
struct ConnectionWait<R> {
    record: R,
}

impl<S, R> Layer<S> for ConnectionWait<R>
where
    S: tracing::Subscriber,
    R: Fn(f64) + Send + Sync + 'static,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut waited = AcquiredAfter(None);
        event.record(&mut waited);
        if let Some(seconds) = waited.0 {
            (self.record)(seconds);
        }
    }
}

struct AcquiredAfter(Option<f64>);

impl field::Visit for AcquiredAfter {
    fn record_f64(&mut self, field: &field::Field, value: f64) {
        if field.name() == ACQUIRED_AFTER_FIELD {
            self.0 = Some(value);
        }
    }

    fn record_debug(&mut self, _field: &field::Field, _value: &dyn std::fmt::Debug) {}
}

/// [`ConnectionWait`], fed only the pool's timing events.
fn connection_wait_layer<S>(record: impl Fn(f64) + Send + Sync + 'static) -> impl Layer<S>
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    ConnectionWait { record }.with_filter(filter_fn(|metadata| {
        metadata.is_event() && metadata.target() == POOL_ACQUIRE_TARGET
    }))
}

fn connection_wait_histogram() -> impl Fn(f64) + Send + Sync + 'static {
    let histogram = global::meter("oxidgene")
        .f64_histogram("db.client.connection.wait_time")
        .with_unit("s")
        .with_description("Time spent waiting for a pooled database connection")
        .build();
    move |seconds| histogram.record(seconds, &[])
}

/// Every span follows its parent's decision; a span without a parent is kept
/// unless it is a database call.
fn sampler() -> Sampler {
    Sampler::ParentBased(Box::new(NoOrphanDatabaseCalls))
}

/// Drops the traces a database call would start on its own.
///
/// SeaORM spans are leaves under an operation's span. Without one — a
/// background loop polling for work, a startup query — each call would be a
/// trace by itself: one per second per job worker, for instance, desktop
/// included. Such calls are recorded under a span of their own where they
/// matter (`purge.tree`, `history.baselines`, `startup.migrate`), and the
/// remaining ones are dropped here.
#[derive(Debug, Clone)]
struct NoOrphanDatabaseCalls;

impl ShouldSample for NoOrphanDatabaseCalls {
    fn should_sample(
        &self,
        _parent_context: Option<&opentelemetry::Context>,
        _trace_id: opentelemetry::trace::TraceId,
        name: &str,
        _span_kind: &opentelemetry::trace::SpanKind,
        _attributes: &[KeyValue],
        _links: &[opentelemetry::trace::Link],
    ) -> SamplingResult {
        let decision = if name.starts_with("sea_orm.") {
            SamplingDecision::Drop
        } else {
            SamplingDecision::RecordAndSample
        };
        SamplingResult {
            decision,
            attributes: Vec::new(),
            trace_state: opentelemetry::trace::TraceState::default(),
        }
    }
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

/// Whether the OTLP log bridge forwards an event from `target`.
///
/// The exporter's own transport (gRPC over HTTP/2) and the OpenTelemetry SDK
/// log through `tracing` too. Bridged, every export would emit events that
/// are exported in turn, feeding the pipeline from itself whenever the filter
/// admits their level (`debug`, say). The console still shows them.
///
/// The bridge also takes events only: it exports log records, and a span
/// enabled for it alone would be created for every crate at the filter's
/// level and then ignored.
fn export_log_target(target: &str) -> bool {
    let krate = target.split("::").next().unwrap_or(target);
    !matches!(
        krate,
        "h2" | "hyper" | "hyper_util" | "tonic" | "tower" | "reqwest"
    ) && !krate.starts_with("opentelemetry")
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
    let method = http_method(Some(request.method()));
    let route = http_route(request.extensions().get::<MatchedPath>());
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

/// The method as a bounded label: a standard method, or `_OTHER` for an
/// extension method a client invented (the OpenTelemetry convention).
fn http_method(method: Option<&Method>) -> &'static str {
    static KNOWN: [Method; 9] = [
        Method::GET,
        Method::HEAD,
        Method::POST,
        Method::PUT,
        Method::DELETE,
        Method::CONNECT,
        Method::OPTIONS,
        Method::TRACE,
        Method::PATCH,
    ];
    method
        .and_then(|method| KNOWN.iter().find(|known| *known == method))
        .map_or("_OTHER", Method::as_str)
}

/// The route template, or `unmatched` for a path no route answers: the raw
/// path may carry identifiers and is never a label.
fn http_route(route: Option<&MatchedPath>) -> &str {
    route.map_or("unmatched", MatchedPath::as_str)
}

/// The labels of the request-duration metric: method, route template, and
/// status, all bounded.
///
/// The response carries the method and route because the request is gone by
/// the time it is answered; the router's request-context middleware copies
/// them onto it (`oxidgene_api::request_context`). A response that never
/// reached a route — a refused or unmatched request — has neither.
fn http_duration_labels<B>(response: &Response<B>) -> [KeyValue; 3] {
    let extensions = response.extensions();
    [
        KeyValue::new(
            "http.request.method",
            http_method(extensions.get::<Method>()),
        ),
        KeyValue::new(
            "http.route",
            http_route(extensions.get::<MatchedPath>()).to_owned(),
        ),
        KeyValue::new(
            "http.response.status_code",
            i64::from(response.status().as_u16()),
        ),
    ]
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
            .with_unit("s")
            .with_description("Duration of inbound HTTP requests in seconds")
            .build()
    });
    duration.record(latency.as_secs_f64(), &http_duration_labels(response));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

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

    #[test]
    fn log_bridge_leaves_out_the_exporter_transport() {
        for target in [
            "oxidgene_api::service::background_job",
            "sea_orm::database::db_connection",
            "tower_http::trace::on_failure",
            "hyperlocal",
        ] {
            assert!(export_log_target(target), "expected {target} to be bridged");
        }
        for target in [
            "h2::codec::framed_write",
            "hyper::proto::h1::conn",
            "hyper_util::client::legacy",
            "tonic::transport::channel",
            "tower::buffer::worker",
            "reqwest::connect",
            "opentelemetry_sdk::logs",
            "opentelemetry-otlp",
            "opentelemetry",
        ] {
            assert!(
                !export_log_target(target),
                "expected {target} to stay out of the log export"
            );
        }
    }

    #[test]
    fn log_format_parses_its_two_names_and_rejects_the_rest() {
        assert_eq!("text".parse(), Ok(LogFormat::Text));
        assert_eq!("json".parse(), Ok(LogFormat::Json));
        assert_eq!(LogFormat::default(), LogFormat::Text);
        let rejected = "private-value".parse::<LogFormat>();
        assert_eq!(rejected, Err(InvalidLogFormat));
        assert!(
            !InvalidLogFormat.to_string().contains("private-value"),
            "the rejected value must not be echoed"
        );
    }

    /// Collects what a console layer writes.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("capture lock")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for Captured {
        type Writer = Self;

        fn make_writer(&'writer self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn json_console_writes_flat_events_without_span_context() {
        let captured = Captured::default();
        let console = Console {
            writer: captured.clone(),
            format: LogFormat::Json,
            terminal: true,
        };
        let filter = EnvFilter::try_new("info").expect("valid filter");
        let subscriber = tracing_subscriber::registry().with(console_layer(console, filter));

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("background_job.process", job.kind = "import");
            span.in_scope(|| {
                tracing::error!(
                    job.kind = "import",
                    error.kind = "io",
                    "background job failed"
                );
            });
        });

        let output = String::from_utf8(captured.0.lock().expect("capture lock").clone())
            .expect("UTF-8 output");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1, "one line per event, none per span");
        let line: serde_json::Value = serde_json::from_str(lines[0]).expect("a JSON line");
        let object = line.as_object().expect("a JSON object");
        for key in [
            "timestamp",
            "level",
            "target",
            "message",
            "job.kind",
            "error.kind",
        ] {
            assert!(object.contains_key(key), "expected the {key} key in {line}");
        }
        assert_eq!(object["message"], "background job failed");
        for key in ["span", "spans", "fields"] {
            assert!(!object.contains_key(key), "unexpected {key} key in {line}");
        }
        assert!(!lines[0].contains('\u{1b}'), "no ANSI escape in JSON");
    }

    #[test]
    fn text_console_writes_no_ansi_escapes_off_a_terminal() {
        let captured = Captured::default();
        let console = Console {
            writer: captured.clone(),
            format: LogFormat::Text,
            terminal: false,
        };
        let filter = EnvFilter::try_new("info").expect("valid filter");
        let subscriber = tracing_subscriber::registry().with(console_layer(console, filter));

        tracing::subscriber::with_default(subscriber, || tracing::warn!("degraded"));

        let output = String::from_utf8(captured.0.lock().expect("capture lock").clone())
            .expect("UTF-8 output");
        assert!(output.contains("degraded"));
        assert!(
            !output.contains('\u{1b}'),
            "unexpected ANSI escape: {output:?}"
        );
    }

    fn decision(parent: Option<&opentelemetry::Context>, name: &str) -> SamplingDecision {
        sampler()
            .should_sample(
                parent,
                opentelemetry::trace::TraceId::from_bytes([1; 16]),
                name,
                &opentelemetry::trace::SpanKind::Internal,
                &[],
                &[],
            )
            .decision
    }

    #[test]
    fn database_calls_start_no_trace_of_their_own() {
        use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState};

        assert_eq!(decision(None, "sea_orm.query_all"), SamplingDecision::Drop);
        assert_eq!(decision(None, "sea_orm.begin"), SamplingDecision::Drop);
        for root in [
            "purge.tree",
            "http.server.request",
            "background_job.process",
        ] {
            assert_eq!(decision(None, root), SamplingDecision::RecordAndSample);
        }

        let parent = opentelemetry::Context::new().with_remote_span_context(SpanContext::new(
            TraceId::from_bytes([7; 16]),
            SpanId::from_bytes([7; 8]),
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        ));
        assert_eq!(
            decision(Some(&parent), "sea_orm.query_all"),
            SamplingDecision::RecordAndSample
        );
    }

    #[test]
    fn connection_waits_are_read_from_the_pool_timing_events() {
        let waits = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&waits);
        let subscriber =
            tracing_subscriber::registry().with(connection_wait_layer(move |seconds| {
                recorded.lock().expect("waits lock").push(seconds)
            }));

        tracing::subscriber::with_default(subscriber, || {
            pool_acquired();
            pool_acquired_slowly();
            unrelated_event();
        });

        assert_eq!(*waits.lock().expect("waits lock"), vec![0.25, 3.0]);
    }

    /// What SQLx emits for an acquisition at the configured timing level.
    fn pool_acquired() {
        tracing::trace!(
            target: "sqlx::pool::acquire",
            acquired_after_secs = 0.25,
            "acquired connection"
        );
    }

    /// What SQLx emits for an acquisition over its slow threshold.
    fn pool_acquired_slowly() {
        tracing::warn!(
            target: "sqlx::pool::acquire",
            acquired_after_secs = 3.0,
            slow_acquire_threshold_secs = 2.0,
            "acquired connection, but time to acquire exceeded slow threshold"
        );
    }

    fn unrelated_event() {
        tracing::info!(acquired_after_secs = 9.0, "not the pool");
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

    #[tokio::test]
    async fn request_duration_labels_are_bounded() {
        use axum::Router;
        use axum::body::Body;
        use axum::middleware::{self, Next};
        use axum::routing::get;
        use tower::ServiceExt as _;

        // What the API router's request-context middleware does.
        async fn copy_route(request: Request<Body>, next: Next) -> axum::response::Response {
            let method = request.method().clone();
            let route = request.extensions().get::<MatchedPath>().cloned();
            let mut response = next.run(request).await;
            response.extensions_mut().insert(method);
            if let Some(route) = route {
                response.extensions_mut().insert(route);
            }
            response
        }
        let app = Router::new()
            .route("/trees/{tree_id}", get(|| async { "ok" }))
            .layer(middleware::from_fn(copy_route));
        let request = |method: &str, uri: &str| {
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("valid request")
        };

        let routed = app
            .clone()
            .oneshot(request("GET", "/trees/private-value"))
            .await
            .expect("infallible router");
        let labels = http_duration_labels(&routed);
        assert_eq!(labels[0].value.as_str(), "GET");
        assert_eq!(labels[1].value.as_str(), "/trees/{tree_id}");
        assert_eq!(labels[2].value, opentelemetry::Value::I64(200));

        let invented = app
            .oneshot(request("FROBNICATE", "/trees/private-value"))
            .await
            .expect("infallible router");
        assert_eq!(http_duration_labels(&invented)[0].value.as_str(), "_OTHER");

        let unrouted = Response::builder()
            .status(404)
            .body(())
            .expect("valid response");
        let labels = http_duration_labels(&unrouted);
        assert_eq!(labels[0].value.as_str(), "_OTHER");
        assert_eq!(labels[1].value.as_str(), "unmatched");
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
