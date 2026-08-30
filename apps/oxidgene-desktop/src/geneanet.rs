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
use oxidgene_ui::geneanet::{GeneanetBridge, GeneanetCollector, GeneanetEvent, WindowStrings};
use serde::Deserialize;
use tracing::{debug, warn};

/// Where the login window opens.
///
/// The media manager rather than a login page: signed in, it is the page whose
/// API we are about to call; signed out, Geneanet redirects to login and back,
/// which is the journey the user would take anyway.
const START_URL: &str = "https://www.geneanet.org/media/manager";

/// The window's size while it is a status panel rather than a browser.
///
/// Shrinking it is most of what tells a user it has stopped being something
/// they interact with — a full-size browser window that no longer responds to
/// clicks reads as broken, a small panel reads as progress.
const STATUS_SIZE: LogicalSize<f64> = LogicalSize::new(420.0, 260.0);

/// The origin whose cookies authenticate the download step.
const COOKIE_ORIGIN: &str = "https://www.geneanet.org";

/// The cookies that authenticate, in the order they are preferred.
///
/// Measured against the live API: exactly these two work, and nothing else a
/// browser sends is read.
///
/// `gntsess5` leads on purpose — the remember-me token is valid for months and
/// can mint fresh sessions on demand, so it is the worse of the two to hand
/// around. But it cannot be the *only* one taken: the login window pre-ticks
/// "remember me" precisely so a long collection survives a `gntsess5` expiry,
/// and after such a refresh the jar may hold no live `gntsess5` at the moment
/// this runs. Taking only that one then yields no session at all and every
/// download fails with "there is no Geneanet session to download it with".
const SESSION_COOKIES: [&str; 2] = ["gntsess5", "REMEMBERME"];

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
    /// One medium came back from [`script::ipc_fetch`].
    Fetched {
        url: String,
        #[serde(default)]
        data: Option<String>,
        #[serde(default)]
        error: Option<String>,
    },
    /// Progress through a fetch batch.
    Fetching {
        done: usize,
        total: usize,
    },
    /// The batch is finished.
    FetchDone,
    Error {
        message: String,
    },
}

/// Something the UI asked for, waiting for the event loop to pick it up.
enum Request {
    /// Open the window and collect.
    Open(UnboundedSender<GeneanetEvent>, WindowStrings),
    /// Fetch these media through the window that is already open.
    Fetch(Vec<String>, UnboundedSender<GeneanetEvent>),
    /// Done with the window — the import finished, or the wizard was dismissed.
    Close,
}

/// Requests from the UI, waiting for the event loop to pick them up.
type Pending = Arc<Mutex<Vec<Request>>>;

/// Messages from the window's scripts, waiting to be processed on the loop.
type Inbox = Arc<Mutex<Vec<Message>>>;

/// The `GeneanetCollector` the UI is handed.
///
/// It cannot open the window itself — that needs the event loop — so all it
/// does is queue the request.
struct QueueingCollector(Pending);

impl GeneanetCollector for QueueingCollector {
    fn start(&self, events: UnboundedSender<GeneanetEvent>, strings: WindowStrings) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(Request::Open(events, strings));
        }
    }

    fn fetch(&self, urls: Vec<String>, events: UnboundedSender<GeneanetEvent>) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(Request::Fetch(urls, events));
        }
    }

    fn close(&self) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(Request::Close);
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
    /// What to put on the status panel, already translated by the wizard.
    strings: WindowStrings,
    /// Where a fetch batch reports to. Separate from `events`, which belongs
    /// to the collection: by the time media are being fetched the wizard has
    /// moved on to its import step and is listening on a new channel.
    fetching: Option<UnboundedSender<GeneanetEvent>>,
    /// URLs still to fetch once the window has reached the right origin.
    queued_fetch: Vec<String>,
    /// Where this run's media are being written, once anything has been.
    staging: Option<Staging>,
    /// How many have been written, which is also how they are named.
    written: usize,
}

struct Staging(tempfile::TempDir);

