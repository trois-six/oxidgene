//! The Geneanet login window, and the collection that runs inside it.
//!
//! This is step 3 of the import wizard, and the one part of it that only the
//! desktop build can do. The UI declares what it needs through
//! [`oxidgene_ui::geneanet::GeneanetCollector`]; this module is the
//! implementation, kept here because it is `wry`/`tao` work and pulling those
//! into `oxidgene-ui` would make that crate unbuildable for the web target.
//!
//! # Why a real window, and why the requests go through it
//!
//! Two things are true at once. A normal user cannot copy a session cookie out
//! of developer tools, and geneanet.org sits behind Cloudflare, which
//! challenges HTTP clients on their TLS/HTTP2 fingerprint — a challenge
//! OxidGene deliberately does not attempt to defeat. Opening an actual browser
//! engine is not a way around that check; it is the thing the check is asking
//! for, and a human is present to satisfy it.
//!
//! So the window is where the requests happen. [`script::PROBE`] runs on every
//! page load and says whether the media API answers yet;
//! [`script::ipc_collection`] gathers the person↔photo mapping once it does;
//! [`script::ipc_sizes`] then asks each deposit's byte length, which is what
//! the local archives are matched against. All of it is the same traffic the
//! media manager page makes when a user clicks around it.
//!
//! # How the window gets created
//!
//! A `tao` window needs the event loop's `EventLoopWindowTarget`, which Dioxus
//! owns. [`install`] hands back a closure for
//! `Config::with_custom_event_handler`, so the window is built on the event
//! loop like any other. Requests reach it through a queue: the UI pushes one,
//! the re-render that the click causes wakes the loop, and the handler picks it
//! up on the next event.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::event_loop::EventLoopWindowTarget;
use dioxus::desktop::tao::window::Window;
use dioxus::desktop::wry::{WebView, WebViewBuilder};
use dioxus::desktop::{LogicalSize, WindowBuilder};
use futures_channel::mpsc::UnboundedSender;
use oxidgene_geneanet::script;
use oxidgene_ui::geneanet::{GeneanetBridge, GeneanetCollector, GeneanetEvent};
use serde::Deserialize;
use tracing::{debug, warn};

/// Where the login window opens.
///
/// The media manager rather than a login page: signed in, it is the page whose
/// API we are about to call; signed out, Geneanet redirects to login and back,
/// which is the journey the user would take anyway.
const START_URL: &str = "https://www.geneanet.org/media/manager";

/// The origin whose cookies authenticate the download step.
const COOKIE_ORIGIN: &str = "https://www.geneanet.org";

/// The session cookie. Measured against the live API: exactly this one and
/// `REMEMBERME` authenticate, and nothing else a browser sends is read.
///
/// `gntsess5` is taken in preference on purpose — the remember-me token is
/// valid for months and can mint fresh sessions on demand, so passing it to
/// the download step would hand around far more than the job needs.
const SESSION_COOKIE: &str = "gntsess5";

/// One message from the scripts running inside the window.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    /// [`script::PROBE`], on every page load.
    Auth {
        signed_in: bool,
    },
    /// Stage 1 progress.
    Progress {
        done: usize,
    },
    /// Stage 1 result: the JSON the server turns into a manifest.
    Collected {
        data: String,
    },
    /// Stage 2 progress.
    Sizing {
        done: usize,
        total: usize,
    },
    /// Stage 2 result. Keyed by string because that is what a JS object is.
    Sized {
        sizes: HashMap<String, u64>,
    },
    Error {
        message: String,
    },
}

/// Requests from the UI, waiting for the event loop to pick them up.
type Pending = Arc<Mutex<Vec<UnboundedSender<GeneanetEvent>>>>;

/// Messages from the window's scripts, waiting to be processed on the loop.
type Inbox = Arc<Mutex<Vec<Message>>>;

/// The `GeneanetCollector` the UI is handed.
///
/// It cannot open the window itself — that needs the event loop — so all it
/// does is queue the request.
struct QueueingCollector(Pending);

impl GeneanetCollector for QueueingCollector {
    fn start(&self, events: UnboundedSender<GeneanetEvent>) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(events);
        }
    }
}

/// A login window that is currently open.
struct Session {
    window: Window,
    webview: WebView,
    events: UnboundedSender<GeneanetEvent>,
    /// Set once the probe has reported a session, so the further probes that a
    /// redirect fires do not start the collection a second time.
    collecting: bool,
    /// Stage 1's output, held until stage 2 has measured the deposits.
    collection: Option<String>,
}

impl Session {
    fn send(&self, event: GeneanetEvent) {
        // A closed receiver means the modal went away; the window is torn down
        // by the caller either way.
        let _ = self.events.unbounded_send(event);
    }

    fn eval(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            warn!(%e, "could not run the collection script in the login window");
        }
    }

    /// Reads the session cookie out of the window, for the download step.
    ///
    /// Only needed when the archives do not cover every photo. Absent is not
    /// an error: the import then reports the photos it could not fetch rather
    /// than failing.
    fn cookie(&self) -> Option<String> {
        let cookies = self.webview.cookies_for_url(COOKIE_ORIGIN).ok()?;
        cookies
            .iter()
            .find(|cookie| cookie.name() == SESSION_COOKIE)
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
    }
}

