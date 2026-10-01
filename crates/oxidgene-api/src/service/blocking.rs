//! Work moved off the async workers without leaving its trace.
//!
//! Every blocking hand-off in this crate goes through here: a bare
//! `spawn_blocking` runs on a thread that knows nothing of the caller's span,
//! so whatever the work traces — its own spans, its database calls — would
//! start traces of their own instead of continuing the request's.

use oxidgene_core::OxidGeneError;
use tokio::task::JoinHandle;
use tracing::Span;

/// Run `work` on the blocking pool inside `span`.
///
/// `spawn_blocking` does not carry the caller's span over to the thread it
/// runs on, so a span opened there would start a trace of its own. The caller
/// therefore creates `span` — under its own, with aggregate counts as fields —
/// and it is entered on the blocking thread around `work`. The caller's
/// subscriber goes along too, so spans `work` opens itself reach the same
/// subscriber even where it is scoped rather than global.
pub(crate) async fn run<T, F>(span: Span, work: F) -> Result<T, OxidGeneError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    spawn_in(span, work)
        .await
        .map_err(|error| OxidGeneError::Internal(error.to_string()))
}

/// `spawn_blocking` inside the span current at the call: for a hand-off that
/// is not worth a span of its own but must stay in the caller's trace.
pub(crate) fn spawn<T, F>(work: F) -> JoinHandle<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    spawn_in(Span::current(), work)
}

/// `spawn_blocking` inside `span`, with the caller's subscriber.
pub(crate) fn spawn_in<T, F>(span: Span, work: F) -> JoinHandle<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let dispatch = tracing::dispatcher::get_default(Clone::clone);
    tokio::task::spawn_blocking(move || {
        tracing::dispatcher::with_default(&dispatch, || span.in_scope(work))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;

    use tracing::Subscriber;
    use tracing::span::Id;
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::{Context, SubscriberExt as _};
    use tracing_subscriber::registry::LookupSpan;

    use super::*;

    /// Each span entered: its name, its parent's name, and the entering thread.
    type Entered = (String, Option<String>, ThreadId);

    #[derive(Clone, Default)]
    struct Entries(Arc<Mutex<Vec<Entered>>>);

    impl<S> Layer<S> for Entries
    where
        S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    {
        fn on_enter(&self, id: &Id, context: Context<'_, S>) {
            let Some(span) = context.span(id) else {
                return;
            };
            let parent = span.parent().map(|parent| parent.name().to_string());
            self.0.lock().expect("capture lock").push((
                span.name().to_string(),
                parent,
                std::thread::current().id(),
            ));
        }
    }

    #[tokio::test]
    async fn blocking_work_runs_inside_a_child_of_the_callers_span() {
        let entries = Entries::default();
        let subscriber = tracing_subscriber::registry().with(entries.clone());
        let _guard = tracing::subscriber::set_default(subscriber);
        let caller = std::thread::current().id();

        let load = tracing::info_span!("statistics.load");
        let span = load.in_scope(|| tracing::info_span!("statistics.compute"));
        let answer = run(span, || 42).await.expect("blocking work completes");

        assert_eq!(answer, 42);
        let entries = entries.0.lock().expect("capture lock");
        assert!(
            entries
                .iter()
                .any(|(name, parent, thread)| name == "statistics.compute"
                    && parent.as_deref() == Some("statistics.load")
                    && *thread != caller),
            "{entries:?}"
        );
    }
}