impl Staging {
    fn create() -> Result<Self, String> {
        tempfile::Builder::new()
            .prefix("oxidgene-geneanet-")
            .tempdir()
            .map(Self)
            .map_err(|error| format!("could not create Geneanet staging directory: {error}"))
    }

    fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

impl Session {
    fn send(&self, event: GeneanetEvent) {
        // A closed receiver means the modal went away; the window is torn down
        // by the caller either way.
        let _ = self.events.unbounded_send(event);
    }

    /// Decodes one fetched medium and writes it beside the others.
    ///
    /// Named by position rather than by URL: a URL is not a filename, and the
    /// manifest the import receives is what maps one to the other. The
    /// extension is kept only so the directory is browsable.
    fn write_medium(&mut self, url: &str, encoded: &str) -> Result<String, String> {
        use base64::Engine as _;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| format!("{url}: {e}"))?;

        let index = self.written;
        let directory = self.staging()?;
        let extension = url
            .split('?')
            .next()
            .and_then(|path| path.rsplit('.').next())
            .filter(|ext| {
                ext.len() <= 5 && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
            })
            .map_or_else(String::new, |ext| format!(".{}", ext.to_ascii_lowercase()));

        let path = directory.join(format!("{index:05}{extension}"));
        std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        self.written += 1;

        Ok(path.display().to_string())
    }

    /// The directory this run's media are written to, created on first use.
    ///
    /// Under the OS temp directory rather than the app's data directory: these
    /// are working files that exist only until the import has read them.
    fn staging(&mut self) -> Result<&std::path::Path, String> {
        if self.staging.is_none() {
            self.staging = Some(Staging::create()?);
        }
        Ok(self.staging.as_ref().expect("staging was created").path())
    }

    /// Puts a status line on the window's panel, creating it if needed.
    fn status(&self, message: &str) {
        self.eval(&script::status_overlay(
            &self.strings.heading,
            message,
            &self.strings.cancel_hint,
        ));
    }

    /// Starts a fetch batch, moving the window to the right origin first.
    ///
    /// Renditions live on `gw.geneanet.org` and the collection ran on
    /// `www.geneanet.org`; a cross-origin `fetch` would simply be refused. So
    /// the window is navigated to whichever host the batch reads from — the
    /// session cookie is set on `.geneanet.org`, so it covers both — and the
    /// URLs go out once that page has loaded.
    fn begin_fetch(&mut self, urls: Vec<String>, events: UnboundedSender<GeneanetEvent>) {
        self.fetching = Some(events);

        let Some(origin) = urls.first().and_then(|url| origin_of(url)) else {
            self.finish_fetch();
            return;
        };

        self.queued_fetch = urls;
        if self.webview.load_url(&origin).is_err() {
            warn!(
                error = "window_navigation",
                "could not move the Geneanet window"
            );
            // Try from where we are; same-origin URLs still work.
            self.run_queued_fetch();
        }
    }

    /// Issues the queued batch. Called once the window has reached the origin.
    fn run_queued_fetch(&mut self) {
        if self.queued_fetch.is_empty() {
            return;
        }
        let urls = std::mem::take(&mut self.queued_fetch);
        let json = serde_json::to_string(&urls).unwrap_or_else(|_| "[]".into());
        self.status(&self.strings.matching);
        self.eval(&script::ipc_fetch(&json));
    }

    fn finish_fetch(&mut self) {
        if let Some(events) = self.fetching.take() {
            let _ = events.unbounded_send(GeneanetEvent::FetchDone);
        }
        // The session still owns the staged files until the backend has copied
        // them for the import job, but it has no more browser work. Keeping the
        // window visible at 100% makes the operation look unfinished.
        self.window.set_visible(false);
    }

    /// Reports one fetched medium to whoever asked for the batch.
    fn send_fetched(&mut self, event: GeneanetEvent) {
        if let Some(events) = &self.fetching {
            let _ = events.unbounded_send(event);
        }
    }

