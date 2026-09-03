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
    SearchResults,
    Dictionary,
    Settings,
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
    ExportRequest,
    ExportQueue,
    ExportPoll,
    ExportSave,
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
            let span = tracing::info_span!(
                parent: &root,
                "ui.resource.load",
                otel.name = name,
                ui.resource.name = name,
                otel.status_code = tracing::field::Empty,
            );
            let output = future.instrument(span).await;
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
                None => {
                    tracing::info_span!("ui.compute", otel.name = name, ui.compute.name = name,)
                }
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
pub async fn trace_ui_action<T>(action: UiAction, future: impl Future<Output = T>) -> T {
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
    }
}

#[cfg(feature = "telemetry-client")]
fn action_step_span(step: UiActionStep) -> tracing::Span {
    match step {
        UiActionStep::ImportUpload => tracing::info_span!("ui.import.upload"),
        UiActionStep::ImportPoll => tracing::info_span!("ui.import.poll"),
        UiActionStep::GeneanetRead => tracing::info_span!("ui.geneanet_import.read"),
        UiActionStep::GeneanetWrite => tracing::info_span!("ui.geneanet_import.write"),
        UiActionStep::GeneanetInspect => tracing::info_span!("ui.geneanet_import.inspect"),
        UiActionStep::GeneanetIndex => tracing::info_span!("ui.geneanet_import.index"),
        UiActionStep::GeneanetConnect => tracing::info_span!("ui.geneanet_import.connect"),
        UiActionStep::GeneanetPreview => tracing::info_span!("ui.geneanet_import.preview"),
        UiActionStep::GeneanetCollect => tracing::info_span!("ui.geneanet_import.collect"),
        UiActionStep::GeneanetUpload => tracing::info_span!("ui.geneanet_import.upload"),
        UiActionStep::GeneanetPoll => tracing::info_span!("ui.geneanet_import.poll"),
        UiActionStep::GeneanetSessionEncode => {
            tracing::info_span!("ui.geneanet_import.session_encode")
        }
        UiActionStep::GeneanetSessionDecode => {
            tracing::info_span!("ui.geneanet_import.session_decode")
        }
        UiActionStep::ExportRequest => tracing::info_span!("ui.export.request"),
        UiActionStep::ExportQueue => tracing::info_span!("ui.export.queue"),
        UiActionStep::ExportPoll => tracing::info_span!("ui.export.poll"),
        UiActionStep::ExportSave => tracing::info_span!("ui.export.save"),
    }
}

#[cfg(feature = "telemetry-client")]
fn page_span(page: UiPage) -> tracing::Span {
    match page {
        UiPage::Home => tracing::info_span!(parent: None, "ui.home.load"),
        UiPage::Pedigree => tracing::info_span!(parent: None, "ui.pedigree.load"),
        UiPage::PersonDetail => tracing::info_span!(parent: None, "ui.person_detail.load"),
        UiPage::SearchResults => tracing::info_span!(parent: None, "ui.search_results.load"),
        UiPage::Dictionary => tracing::info_span!(parent: None, "ui.dictionary.load"),
        UiPage::Settings => tracing::info_span!(parent: None, "ui.settings.load"),
        UiPage::AppSettings => tracing::info_span!(parent: None, "ui.app_settings.load"),
        UiPage::NotFound => tracing::info_span!(parent: None, "ui.not_found.load"),
        UiPage::Component => tracing::info_span!(parent: None, "ui.component.load"),
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
            (UiPage::SearchResults, "ui.search_results.load"),
            (UiPage::Dictionary, "ui.dictionary.load"),
            (UiPage::Settings, "ui.settings.load"),
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
}