/// Creates the bridge and the event handler that services it.
///
/// The bridge goes into the Dioxus context; the closure goes into
/// `Config::with_custom_event_handler`. They are returned together because the
/// queue they share is private to the pair.
///
/// Generic over the loop's user-event type: this handler never looks at one,
/// and `dioxus-desktop` does not export the type it uses.
pub fn install<T: 'static>() -> (
    GeneanetBridge,
    impl FnMut(&Event<'_, T>, &EventLoopWindowTarget<T>) + 'static,
) {
    let pending: Pending = Arc::new(Mutex::new(Vec::new()));
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));

    let bridge = GeneanetBridge::new(Arc::new(QueueingCollector(Arc::clone(&pending))));

    let mut session: Option<Session> = None;
    let handler_pending = Arc::clone(&pending);
    let handler_inbox = Arc::clone(&inbox);

    let handler = move |event: &Event<'_, T>, target: &EventLoopWindowTarget<T>| {
        // A request the UI queued. One window at a time: a second sign-in
        // while the first is still collecting would fight over the session.
        let queued: Vec<_> = handler_pending
            .lock()
            .map(|mut pending| pending.drain(..).collect())
            .unwrap_or_default();

        for events in queued {
            if session.is_some() {
                let _ = events.unbounded_send(GeneanetEvent::Failed(
                    "a Geneanet window is already open".into(),
                ));
                continue;
            }
            session = open(target, events, Arc::clone(&handler_inbox));
        }

        // The user closed the window. Before signing in that is not an error,
        // and after a successful collection the window is already gone — so
        // either way this only has to unwind what is still open.
        if let Event::WindowEvent {
            window_id,
            event: WindowEvent::CloseRequested,
            ..
        } = event
            && session
                .as_ref()
                .is_some_and(|open| open.window.id() == *window_id)
            && let Some(open) = session.take()
        {
            open.send(GeneanetEvent::Cancelled);
        }

        let messages: Vec<_> = handler_inbox
            .lock()
            .map(|mut inbox| inbox.drain(..).collect())
            .unwrap_or_default();

        for message in messages {
            if let Some(done) = handle(&mut session, message) {
                session = None;
                drop(done);
            }
        }
    };

    (bridge, handler)
}

/// Advances the session by one message.
///
/// Returns the session when it is finished with, so the caller can drop it —
/// dropping the [`Window`] is what closes it.
fn handle(session: &mut Option<Session>, message: Message) -> Option<Session> {
    let open = session.as_mut()?;

    match message {
        // The probe fires on every navigation, including the login page and any
        // Cloudflare interstitial. "Not yet" is the normal answer until the
        // user has signed in, and needs no reporting.
        Message::Auth { signed_in: false } => None,
        Message::Auth { signed_in: true } => {
            if open.collecting {
                return None;
            }
            open.collecting = true;
            open.send(GeneanetEvent::SignedIn);
            open.eval(&script::ipc_collection());
            None
        }
        Message::Progress { done } => {
            // The bulk endpoints do not report a total, so the bar is honest
            // about counting up rather than pretending to know how far along
            // it is.
            open.send(GeneanetEvent::Collecting { done, total: 0 });
            None
        }
        Message::Collected { data } => {
            let ids = single_page_deposits(&data);
            open.collection = Some(data);

            if ids.is_empty() {
                // Nothing measurable, so nothing to match: finish here rather
                // than running an empty sizing pass.
                return finish(session, HashMap::new());
            }

            open.send(GeneanetEvent::Sizing {
                done: 0,
                total: ids.len(),
            });
            let json = serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into());
            open.eval(&script::ipc_sizes(&json));
            None
        }
        Message::Sizing { done, total } => {
            open.send(GeneanetEvent::Sizing { done, total });
            None
        }
        Message::Sized { sizes } => {
            let sizes = sizes
                .into_iter()
                .filter_map(|(id, size)| id.parse::<i64>().ok().map(|id| (id, size)))
                .collect();
            finish(session, sizes)
        }
        Message::Error { message } => {
            let done = session.take()?;
            done.send(GeneanetEvent::Failed(message));
            Some(done)
        }
    }
}

/// Reports the collection and hands the session back for closing.
fn finish(session: &mut Option<Session>, deposit_sizes: HashMap<i64, u64>) -> Option<Session> {
    let done = session.take()?;
    let Some(collection) = done.collection.clone() else {
        done.send(GeneanetEvent::Failed(
            "the login window reported no collection".into(),
        ));
        return Some(done);
    };

    done.send(GeneanetEvent::Collected {
        collection,
        deposit_sizes,
        // Only used if the archives do not cover every photo.
        cookie: done.cookie(),
        // Geneanet does not put the account name anywhere this flow reads, and
        // scraping the page for it would be the first thing a redesign broke.
        account: None,
    });

    Some(done)
}

