use std::future::Future;

use dioxus::prelude::*;

#[cfg(feature = "telemetry-client")]
use std::sync::{Arc, Mutex};

#[cfg(feature = "telemetry-client")]
use tracing::Instrument as _;

#[derive(Clone, Copy)]
pub enum UiPage {
    Home,
    Pedigree,
    PersonDetail,
    PersonHistory,
    CoupleDetail,
    SearchResults,
    Dictionary,
    Kinship,
    Settings,
    Statistics,
    Tools,
    AppSettings,
    NotFound,
    Component,
}

/// A user-initiated operation that owns a trace of its own.
///
/// A page load is bounded by its resources; these are bounded by a button. They
/// outlive the render that started them and must not be filed under whichever
/// screen happened to be loading, so each one opens a root span of its own.
#[derive(Clone, Copy)]
pub enum UiAction {
    /// A file import, named by the reader its extension picked.
    Import(&'static str),
    /// The Geneanet wizard. Every step the user drives is its own root: they
    /// are separated by however long the person spends reading the screen.
    GeneanetImport,
    /// An export, named by the artifact the user asked for.
    Export(&'static str),
    /// One of the operations a single button starts and finishes.
    Command(UiCommand),
}

impl From<UiCommand> for UiAction {
    fn from(command: UiCommand) -> Self {
        Self::Command(command)
    }
}

/// An operation a single button starts and finishes — mostly a write —
/// traced as a root of its own rather than as a bare HTTP request.
#[derive(Clone, Copy)]
pub enum UiCommand {
    /// Deleting a whole tree.
    DeleteTree,
    /// Copying a whole tree under a new name.
    DuplicateTree,
    /// Merging one person into another.
    Merge,
    /// Recording two persons as distinct, so they are no longer offered as
    /// duplicates.
    MarkDistinct,
    /// Restoring an earlier version of a record.
    Restore,
    /// Attaching a media to a person, a couple or an event, or detaching it
    /// from an event.
    MediaLink,
    /// Printing the page or the chart.
    Print,
}

#[derive(Clone, Copy)]
pub enum UiActionStep {
    ImportUpload,
    ImportPoll,
    GeneanetRead,
    GeneanetWrite,
    GeneanetInspect,
    GeneanetIndex,
    GeneanetConnect,
    GeneanetPreview,
    GeneanetCollect,
    GeneanetUpload,
    GeneanetPoll,
    GeneanetSessionEncode,
    GeneanetSessionDecode,
    GeneanetHomonyms,
    ExportRequest,
    ExportQueue,
    ExportPoll,
    ExportSave,
    PrintMeasure,
    PrintPrepare,
    PrintDialog,
}

#[cfg(feature = "telemetry-client")]
#[derive(Default)]
struct LoadState {
    root: Option<tracing::Span>,
    active_resources: usize,
    cycle: u64,
    stabilization: u64,
}

#[derive(Clone)]
pub struct UiLoadTrace {
    #[cfg(feature = "telemetry-client")]
    page: UiPage,
    #[cfg(feature = "telemetry-client")]
    state: Arc<Mutex<LoadState>>,
}

#[cfg(feature = "telemetry-client")]
struct ResourceCompletion {
    trace: UiLoadTrace,
    root: Option<tracing::Span>,
    cycle: u64,
}

#[cfg(feature = "telemetry-client")]
impl ResourceCompletion {
    fn new(trace: UiLoadTrace, root: tracing::Span, cycle: u64) -> Self {
        Self {
            trace,
            root: Some(root),
            cycle,
        }
    }

    fn finish(mut self) {
        if let Some(root) = self.root.take() {
            self.trace.finish_resource(root, self.cycle);
        }
    }
}

#[cfg(feature = "telemetry-client")]
impl Drop for ResourceCompletion {
    fn drop(&mut self) {
        if self.root.take().is_some() {
            self.trace.cancel_resource(self.cycle);
        }
    }
}

impl UiLoadTrace {
    #[must_use]
    pub fn new(page: UiPage) -> Self {
        #[cfg(not(feature = "telemetry-client"))]
        let _ = page;
        Self {
            #[cfg(feature = "telemetry-client")]
            page,
            #[cfg(feature = "telemetry-client")]
            state: Arc::new(Mutex::new(LoadState::default())),
        }
    }

    pub async fn resource<T>(&self, name: &'static str, future: impl Future<Output = T>) -> T {
        #[cfg(feature = "telemetry-client")]
        {
            let (root, cycle) = self.begin_resource();
            let completion = ResourceCompletion::new(self.clone(), root.clone(), cycle);
            // The future is polled inside the resource span, so a request it
            // sends opens its client span as a child of the page's root; the
            // `instrument` wrapper re-enters it on every poll, which keeps that
            // true on the browser's single-threaded executor too.
            let output = future.instrument(resource_span(&root, name)).await;
            completion.finish();
            output
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            let _ = name;
            future.await
        }
    }

    #[cfg(feature = "telemetry-client")]
    fn begin_resource(&self) -> (tracing::Span, u64) {
        let mut state = self.state.lock().expect("UI trace state poisoned");
        if state.root.is_none() {
            state.cycle = state.cycle.wrapping_add(1);
            state.root = Some(page_span(self.page));
        }
        state.active_resources += 1;
        (
            state.root.as_ref().expect("UI root span missing").clone(),
            state.cycle,
        )
    }

    #[cfg(feature = "telemetry-client")]
    fn finish_resource(&self, root: tracing::Span, cycle: u64) {
        let stabilization = {
            let mut state = self.state.lock().expect("UI trace state poisoned");
            state.active_resources = state.active_resources.saturating_sub(1);
            if state.active_resources != 0 || state.cycle != cycle {
                return;
            }
            state.stabilization = state.stabilization.wrapping_add(1);
            state.stabilization
        };

        let state = self.state.clone();
        spawn(async move {
            let stabilize = tracing::info_span!(
                parent: &root,
                "ui.render.stabilize",
                otel.name = "wait for render stabilization",
                ui.render.frames = 2,
                ui.render.reason = "resource_cycle_complete",
            );
            wait_for_render().instrument(stabilize).await;

            let mut state = state.lock().expect("UI trace state poisoned");
            if state.active_resources == 0
                && state.cycle == cycle
                && state.stabilization == stabilization
            {
                state.root.take();
            }
        });
    }

    #[cfg(feature = "telemetry-client")]
    fn cancel_resource(&self, cycle: u64) {
        let mut state = self.state.lock().expect("UI trace state poisoned");
        if state.cycle != cycle {
            return;
        }
        state.active_resources = state.active_resources.saturating_sub(1);
        if state.active_resources == 0 {
            state.stabilization = state.stabilization.wrapping_add(1);
            state.root.take();
        }
    }

    /// Run `operation` as a `ui.compute` span of the load in progress.
    ///
    /// Outside a load the span goes under the operation running, if any — an
    /// action, say. With neither there is nothing to file it under, and a
    /// compute span of its own would only be an orphan root, so none is
    /// opened.
    pub fn measure<T>(&self, name: &'static str, operation: impl FnOnce() -> T) -> T {
        #[cfg(feature = "telemetry-client")]
        {
            let root = {
                let state = self.state.lock().expect("UI trace state poisoned");
                state.root.clone()
            };
            let span = match root {
                Some(root) => tracing::info_span!(
                    parent: &root,
                    "ui.compute",
                    otel.name = name,
                    ui.compute.name = name,
                ),
                None if !tracing::Span::current().is_none() => {
                    tracing::info_span!("ui.compute", otel.name = name, ui.compute.name = name,)
                }
                None => return operation(),
            };
            span.in_scope(operation)
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            let _ = name;
            operation()
        }
    }

    pub fn render_only(&self) {
        #[cfg(feature = "telemetry-client")]
        {
            let (root, cycle) = self.begin_resource();
            self.finish_resource(root, cycle);
        }
    }
}

pub fn use_ui_load_trace(page: UiPage) -> UiLoadTrace {
    let trace = use_context_provider(|| UiLoadTrace::new(page));
    use_effect({
        let trace = trace.clone();
        move || trace.render_only()
    });
    trace
}

pub fn use_traced_resource<T, F>(
    trace: UiLoadTrace,
    name: &'static str,
    mut future: impl FnMut() -> F + 'static,
) -> Resource<T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    use_resource(move || {
        let trace = trace.clone();
        let future = future();
        async move { trace.resource(name, future).await }
    })
}

pub fn use_ui_resource<T, F>(name: &'static str, future: impl FnMut() -> F + 'static) -> Resource<T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    let fallback = use_hook(|| UiLoadTrace::new(UiPage::Component));
    let trace = try_use_context::<UiLoadTrace>().unwrap_or(fallback);
    use_traced_resource(trace, name, future)
}

