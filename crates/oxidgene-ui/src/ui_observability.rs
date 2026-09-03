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

#[derive(Clone, Copy)]
pub enum UiImportStep {
    Read,
    Inspect,
    Index,
    Connect,
    Preview,
    Collect,
    Upload,
    Poll,
    SessionEncode,
    SessionDecode,
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

pub async fn trace_ui_import<T>(format: &'static str, future: impl Future<Output = T>) -> T {
    #[cfg(feature = "telemetry-client")]
    {
        future
            .instrument(tracing::info_span!("ui.import", import.format = format))
            .await
    }

    #[cfg(not(feature = "telemetry-client"))]
    {
        let _ = format;
        future.await
    }
}

pub async fn trace_ui_import_step<T>(step: UiImportStep, future: impl Future<Output = T>) -> T {
    #[cfg(feature = "telemetry-client")]
    {
        let span = match step {
            UiImportStep::Read => tracing::info_span!("ui.import.read"),
            UiImportStep::Inspect => tracing::info_span!("ui.import.inspect"),
            UiImportStep::Index => tracing::info_span!("ui.import.index"),
            UiImportStep::Connect => tracing::info_span!("ui.import.connect"),
            UiImportStep::Preview => tracing::info_span!("ui.import.preview"),
            UiImportStep::Collect => tracing::info_span!("ui.import.collect"),
            UiImportStep::Upload => tracing::info_span!("ui.import.upload"),
            UiImportStep::Poll => tracing::info_span!("ui.import.poll"),
            UiImportStep::SessionEncode => tracing::info_span!("ui.import.session_encode"),
            UiImportStep::SessionDecode => tracing::info_span!("ui.import.session_decode"),
        };
        future.instrument(span).await
    }

    #[cfg(not(feature = "telemetry-client"))]
    {
        let _ = step;
        future.await
    }
}

#[cfg(feature = "telemetry-client")]
fn page_span(page: UiPage) -> tracing::Span {
    match page {
        UiPage::Home => tracing::info_span!("ui.home.load"),
        UiPage::Pedigree => tracing::info_span!("ui.pedigree.load"),
        UiPage::PersonDetail => tracing::info_span!("ui.person_detail.load"),
        UiPage::SearchResults => tracing::info_span!("ui.search_results.load"),
        UiPage::Dictionary => tracing::info_span!("ui.dictionary.load"),
        UiPage::Settings => tracing::info_span!("ui.settings.load"),
        UiPage::AppSettings => tracing::info_span!("ui.app_settings.load"),
        UiPage::NotFound => tracing::info_span!("ui.not_found.load"),
        UiPage::Component => tracing::info_span!("ui.component.load"),
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
            let parent = attributes
                .parent()
                .and_then(|parent| context.span(parent))
                .or_else(|| context.lookup_current())
                .map(|span| span.metadata().name().to_string());
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

    #[tokio::test(flavor = "current_thread")]
    async fn import_phase_is_a_child_of_the_import_action() {
        let captured = CapturedSpans::default();
        let subscriber = tracing_subscriber::registry().with(captured.clone());
        let _guard = tracing::subscriber::set_default(subscriber);

        trace_ui_import("gedcom", trace_ui_import_step(UiImportStep::Poll, async {})).await;

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
}