    fn eval(&self, script: &str) {
        if self.webview.evaluate_script(script).is_err() {
            warn!(
                error = "collection_script",
                "could not run the collection script in the login window"
            );
        }
    }

    /// Reads the session cookie out of the window, for the download step.
    ///
    /// Only needed when the archives do not cover every photo. Absent is not
    /// an error: the import then reports the photos it could not fetch rather
    /// than failing.
    fn cookie(&self) -> Option<String> {
        let cookies = self.webview.cookies_for_url(COOKIE_ORIGIN).ok()?;

        // Both when both are present: the pair is what a browser would send,
        // and it means a `gntsess5` that expires between here and the download
        // is re-minted from the remember-me token rather than ending the run.
        let header = SESSION_COOKIES
            .iter()
            .filter_map(|wanted| {
                cookies
                    .iter()
                    .find(|cookie| cookie.name() == *wanted && !cookie.value().is_empty())
            })
            .map(|cookie| format!("{}={}", cookie.name(), cookie.value()))
            .collect::<Vec<_>>()
            .join("; ");

        (!header.is_empty()).then_some(header)
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

        for request in queued {
            match request {
                Request::Open(events, strings) => {
                    if session.is_some() {
                        let _ = events.unbounded_send(GeneanetEvent::Failed(
                            "a Geneanet window is already open".into(),
                        ));
                        continue;
                    }
                    session = open(target, events, strings, Arc::clone(&handler_inbox));
                }
                // Dropping the session is what closes the window.
                Request::Close => drop(session.take()),
                Request::Fetch(urls, events) => match session.as_mut() {
                    Some(open) => open.begin_fetch(urls, events),
                    None => {
                        let _ = events.unbounded_send(GeneanetEvent::Failed(
                            "the Geneanet window is closed, so nothing can be fetched \
                             — sign in again"
                                .into(),
                        ));
                    }
                },
            }
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
        Message::Auth { signed_in: false } => {
            // A probe on a host with no media API — which is what the fetch
            // navigation lands on. Its arrival is the signal that the page
            // finished loading, so the queued batch can go out.
            open.run_queued_fetch();
            None
        }
        Message::Auth { signed_in: true } => {
            if open.collecting {
                // Already collecting: this is a later page load, which during
                // a fetch batch means the navigation completed.
                open.run_queued_fetch();
                return None;
            }
            open.collecting = true;

            // The human part is over. The window is neither hidden nor left
            // showing Geneanet: hiding it makes cancellation impossible — a
            // hidden window can never emit a close request, so a stalled
            // collection would be unrecoverable — and leaving the site up says
            // nothing about what the app is doing or whether it may be touched.
            //
            // Instead it shrinks to a panel and shows OxidGene's own status,
            // in words. The numbers live in the wizard's modal, which is the
            // only place they can be kept in step with the import.
            open.window.set_title(&open.strings.title);
            open.window.set_inner_size(STATUS_SIZE);
            open.status(&open.strings.reading_list);

            open.send(GeneanetEvent::SignedIn);
            open.eval(&script::ipc_collection());
            None
        }
        Message::Progress { done } => {
            // The bulk endpoints do not report a total, so the bar is honest
            // about counting up rather than pretending to know how far along
            // it is. The window shows no count at all.
            open.send(GeneanetEvent::Collecting { done, total: 0 });
            None
        }
        Message::Collected { data } => {
            let ids = match single_page_deposits(&data) {
                Ok(ids) => ids,
                Err(reason) => {
                    warn!(
                        error = "collection_parse",
                        %reason,
                        "could not read the collection the Geneanet window produced"
                    );
                    let message = open.strings.invalid_collection.clone();
                    let done = session.take()?;
                    done.send(GeneanetEvent::Failed(message));
                    return Some(done);
                }
            };
            open.collection = Some(data);

            if ids.is_empty() {
                // Nothing measurable, so nothing to match: finish here rather
                // than running an empty sizing pass.
                return finish(session, HashMap::new());
            }

            open.status(&open.strings.matching);
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
        Message::Fetched { url, data, error } => {
            // Written straight to disk. The server reads it from there, so the
            // bytes cross the process boundary once instead of riding through
            // the UI and back out in a request body.
            let (path, error) = match data {
                Some(encoded) => match open.write_medium(&url, &encoded) {
                    Ok(path) => (Some(path), None),
                    Err(message) => (None, Some(message)),
                },
                None => (None, error),
            };
            open.send_fetched(GeneanetEvent::Fetched { url, path, error });
            None
        }
        Message::Fetching { done, total } => {
            open.send_fetched(GeneanetEvent::Fetching { done, total });
            None
        }
        Message::FetchDone => {
            open.finish_fetch();
            None
        }
        Message::Error { message } => {
            let done = session.take()?;
            done.send(GeneanetEvent::Failed(message));
            Some(done)
        }
    }
}

/// Reports the collection — and leaves the window open.
///
/// This is the end of step 3 but not the end of the window's job. Every direct
/// request to Geneanet is challenged, so the media the archives cannot account
/// for are fetched through this same session during the import. Closing here
/// would leave step 5 with nothing to fetch through and no way to get it back
/// short of signing in again.
///
/// It is closed by [`Request::Close`], which the wizard sends when the import
/// finishes or the modal is dismissed, and by the user closing it themselves.
///
/// Returns the session only when there is nothing to keep it open for.
fn finish(session: &mut Option<Session>, deposit_sizes: HashMap<i64, u64>) -> Option<Session> {
    let collection = session.as_ref()?.collection.clone();
    let Some(collection) = collection else {
        let done = session.take()?;
        done.send(GeneanetEvent::Failed(
            "the login window reported no collection".into(),
        ));
        return Some(done);
    };

    let open = session.as_ref()?;
    let photo_count = view_count(&collection);
    open.send(GeneanetEvent::Collected {
        photo_count,
        collection,
        deposit_sizes,
        // Only used if the archives do not cover every photo.
        cookie: open.cookie(),
        // Geneanet does not put the account name anywhere this flow reads, and
        // scraping the page for it would be the first thing a redesign broke.
        account: None,
    });

    // Say so, rather than leaving the panel claiming to still be matching.
    open.status(&open.strings.idle);

    None
}

/// The scheme-and-host of a URL, or `None` if it carries neither.
fn origin_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split('/').next()?;
    (!host.is_empty()).then(|| format!("https://{host}/"))
}

/// How many views the collection holds — every page of every deposit.
fn view_count(collection: &str) -> usize {
    oxidgene_geneanet::manifest_from_collection(collection)
        .map(|manifest| manifest.view_count)
        .unwrap_or(0)
}

/// The deposits whose byte length can be asked for.
///
/// Only single-page ones: a multi-page deposit downloads as a ZIP that
/// Geneanet assembles on the fly and streams with no `Content-Length` at all,
/// so there is no length to match an archive entry against.
fn single_page_deposits(collection: &str) -> Result<Vec<i64>, String> {
    oxidgene_geneanet::manifest_from_collection(collection)
        .map(|manifest| {
            manifest
                .deposits
                .iter()
                .filter(|deposit| deposit.views.len() == 1)
                .map(|deposit| deposit.id)
                .collect()
        })
        .map_err(|error| error.to_string())
}

/// Builds the login window and wires its scripts up.
fn open<T: 'static>(
    target: &EventLoopWindowTarget<T>,
    events: UnboundedSender<GeneanetEvent>,
    strings: WindowStrings,
    inbox: Inbox,
) -> Option<Session> {
    let window = WindowBuilder::new()
        .with_title("Geneanet")
        .with_inner_size(LogicalSize::new(1100.0, 820.0))
        .build(target)
        .inspect_err(|_| {
            warn!(
                error = "window_creation",
                "could not create the Geneanet login window"
            )
        })
        .ok()?;

    let builder = WebViewBuilder::new()
        .with_url(START_URL)
        // An ephemeral context. The login form's "remember me" box is
        // pre-ticked so a long collection survives a `gntsess5` expiry, and
        // that token is valid for months — leaving it in the app's cookie jar
        // afterwards would mean a one-off import left a long-lived credential
        // to the user's Geneanet account sitting on disk. Incognito makes the
        // whole jar die with the window. (wry ignores any `WebContext` when
        // this is set, which is exactly the intent.)
        .with_incognito(true)
        // Runs at document start on every navigation, including the ones a
        // login and a Cloudflare challenge cause. Each says only "does the
        // media API answer yet", which is the one thing the next step needs.
        .with_initialization_script(script::PROBE)
        // Pre-checks "Remember me" on the login form: the window goes
        // headless once signed in, and a session nobody can see should not
        // expire under the collection.
        .with_initialization_script(script::REMEMBER_ME)
        .with_ipc_handler(move |request| {
            match serde_json::from_str::<Message>(request.body()) {
                Ok(message) => {
                    if let Ok(mut inbox) = inbox.lock() {
                        inbox.push(message);
                    }
                }
                // Geneanet's own pages post to this channel too; anything we
                // cannot read is not ours.
                Err(_) => debug!("ignoring an unrecognised IPC message"),
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
        .inspect_err(|_| {
            warn!(
                error = "webview_creation",
                "could not create the Geneanet WebView"
            )
        })
        .ok()?;

    let _ = events.unbounded_send(GeneanetEvent::Opened);

    Some(Session {
        window,
        webview,
        events,
        collecting: false,
        collection: None,
        strings,
        fetching: None,
        queued_fetch: Vec::new(),
        staging: None,
        written: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_is_removed_when_its_owner_is_dropped() {
        let path = {
            let staging = Staging::create().expect("staging directory");
            let path = staging.path().to_path_buf();
            std::fs::write(path.join("00000.jpg"), b"temporary media").unwrap();
            assert!(path.exists());
            path
        };

        assert!(!path.exists());
    }

    #[test]
    fn an_origin_is_taken_from_an_absolute_url() {
        // Renditions sit on a different host from the collection, and a
        // cross-origin fetch would simply be refused — so the batch has to
        // know where to move the window before it starts.
        assert_eq!(
            origin_of("https://gw.geneanet.org/public/img/x/normal.jpg?t=1").as_deref(),
            Some("https://gw.geneanet.org/")
        );
        assert_eq!(
            origin_of("https://www.geneanet.org/media/download/?deposits[]=1").as_deref(),
            Some("https://www.geneanet.org/")
        );
    }

    #[test]
    fn a_relative_url_has_no_origin_to_move_to() {
        // Manifest paths are host-relative; they are absolutised before they
        // reach a fetch batch, and anything that slipped through must not be
        // turned into a bogus navigation.
        assert_eq!(origin_of("/public/img/x/normal.jpg"), None);
        assert_eq!(origin_of(""), None);
    }

    #[test]
    fn the_fetch_result_shapes_are_the_ones_this_module_parses() {
        // Written in two languages; they can only disagree at runtime, on a
        // live account, after a login.
        let ok: Message =
            serde_json::from_str(r#"{"kind":"fetched","url":"/a.jpg","data":"AAA="}"#)
                .expect("parses");
        assert!(matches!(ok, Message::Fetched { data: Some(_), .. }));

        let failed: Message =
            serde_json::from_str(r#"{"kind":"fetched","url":"/a.jpg","error":"HTTP 404"}"#)
                .expect("parses");
        assert!(matches!(failed, Message::Fetched { error: Some(_), .. }));

        let done: Message = serde_json::from_str(r#"{"kind":"fetch_done"}"#).expect("parses");
        assert!(matches!(done, Message::FetchDone));
    }

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

        assert_eq!(single_page_deposits(collection).unwrap(), vec![1]);
    }

    #[test]
    fn an_unreadable_collection_is_reported_before_preview() {
        assert!(single_page_deposits("not json").is_err());
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