pub fn measure_ui<T>(name: &'static str, operation: impl FnOnce() -> T) -> T {
    match try_consume_context::<UiLoadTrace>() {
        Some(trace) => trace.measure(name, operation),
        None => operation(),
    }
}

/// A trace covering a multi-step operation the user drives by hand.
///
/// A page load is bounded by its resources and a single action by its future.
/// This one is bounded by the person finishing: an assistant they advance one
/// screen at a time. The root opens on the first step and stays open across
/// however long they spend between screens, so one import reads as one trace
/// rather than one per button — which is what a reader looking for "that
/// import" expects to find.
///
/// The cost of that is the root reaching the collector only when the operation
/// ends. Each step is exported as it completes and already carries the trace
/// id, so the waterfall fills in as the work happens and only the outermost
/// bar arrives last.
#[derive(Clone)]
pub struct UiActionTrace {
    #[cfg(feature = "telemetry-client")]
    action: UiAction,
    #[cfg(feature = "telemetry-client")]
    root: Arc<Mutex<Option<tracing::Span>>>,
}

impl UiActionTrace {
    #[must_use]
    pub fn new(action: UiAction) -> Self {
        #[cfg(not(feature = "telemetry-client"))]
        let _ = action;
        Self {
            #[cfg(feature = "telemetry-client")]
            action,
            #[cfg(feature = "telemetry-client")]
            root: Arc::new(Mutex::new(None)),
        }
    }