/// The deposits whose byte length can be asked for.
///
/// Only single-page ones: a multi-page deposit downloads as a ZIP that
/// Geneanet assembles on the fly and streams with no `Content-Length` at all,
/// so there is no length to match an archive entry against.
fn single_page_deposits(collection: &str) -> Vec<i64> {
    match oxidgene_geneanet::manifest_from_collection(collection) {
        Ok(manifest) => manifest
            .deposits
            .iter()
            .filter(|deposit| deposit.views.len() == 1)
            .map(|deposit| deposit.id)
            .collect(),
        Err(e) => {
            warn!(%e, "could not read the collection the login window produced");
            Vec::new()
        }
    }
}

/// Builds the login window and wires its scripts up.
fn open<T: 'static>(
    target: &EventLoopWindowTarget<T>,
    events: UnboundedSender<GeneanetEvent>,
    inbox: Inbox,
) -> Option<Session> {
    let window = WindowBuilder::new()
        .with_title("Geneanet")
        .with_inner_size(LogicalSize::new(1100.0, 820.0))
        .build(target)
        .inspect_err(|e| warn!(%e, "could not create the Geneanet login window"))
        .ok()?;

    let builder = WebViewBuilder::new()
        .with_url(START_URL)
        // Runs at document start on every navigation, including the ones a
        // login and a Cloudflare challenge cause. Each says only "does the
        // media API answer yet", which is the one thing the next step needs.
        .with_initialization_script(script::PROBE)
        .with_ipc_handler(move |request| {
            match serde_json::from_str::<Message>(request.body()) {
                Ok(message) => {
                    if let Ok(mut inbox) = inbox.lock() {
                        inbox.push(message);
                    }
                }
                // Geneanet's own pages post to this channel too; anything we
                // cannot read is not ours.
                Err(e) => debug!(%e, "ignoring an unrecognised IPC message"),
            }
        });

    // Building onto the window handle only works where the platform's webview
    // attaches to one. On WebKitGTK it does not: a tao window hands out an
    // Xlib/Wayland handle, which wry rejects with "the window handle kind is
    // not supported", and the webview has to go into a GTK container instead —
    // the vertical box tao puts in every window as its sole child. This is the
    // same split `dioxus-desktop` makes for its own windows, and it is kept
    // identical on purpose: a login window that attached differently from the
    // app's would be a second thing to keep working.
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let built = builder.build(&window);

    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let built = {
        use dioxus::desktop::tao::platform::unix::WindowExtUnix;
        use dioxus::desktop::wry::WebViewBuilderExtUnix;

        match window.default_vbox() {
            Some(vbox) => builder.build_gtk(vbox),
            // Only reachable if the default vbox was disabled at build time,
            // which this window does not do — but adding straight to the
            // window is the right fallback, since a GTK window is a container
            // too.
            None => builder.build_gtk(window.gtk_window()),
        }
    };

    let webview = built
        .inspect_err(|e| warn!(%e, "could not create the Geneanet WebView"))
        .ok()?;

    let _ = events.unbounded_send(GeneanetEvent::Opened);

    Some(Session {
        window,
        webview,
        events,
        collecting: false,
        collection: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_single_page_deposits_are_measured() {
        // A multi-page deposit's download has no Content-Length, so asking for
        // its size would waste a request and learn nothing.
        let collection = r#"{
            "deposits": [
                {"id": 1, "title": "one page", "type": "portraits", "private": true,
                 "views": [{"id": 10, "page": 1, "files": {}}]},
                {"id": 2, "title": "a dossier", "type": "documents", "private": true,
                 "views": [{"id": 20, "page": 1, "files": {}},
                           {"id": 21, "page": 2, "files": {}}]}
            ],
            "references": [],
            "view_references": {}
        }"#;

        assert_eq!(single_page_deposits(collection), vec![1]);
    }

    #[test]
    fn an_unreadable_collection_measures_nothing_rather_than_panicking() {
        assert!(single_page_deposits("not json").is_empty());
    }

    #[test]
    fn the_probe_result_is_the_shape_this_module_parses() {
        // PROBE and `Message::Auth` are written in two languages and can only
        // disagree at runtime, on a live account, after a login.
        let parsed: Message =
            serde_json::from_str(r#"{"kind":"auth","signed_in":true,"deposits":378}"#)
                .expect("parses");
        assert!(matches!(parsed, Message::Auth { signed_in: true }));
    }

    #[test]
    fn the_collection_and_sizing_results_are_the_shapes_this_module_parses() {
        let collected: Message =
            serde_json::from_str(r#"{"kind":"collected","data":"{}"}"#).expect("parses");
        assert!(matches!(collected, Message::Collected { .. }));

        let sized: Message =
            serde_json::from_str(r#"{"kind":"sized","sizes":{"16053569":69122}}"#).expect("parses");
        match sized {
            Message::Sized { sizes } => assert_eq!(sizes.get("16053569"), Some(&69122)),
            other => panic!("got {other:?}"),
        }
    }
}
