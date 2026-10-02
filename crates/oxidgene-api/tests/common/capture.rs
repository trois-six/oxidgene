//! A `tracing` layer that keeps every field of every span and event, for
//! the guards that read what the application logs and traces.

use std::fmt::Debug;
use std::sync::{Arc, Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};

/// One recorded field: the span or event name, the field name, its value.
#[derive(Clone, Debug)]
pub struct Captured {
    pub owner: String,
    pub field: String,
    pub value: String,
}

/// The layer, and the fields it has kept so far.
#[derive(Clone, Default)]
pub struct Capture(pub Arc<Mutex<Vec<Captured>>>);

impl Capture {
    pub fn take(&self) -> Vec<Captured> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct Collector<'a> {
    owner: &'a str,
    out: Vec<Captured>,
}

impl Visit for Collector<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        self.out.push(Captured {
            owner: self.owner.to_string(),
            field: field.name().to_string(),
            value: format!("{value:?}"),
        });
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.out.push(Captured {
            owner: self.owner.to_string(),
            field: field.name().to_string(),
            value: value.to_string(),
        });
    }
}

impl Capture {
    fn keep(&self, owner: &str, visit: impl FnOnce(&mut Collector<'_>)) {
        let mut collector = Collector {
            owner,
            out: Vec::new(),
        };
        visit(&mut collector);
        self.0.lock().unwrap().extend(collector.out);
    }
}

impl<S> Layer<S> for Capture
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        self.keep(attributes.metadata().name(), |c| attributes.record(c));
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let name = ctx.span(id).map_or("?", |span| span.name());
        self.keep(name, |c| values.record(c));
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.keep(event.metadata().target(), |c| event.record(c));
    }
}

/// The capture installed as this test process's global subscriber, so the
/// spans of the blocking pool and of spawned tasks are kept too, which a
/// thread-local default would miss. Installed on first use.
pub fn global_capture() -> &'static Capture {
    static CAPTURE: OnceLock<Capture> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let capture = Capture::default();
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(capture.clone()),
        )
        .expect("no other global subscriber in a test process");
        capture
    })
}