    /// Run one step of the operation as a child of its root.
    pub async fn step<T>(&self, step: UiActionStep, future: impl Future<Output = T>) -> T {
        #[cfg(feature = "telemetry-client")]
        {
            // Built inside the root so the root is its contextual parent: the
            // steps run in separate tasks with no ambient span of their own.
            let span = self.root().in_scope(|| action_step_span(step));
            future.instrument(span).await
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            let _ = step;
            future.await
        }
    }

    /// Close the trace. The operation is over, however it ended.
    pub fn finish(&self) {
        #[cfg(feature = "telemetry-client")]
        {
            self.root.lock().expect("UI action trace poisoned").take();
        }
    }

    #[cfg(feature = "telemetry-client")]
    fn root(&self) -> tracing::Span {
        let mut root = self.root.lock().expect("UI action trace poisoned");
        root.get_or_insert_with(|| action_span(self.action)).clone()
    }
}

/// Provide a trace covering every step of one multi-step operation.
///
/// Dropped with the component that owns the operation, so abandoning the
/// assistant closes the trace rather than leaving it open for the session.
pub fn use_ui_action_trace(action: UiAction) -> UiActionTrace {
    let trace = use_context_provider(|| UiActionTrace::new(action));
    use_drop({
        let trace = trace.clone();
        move || trace.finish()
    });
    trace
}

/// Run `future` under a root span for the whole operation.
pub async fn trace_ui_action<T>(action: impl Into<UiAction>, future: impl Future<Output = T>) -> T {
    let action = action.into();
    #[cfg(feature = "telemetry-client")]
    {
        future.instrument(action_span(action)).await
    }

    #[cfg(not(feature = "telemetry-client"))]
    {
        let _ = action;
        future.await
    }
}

/// Run `future` as one bounded phase of the surrounding action.
pub async fn trace_ui_action_step<T>(step: UiActionStep, future: impl Future<Output = T>) -> T {
    #[cfg(feature = "telemetry-client")]
    {
        future.instrument(action_step_span(step)).await
    }

    #[cfg(not(feature = "telemetry-client"))]
    {
        let _ = step;
        future.await
    }
}

/// One resource of a page load, a child of the page's root span.
#[cfg(feature = "telemetry-client")]
fn resource_span(root: &tracing::Span, name: &'static str) -> tracing::Span {
    tracing::info_span!(
        parent: root,
        "ui.resource.load",
        otel.name = name,
        ui.resource.name = name,
        otel.status_code = tracing::field::Empty,
    )
}

#[cfg(feature = "telemetry-client")]
fn action_span(action: UiAction) -> tracing::Span {
    match action {
        UiAction::Import(format) => {
            tracing::info_span!(parent: None, "ui.import", import.format = format)
        }
        UiAction::GeneanetImport => {
            tracing::info_span!(parent: None, "ui.geneanet_import", import.format = "geneanet")
        }
        UiAction::Export(format) => {
            tracing::info_span!(parent: None, "ui.export", export.format = format)
        }
        UiAction::Command(command) => command_span(command),
    }
}

/// Declares a function mapping each variant of a fieldless enum to an INFO
/// span with a fixed name.
///
/// `tracing` takes a span name only as a literal at its own call site, so the
/// name cannot come from a lookup table at run time: every variant needs its
/// own `info_span!`. This writes that `match` from a `variant => name` table.
/// A `root fn` opens each span as a root; a plain `fn` opens it under the
/// current span.
#[cfg(feature = "telemetry-client")]
macro_rules! span_names {
    (fn $function:ident($enum:ident) { $($variant:ident => $name:literal,)+ }) => {
        fn $function(value: $enum) -> tracing::Span {
            match value {
                $($enum::$variant => tracing::info_span!($name),)+
            }
        }
    };
    (root fn $function:ident($enum:ident) { $($variant:ident => $name:literal,)+ }) => {
        fn $function(value: $enum) -> tracing::Span {
            match value {
                $($enum::$variant => tracing::info_span!(parent: None, $name),)+
            }
        }
    };
}

#[cfg(feature = "telemetry-client")]
span_names! {
    fn action_step_span(UiActionStep) {
        ImportUpload => "ui.import.upload",
        ImportPoll => "ui.import.poll",
        GeneanetRead => "ui.geneanet_import.read",
        GeneanetWrite => "ui.geneanet_import.write",
        GeneanetInspect => "ui.geneanet_import.inspect",
        GeneanetIndex => "ui.geneanet_import.index",
        GeneanetConnect => "ui.geneanet_import.connect",
        GeneanetPreview => "ui.geneanet_import.preview",
        GeneanetCollect => "ui.geneanet_import.collect",
        GeneanetUpload => "ui.geneanet_import.upload",
        GeneanetPoll => "ui.geneanet_import.poll",
        GeneanetSessionEncode => "ui.geneanet_import.session_encode",
        GeneanetSessionDecode => "ui.geneanet_import.session_decode",
        GeneanetHomonyms => "ui.geneanet_import.homonyms",
        ExportRequest => "ui.export.request",
        ExportQueue => "ui.export.queue",
        ExportPoll => "ui.export.poll",
        ExportSave => "ui.export.save",
        PrintMeasure => "ui.print.measure",
        PrintPrepare => "ui.print.prepare",
        PrintDialog => "ui.print.dialog",
    }
}

#[cfg(feature = "telemetry-client")]
span_names! {
    root fn command_span(UiCommand) {
        DeleteTree => "ui.delete_tree",
        DuplicateTree => "ui.duplicate_tree",
        Merge => "ui.merge",
        MarkDistinct => "ui.mark_distinct",
        Restore => "ui.restore",
        MediaLink => "ui.media_link",
        Print => "ui.print",
    }
}

#[cfg(feature = "telemetry-client")]
span_names! {
    root fn page_span(UiPage) {
        Home => "ui.home.load",
        Pedigree => "ui.pedigree.load",
        PersonDetail => "ui.person_detail.load",
        PersonHistory => "ui.person_history.load",
        CoupleDetail => "ui.couple_detail.load",
        SearchResults => "ui.search_results.load",
        Dictionary => "ui.dictionary.load",
        Kinship => "ui.kinship.load",
        Settings => "ui.settings.load",
        Statistics => "ui.statistics.load",
        Tools => "ui.tools.load",
        AppSettings => "ui.app_settings.load",
        NotFound => "ui.not_found.load",
        Component => "ui.component.load",
    }
}

#[cfg(feature = "telemetry-client")]
async fn wait_for_render() {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dioxus::document::eval(
            "await new Promise(requestAnimationFrame); await new Promise(requestAnimationFrame)",
        )
        .await;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::utils::sleep_ms(16).await;
    }
}

#[cfg(all(test, feature = "telemetry-client"))]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::Subscriber;
    use tracing_subscriber::{
        Layer,
        layer::{Context, SubscriberExt as _},
        registry::LookupSpan,
    };

    use super::*;

    type CapturedSpan = (String, Option<String>);

    #[derive(Clone, Default)]
    struct CapturedSpans(Arc<Mutex<Vec<CapturedSpan>>>);

    impl<S> Layer<S> for CapturedSpans
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_new_span(
            &self,
            attributes: &tracing::span::Attributes<'_>,
            _id: &tracing::span::Id,
            context: Context<'_, S>,
        ) {
            let parent = if attributes.is_root() {
                None
            } else {
                attributes
                    .parent()
                    .and_then(|parent| context.span(parent))
                    .or_else(|| context.lookup_current())
                    .map(|span| span.metadata().name().to_string())
            };
            self.0
                .lock()
                .expect("capture lock")
                .push((attributes.metadata().name().to_string(), parent));
        }
    }

    #[test]
    fn every_ui_page_has_a_stable_root_name() {
        let subscriber = tracing_subscriber::registry();
        let _guard = tracing::subscriber::set_default(subscriber);
        let pages = [
            (UiPage::Home, "ui.home.load"),
            (UiPage::Pedigree, "ui.pedigree.load"),
            (UiPage::PersonDetail, "ui.person_detail.load"),
            (UiPage::PersonHistory, "ui.person_history.load"),
            (UiPage::CoupleDetail, "ui.couple_detail.load"),
            (UiPage::SearchResults, "ui.search_results.load"),
            (UiPage::Dictionary, "ui.dictionary.load"),
            (UiPage::Kinship, "ui.kinship.load"),
            (UiPage::Settings, "ui.settings.load"),
            (UiPage::Statistics, "ui.statistics.load"),
            (UiPage::Tools, "ui.tools.load"),
            (UiPage::AppSettings, "ui.app_settings.load"),
            (UiPage::NotFound, "ui.not_found.load"),
            (UiPage::Component, "ui.component.load"),
        ];

        for (page, expected) in pages {
            assert_eq!(
                page_span(page).metadata().expect("enabled span").name(),
                expected
            );
        }
    }

    #[test]
    fn a_compute_outside_a_load_joins_the_running_operation_or_opens_no_span() {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        let trace = UiLoadTrace::new(UiPage::Pedigree);

        trace.measure("pedigree_layout.tree", || ());
        assert!(captured.0.lock().expect("capture lock").is_empty());

        action_span(UiAction::Export("gedzip")).in_scope(|| {
            trace.measure("pedigree_layout.tree", || ());
        });
        let captured = captured.0.lock().expect("capture lock");
        assert!(
            captured.iter().any(
                |(name, parent)| name == "ui.compute" && parent.as_deref() == Some("ui.export")
            )
        );
    }

    #[test]
    fn cancelling_a_resource_releases_the_page_load_trace() {
        let trace = UiLoadTrace::new(UiPage::Pedigree);
        let (root, cycle) = trace.begin_resource();
        let completion = ResourceCompletion::new(trace.clone(), root, cycle);

        drop(completion);

        let state = trace.state.lock().expect("UI trace state poisoned");
        assert_eq!(state.active_resources, 0);
        assert!(state.root.is_none());
    }

    #[test]
    fn every_ui_action_has_a_stable_root_name() {
        let subscriber = tracing_subscriber::registry();
        let _guard = tracing::subscriber::set_default(subscriber);
        let actions = [
            (UiAction::Import("gedcom"), "ui.import"),
            (UiAction::GeneanetImport, "ui.geneanet_import"),
            (UiAction::Export("gedzip"), "ui.export"),
            (UiCommand::DeleteTree.into(), "ui.delete_tree"),
            (UiCommand::DuplicateTree.into(), "ui.duplicate_tree"),
            (UiCommand::Merge.into(), "ui.merge"),
            (UiCommand::MarkDistinct.into(), "ui.mark_distinct"),
            (UiCommand::Restore.into(), "ui.restore"),
            (UiCommand::MediaLink.into(), "ui.media_link"),
            (UiCommand::Print.into(), "ui.print"),
        ];

        for (action, expected) in actions {
            assert_eq!(
                action_span(action).metadata().expect("enabled span").name(),
                expected
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn action_phase_is_a_child_of_the_action() {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);

        trace_ui_action(
            UiAction::Import("gedcom"),
            trace_ui_action_step(UiActionStep::ImportPoll, async {}),
        )
        .await;

        assert!(
            captured
                .0
                .lock()
                .expect("capture lock")
                .iter()
                .any(|(name, parent)| name == "ui.import.poll"
                    && parent.as_deref() == Some("ui.import"))
        );
    }

    /// An action started from a screen that is still loading belongs to itself,
    /// not to that screen's load trace: it outlives the render that began it.
    #[tokio::test(flavor = "current_thread")]
    async fn an_action_started_during_a_page_load_is_still_a_root() {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);

        let page = page_span(UiPage::Settings);
        async {
            trace_ui_action(UiAction::Export("gedzip"), async {}).await;
            trace_ui_action(UiAction::GeneanetImport, async {}).await;
        }
        .instrument(page)
        .await;

        let captured = captured.0.lock().expect("capture lock");
        for root in ["ui.settings.load", "ui.export", "ui.geneanet_import"] {
            assert_eq!(
                captured
                    .iter()
                    .find(|(name, _)| name == root)
                    .map(|(_, parent)| parent.clone()),
                Some(None),
                "{root} should be a root span"
            );
        }
    }

    /// A page load's request leaves with the page's trace: its client span is
    /// a child of the resource span, itself a child of the page root, and the
    /// `traceparent` it sends names that client span in that trace — where the
    /// server picks the trace up.
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test(flavor = "current_thread")]
    async fn a_resource_request_carries_the_page_trace_to_the_backend() {
        use opentelemetry::trace::{SpanKind, TraceContextExt as _, TracerProvider as _};
        use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        use tracing_opentelemetry::OpenTelemetrySpanExt as _;

        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        let exporter = InMemorySpanExporter::default();
        let provider = SdkTracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();
        let subscriber = tracing_subscriber::registry().with(
            tracing_opentelemetry::OpenTelemetryLayer::new(provider.tracer("ui-test")),
        );
        let _guard = tracing::subscriber::set_default(subscriber);

        // A one-shot backend that records the request head it receives.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        let backend = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut head = Vec::new();
            let mut buffer = [0; 4096];
            while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = socket.read(&mut buffer).await.expect("read");
                if read == 0 {
                    break;
                }
                head.extend_from_slice(&buffer[..read]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                      Content-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .await
                .expect("write");
            String::from_utf8_lossy(&head).to_lowercase()
        });

        let trace = UiLoadTrace::new(UiPage::Pedigree);
        let (root, _cycle) = trace.begin_resource();
        let client = crate::api::ApiClient::new(&format!("http://{address}"));
        let _ = client
            .get_tree(uuid::Uuid::nil())
            .instrument(resource_span(&root, "tree"))
            .await;
        let head = backend.await.expect("backend");
        let page_trace = root.context().span().span_context().trace_id();
        drop(root);
        drop(trace);

        let spans = exporter.get_finished_spans().expect("spans");
        let find = |predicate: &dyn Fn(&opentelemetry_sdk::trace::SpanData) -> bool| {
            spans
                .iter()
                .find(|span| predicate(span))
                .unwrap_or_else(|| panic!("missing span among {spans:?}"))
        };
        let page = find(&|span| span.name == "ui.pedigree.load");
        let resource = find(&|span| span.name == "tree");
        let request = find(&|span| span.span_kind == SpanKind::Client);
        for span in [page, resource, request] {
            assert_eq!(span.span_context.trace_id(), page_trace, "{}", span.name);
        }
        assert_eq!(resource.parent_span_id, page.span_context.span_id());
        assert_eq!(request.parent_span_id, resource.span_context.span_id());
        assert!(
            head.contains(&format!(
                "traceparent: 00-{}-{}-01",
                page_trace,
                request.span_context.span_id()
            )),
            "the request did not carry its client span: {head}"
        );
    }
}
