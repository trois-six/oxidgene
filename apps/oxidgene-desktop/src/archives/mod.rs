//! Archive windows: opening a cited register on its archive's portal.
//!
//! The interface asks through [`ArchiveViewerBridge`]. Each request starts a
//! resolution on the Dioxus runtime with the shared [`Resolver`], whose
//! session cache spares a second request for a citation already opened. The
//! resolution uses the `window` transport ([`transport`]): the archive window
//! first loads the collection's search page, the portal's own page, waiting
//! out — or asking the reader to answer — any anti-bot check, and the
//! adapter's requests then run in it. The window finally loads the target —
//! the cited view, or the portal's filtered results — with a banner saying
//! what OxidGene found, or, when the resolution failed, the landing of the
//! failure; a resolution running past [`LOOKUP_DEADLINE`] lands as a
//! timeout. What a window's page posts wakes the event loop at once
//! ([`Inbox`]), so that neither the banner nor the resolution waits for the
//! reader to move the mouse.
//!
//! Windows are top-level, since portals refuse to be framed, and share one
//! persistent web profile of their own, so that a portal's cookies spare the
//! reader its reuse licence and its challenge at every opening. On WebKitGTK,
//! a portal certificate served without its issuer is completed ([`tls`]).

mod script;
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
mod tls;
mod transport;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::event_loop::EventLoopWindowTarget;
use dioxus::desktop::tao::window::{Window, WindowBuilder};
use dioxus::desktop::wry::{WebContext, WebView, WebViewBuilder};
use oxidgene_archives::ArchiveTarget;
use oxidgene_archives::transport::origin_of;
use oxidgene_archives::{ArchiveRegistry, ResolveError, Resolver};
use oxidgene_ui::archive_viewer::{
    ArchiveLink, ArchivePageRequest, ArchiveRegister, ArchiveViewerBridge, ArchiveViewerMessages,
    ArchiveViewerOpener, ArchiveViewerRequest, AttachSender, Landing,
};
use serde::Deserialize;
use tokio::sync::Notify;
use tracing::{debug, warn};

use transport::{
    Attachable, Command, Fetched, Page, PageState, Seen, SessionId, Shared, Texts, WindowTransport,
};

/// One message from the scripts of [`script`].
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    Page(Page),
    Fetched(Fetched),
    /// The reader asks to attach the views on screen as a document.
    Attach,
    /// The reader closed the banner.
    Dismiss,
    /// The reader asks to open the page whose certificate could not be
    /// verified in the system browser.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    OpenInBrowser,
}

/// What reaches the event loop from the windows.
#[derive(Debug)]
enum Inbound {
    /// A message a page posted.
    Posted(Message),
    /// A page whose certificate could not be verified: its address.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    Untrusted(String),
}

/// Messages from the windows, waiting for the event loop.
///
/// A window's page posts on the GTK main loop, outside any event of the
/// event loop, whose handler alone carries messages out: queued and nothing
/// more, a message would wait for an unrelated event — the reader moving
/// the mouse over a window —, and with it the banner it shows and the
/// resolution waiting on it. Each message therefore also wakes the
/// [`pump`], a Dioxus task whose wake-up makes the event loop run the
/// handler at once.
#[derive(Clone, Default)]
struct Inbox {
    messages: Arc<Mutex<Vec<(SessionId, Inbound)>>>,
    posted: Arc<Notify>,
}

impl Inbox {
    fn push(&self, session: SessionId, inbound: Inbound) {
        if let Ok(mut messages) = self.messages.lock() {
            messages.push((session, inbound));
        }
        self.posted.notify_one();
    }

    fn drain(&self) -> Vec<(SessionId, Inbound)> {
        self.messages
            .lock()
            .map(|mut messages| messages.drain(..).collect())
            .unwrap_or_default()
    }
}

/// Wakes the Dioxus runtime whenever a window posts: Dioxus polls the task
/// on an event of the event loop, whose handler runs on that event and
/// carries the window's messages out.
async fn pump(posted: Arc<Notify>) {
    loop {
        posted.notified().await;
    }
}

struct WindowOpener {
    shared: Arc<Shared>,
    resolver: Arc<Resolver<'static>>,
    /// What the windows' messages notify, and whether the [`pump`] runs:
    /// started on the first opening, from the Dioxus runtime.
    posted: Arc<Notify>,
    pumping: AtomicBool,
}

impl WindowOpener {
    fn start_pump(&self) {
        if !self.pumping.swap(true, Ordering::Relaxed) {
            dioxus::core::spawn_forever(pump(Arc::clone(&self.posted)));
        }
    }
}

impl ArchiveViewerOpener for WindowOpener {
    fn supports(&self, link: &ArchiveLink) -> bool {
        ArchiveRegistry::embedded()
            .candidates(&link.citation)
            .is_some()
    }

    fn open(&self, request: ArchiveViewerRequest) {
        self.start_pump();
        let shared = Arc::clone(&self.shared);
        let resolver = Arc::clone(&self.resolver);
        // The task outlives the page that asked: the reader may navigate on
        // while the window opens.
        dioxus::core::spawn_forever(open_register(shared, resolver, request));
    }

    fn focus(&self) {
        dioxus::desktop::window().set_focus();
    }

    fn open_page(&self, request: ArchivePageRequest) {
        self.start_pump();
        let shared = Arc::clone(&self.shared);
        // Queued from a task, as a resolution's landing is, so that the
        // event loop that carries it out is woken.
        dioxus::core::spawn_forever(async move {
            shared.push(page_command(shared.next_id(), request));
        });
    }
}

/// What the window says, from the interface's messages.
fn texts(messages: &ArchiveViewerMessages) -> Texts {
    Texts {
        close: messages.close.clone(),
        challenge: messages.challenge.clone(),
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        certificate: messages.certificate.clone(),
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        open_in_browser: messages.open_in_browser.clone(),
    }
}

/// The command opening a portal page as it is, in a window of its own.
fn page_command(session: SessionId, request: ArchivePageRequest) -> Command {
    let ArchivePageRequest {
        title,
        url,
        banner,
        messages,
    } = request;
    Command::Load {
        session,
        title,
        origins: origin_of(&url).map(str::to_owned).into_iter().collect(),
        url,
        banner,
        texts: texts(&messages),
        attach: None,
    }
}

/// Resolves the citation through the session's window, then loads the
/// target in it.
async fn open_register(
    shared: Arc<Shared>,
    resolver: Arc<Resolver<'static>>,
    request: ArchiveViewerRequest,
) {
    let ArchiveViewerRequest {
        link,
        messages,
        attach,
    } = request;
    let session = shared.next_id();
    let transport = WindowTransport {
        shared: Arc::clone(&shared),
        session,
        title: link.title.clone(),
        searching: messages.searching.clone(),
        texts: texts(&messages),
    };
    let citation = refined(&link).await;
    let started = std::time::Instant::now();
    let outcome = within(LOOKUP_DEADLINE, resolver.resolve(&citation, &transport)).await;
    log_outcome(&link, &outcome, started);
    let attach =
        attach.and_then(|sender| attachable(&link, outcome.as_ref().ok()?, sender, &messages));
    let Landing { url, banner } = Landing::of(&link, outcome.map_err(|error| error.code()));
    if shared.is_closed(session) {
        return;
    }
    shared.push(Command::Load {
        session,
        title: link.title,
        origins: origin_of(&url).map(str::to_owned).into_iter().collect(),
        url,
        banner: banner.and_then(|banner| messages.banner(banner)),
        texts: transport.texts,
        attach,
    });
}

/// Logs how a resolution ended and how long it took, with the archive's
/// identifier and never the citation.
fn log_outcome(
    link: &ArchiveLink,
    outcome: &Result<ArchiveTarget, ResolveError>,
    started: std::time::Instant,
) {
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let archive = link.archive.id.as_str();
    match outcome {
        Err(error) => log_failure(archive, error.code(), elapsed_ms),
        Ok(_) => debug!(archive, elapsed_ms, "resolved the cited register"),
    }
}

fn log_failure(archive: &str, error: &str, elapsed_ms: u64) {
    warn!(
        error,
        archive, elapsed_ms, "could not resolve the cited register"
    );
}

/// The longest a resolution may run before the window lands with the
/// `timeout` banner. Each wait of the resolution is bounded already — the
/// start page, the reader's answer to a check, every request —; this bounds
/// their sum, so that whatever an adapter does, the reader is never left
/// before a lookup that ends nowhere. It leaves room for a reader answering
/// a check twice (§6.1).
const LOOKUP_DEADLINE: Duration = Duration::from_secs(8 * 60);

/// The resolution, failing `timeout` past `deadline`.
async fn within(
    deadline: Duration,
    resolution: impl Future<Output = Result<ArchiveTarget, ResolveError>>,
) -> Result<ArchiveTarget, ResolveError> {
    tokio::time::timeout(deadline, resolution)
        .await
        .unwrap_or(Err(ResolveError::Timeout))
}

/// What the window offers to attach: for an archive whose images OxidGene
/// may use, a target whose views all carry their image.
fn attachable(
    link: &ArchiveLink,
    target: &ArchiveTarget,
    sender: AttachSender,
    messages: &ArchiveViewerMessages,
) -> Option<Box<Attachable>> {
    ArchiveRegister::attachable(link, target).then(|| {
        Box::new(Attachable {
            sender,
            target: target.clone(),
            hint: messages.attach_hint.clone(),
            label: messages.attach.clone(),
        })
    })
}

/// The citation's parts read again with the place dictionary, which the
/// interface does not embed: it tells the locality from a parish or a hamlet
/// among the names the records give (docs/archives.md §5.1). Read on a
/// thread of its own, since the dictionary may be decompressed for it; the
/// link's own parts when the reading changes nothing or fails.
async fn refined(link: &ArchiveLink) -> oxidgene_archives::CitationParts {
    let (evidence, supplied) = (link.evidence.clone(), link.supplied.clone());
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let citation = ArchiveRegistry::embedded()
            .recognize(
                &evidence,
                supplied.as_ref(),
                Some(&oxidgene_api::service::archive::DictionaryPlaces),
            )
            .ok()
            .and_then(|recognition| recognition.citation());
        let _ = sender.send(citation);
    });
    receiver
        .await
        .ok()
        .flatten()
        .filter(|citation| citation.code == link.citation.code)
        .unwrap_or_else(|| link.citation.clone())
}

/// A banner to show: its text, and the label and IPC message kind of its
/// button.
type Shown = (String, Option<(String, &'static str)>);

/// What a window says over its pages, apart from the window so that it can
/// be tested.
///
/// The banner of a load — OxidGene searching, or what the resolution found —
/// shows over every page of the portal, of a block, and of a check the
/// reader was not asked to answer yet, from the load on, until the reader
/// closes it or the window loads another page: a portal page that navigates
/// on, a check's redirect, keeps it. A check the reader was asked to answer
/// shows the request to answer it instead, until the portal's page shows.
/// Over views the reader may attach, the banner offers to attach them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Status {
    banner: Option<String>,
    /// Whether the reader was asked to answer an anti-bot check.
    asking: bool,
    /// Whether the reader closed the banner.
    dismissed: bool,
}

impl Status {
    fn new(banner: Option<String>) -> Self {
        Self {
            banner,
            ..Self::default()
        }
    }

    /// What to show over a page of `state`.
    fn page(
        &mut self,
        state: PageState,
        attach: Option<&Attachable>,
        challenge: &str,
    ) -> Option<Shown> {
        if state == PageState::Challenge && self.asking {
            return Some((challenge.to_owned(), None));
        }
        if state != PageState::Challenge {
            self.asking = false;
        }
        if self.dismissed {
            return None;
        }
        match (state, attach) {
            (PageState::Portal, Some(attach)) => Some((
                self.banner.clone().unwrap_or_else(|| attach.hint.clone()),
                Some((attach.label.clone(), "attach")),
            )),
            _ => self.banner.clone().map(|banner| (banner, None)),
        }
    }

    /// The reader was asked to answer the check on screen.
    fn ask(&mut self) {
        self.asking = true;
    }

    /// The reader closed the banner.
    fn dismiss(&mut self) {
        self.dismissed = true;
    }
}

/// An open archive window.
struct ArchiveWindow {
    window: Window,
    webview: WebView,
    /// The origins its IPC messages are accepted from, shared with the
    /// WebView's IPC handler.
    origins: Arc<Mutex<Vec<String>>>,
    /// What it says over its pages.
    status: Status,
    texts: Texts,
    /// The views on screen the reader may attach.
    attach: Option<Box<Attachable>>,
    /// The page whose certificate could not be verified, which the reader
    /// may open in the system browser.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    untrusted: Option<String>,
}

impl ArchiveWindow {
    fn load(
        &mut self,
        url: &str,
        origins: Vec<String>,
        banner: Option<String>,
        texts: Texts,
        attach: Option<Box<Attachable>>,
    ) {
        if let Ok(mut allowed) = self.origins.lock() {
            *allowed = origins;
        }
        self.status = Status::new(banner);
        self.attach = attach;
        self.texts = texts;
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        {
            self.untrusted = None;
        }
        navigate(&self.webview, url);
    }

    fn eval(&self, script: &str) -> bool {
        self.webview.evaluate_script(script).is_ok()
    }

    fn show(&self, text: &str) {
        self.eval(&script::banner(text, &self.texts.close, None));
    }

    /// Sends the views on screen to the interface, which opens the document
    /// form prefilled with them.
    fn attach(&self) {
        if let Some(attach) = &self.attach
            && !attach.sender.send(attach.target.clone())
        {
            debug!("the page that asked to attach the views is gone");
        }
    }

    /// A page loaded: shows what [`Status::page`] says over it.
    fn on_page(&mut self, state: PageState) {
        let shown = self
            .status
            .page(state, self.attach.as_deref(), &self.texts.challenge);
        if let Some((text, action)) = shown {
            self.eval(&script::banner(
                &text,
                &self.texts.close,
                action.as_ref().map(|(label, kind)| (label.as_str(), *kind)),
            ));
        }
    }

    /// Asks the reader to answer the check on screen.
    fn ask(&mut self) {
        self.status.ask();
        self.show(&self.texts.challenge);
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    fn untrusted(&mut self, url: String) {
        self.untrusted = Some(url);
        self.eval(&script::banner(
            &self.texts.certificate,
            &self.texts.close,
            Some((&self.texts.open_in_browser, "open_in_browser")),
        ));
    }

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    fn open_in_browser(&self) {
        if let Some(url) = &self.untrusted {
            tls::open_in_browser(url);
        }
    }
}

/// The bridge for the interface, and the event-loop handler that serves it.
///
/// `profile` is the archive windows' web profile directory.
pub fn install<T: 'static>(
    profile: PathBuf,
) -> (
    ArchiveViewerBridge,
    impl FnMut(&Event<'_, T>, &EventLoopWindowTarget<T>) + 'static,
) {
    let shared = Arc::new(Shared::default());
    let inbox = Inbox::default();
    let bridge = ArchiveViewerBridge::new(Arc::new(WindowOpener {
        shared: Arc::clone(&shared),
        resolver: Arc::new(Resolver::new(ArchiveRegistry::embedded())),
        posted: Arc::clone(&inbox.posted),
        pumping: AtomicBool::new(false),
    }));
    let mut windows = HashMap::<SessionId, ArchiveWindow>::new();
    let mut context: Option<WebContext> = None;

    let handler = move |event: &Event<'_, T>, target: &EventLoopWindowTarget<T>| {
        // What the pages posted first: a message posted before a load this
        // round carries out belongs to the page being left.
        for (session, inbound) in inbox.drain() {
            receive_inbound(&shared, &mut windows, session, inbound);
        }

        for command in shared.drain() {
            match command {
                Command::Load {
                    session,
                    title,
                    url,
                    origins,
                    banner,
                    texts,
                    attach,
                } => {
                    if shared.is_closed(session) {
                        continue;
                    }
                    if let Some(window) = windows.get_mut(&session) {
                        window.load(&url, origins, banner, texts, attach);
                        continue;
                    }
                    let context =
                        context.get_or_insert_with(|| WebContext::new(Some(profile.clone())));
                    let opening = Opening {
                        session,
                        title: &title,
                        url: &url,
                        origins,
                        banner,
                        texts,
                        attach,
                    };
                    match open(target, context, opening, inbox.clone()) {
                        Some(window) => {
                            windows.insert(session, window);
                        }
                        None => shared.close(session),
                    }
                }
                Command::Ask { session } => {
                    if let Some(window) = windows.get_mut(&session) {
                        window.ask();
                    }
                }
                Command::Fetch {
                    session,
                    ticket,
                    script,
                } => {
                    if !windows
                        .get(&session)
                        .is_some_and(|window| window.eval(&script))
                    {
                        shared.abandon(ticket);
                    }
                }
            }
        }

        if let Event::WindowEvent {
            window_id,
            event: WindowEvent::CloseRequested,
            ..
        } = event
            && let Some(session) = windows
                .iter()
                .find(|(_, open)| open.window.id() == *window_id)
                .map(|(session, _)| *session)
        {
            windows.remove(&session);
            shared.close(session);
        }
    };

    (bridge, handler)
}

/// Carries out what a window sent.
fn receive_inbound(
    shared: &Shared,
    windows: &mut HashMap<SessionId, ArchiveWindow>,
    session: SessionId,
    inbound: Inbound,
) {
    let window = windows.get_mut(&session);
    match inbound {
        Inbound::Posted(Message::Page(page)) => {
            if page.state != PageState::Portal {
                debug!(
                    vendor = page.vendor.as_deref().unwrap_or_default(),
                    state = ?page.state,
                    "an anti-bot page in an archive window"
                );
            }
            if let Some(window) = window {
                window.on_page(page.state);
            }
            shared.seen(session, Seen::Page(page));
        }
        Inbound::Posted(Message::Fetched(fetched)) => shared.fetched(session, fetched),
        Inbound::Posted(Message::Attach) => {
            if let Some(window) = window {
                window.attach();
            }
        }
        Inbound::Posted(Message::Dismiss) => {
            if let Some(window) = window {
                window.status.dismiss();
            }
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        Inbound::Posted(Message::OpenInBrowser) => {
            if let Some(window) = window {
                window.open_in_browser();
            }
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        Inbound::Untrusted(url) => {
            if let Some(window) = window {
                window.untrusted(url);
            }
            shared.seen(session, Seen::Untrusted);
        }
    }
}

/// What a new window opens on.
struct Opening<'a> {
    session: SessionId,
    title: &'a str,
    url: &'a str,
    origins: Vec<String>,
    banner: Option<String>,
    texts: Texts,
    attach: Option<Box<Attachable>>,
}

/// Loads `url` in the window, revalidated with the portal rather than taken
/// from WebKit's HTTP cache.
///
/// Portals behind a bot mitigation (Sarthe) mark their pages cacheable for
/// a day, while the cookie that lets a page's own requests through lasts
/// for the session only. The archive windows' profile keeps the cache
/// across application restarts but not session cookies: a page taken from
/// the cache then reaches no server, gets no cookie, and every request of
/// its own scripts is answered by the mitigation's check, which they cannot
/// read — the page shows its loading indicators for good. Asked with
/// `Cache-Control: no-cache`, the server sees the page's load, as on a
/// first visit, and lets it through its check; a page still fresh is
/// answered `304 Not Modified`, so this costs the portal no transfer.
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn navigate(webview: &WebView, url: &str) {
    use dioxus::desktop::wry::WebViewExtUnix;
    use webkit2gtk::WebViewExt;

    webview.webview().load_request(&revalidated(url));
}

/// The request loading `url` past the HTTP cache ([`navigate`]).
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn revalidated(url: &str) -> webkit2gtk::URIRequest {
    use webkit2gtk::URIRequestExt;

    let request = webkit2gtk::URIRequest::new(url);
    if let Some(headers) = request.http_headers() {
        headers.append("Cache-Control", "no-cache");
    }
    request
}

/// Loads `url` in the window.
#[cfg(not(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn navigate(webview: &WebView, url: &str) {
    if webview.load_url(url).is_err() {
        warn!(
            error = "archive_navigation",
            "could not load a page in the archive window"
        );
    }
}

/// The `scheme://authority` of the page that posted an IPC message.
fn page_origin(uri: &dioxus::desktop::wry::http::Uri) -> Option<String> {
    Some(format!("{}://{}", uri.scheme_str()?, uri.authority()?))
}

/// Queues a message a window's page posted, when it is one of [`script`]'s
/// from a page on an accepted origin. The portal's own pages may post to
/// this channel too.
fn receive(
    session: SessionId,
    accepted: &Mutex<Vec<String>>,
    inbox: &Inbox,
    uri: &dioxus::desktop::wry::http::Uri,
    body: &str,
) {
    match read_message(accepted, uri, body) {
        Ok(message) => inbox.push(session, Inbound::Posted(message)),
        Err(reason) => debug!(reason, "ignoring an IPC message"),
    }
}

/// A message of [`script`]'s from a page on an accepted origin. The request
/// to open the unverified page in the system browser is taken from any
/// page — the page on screen is the one the failed load left — since it
/// names nothing: the address is the one the window recorded.
fn read_message(
    accepted: &Mutex<Vec<String>>,
    uri: &dioxus::desktop::wry::http::Uri,
    body: &str,
) -> Result<Message, &'static str> {
    let message = serde_json::from_str(body).map_err(|_| "not an archive window message")?;
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    if matches!(message, Message::OpenInBrowser) {
        return Ok(message);
    }
    let origin = page_origin(uri).ok_or("no page origin")?;
    if !accepted
        .lock()
        .is_ok_and(|accepted| accepted.contains(&origin))
    {
        return Err("outside the archive's origin");
    }
    Ok(message)
}

fn open<T>(
    target: &EventLoopWindowTarget<T>,
    context: &mut WebContext,
    opening: Opening<'_>,
    inbox: Inbox,
) -> Option<ArchiveWindow> {
    let window = WindowBuilder::new()
        .with_title(opening.title)
        .with_inner_size(LogicalSize::new(1280.0, 900.0))
        .build(target)
        .inspect_err(|_| {
            warn!(
                error = "archive_window_creation",
                "could not create the archive window"
            );
        })
        .ok()?;

    let origins = Arc::new(Mutex::new(opening.origins));
    let accepted = Arc::clone(&origins);
    let session = opening.session;
    let posted = inbox.clone();
    let builder = WebViewBuilder::new_with_web_context(context)
        .with_initialization_script_for_main_only(script::page(), true)
        .with_ipc_handler(move |request| {
            receive(session, &accepted, &posted, request.uri(), request.body());
        });
    // As for the Geneanet window: WebKitGTK attaches a WebView to the GTK
    // container tao puts in every window, not to the window handle.
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
            None => builder.build_gtk(window.gtk_window()),
        }
    };

    let webview = built
        .inspect_err(|_| {
            warn!(
                error = "archive_webview_creation",
                "could not create the archive WebView"
            );
        })
        .ok()?;

    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    tls::watch(&webview, session, Arc::clone(&origins), inbox);
    navigate(&webview, opening.url);

    Some(ArchiveWindow {
        window,
        webview,
        origins,
        status: Status::new(opening.banner),
        texts: opening.texts,
        attach: opening.attach,
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        untrusted: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogued_platform_has_an_adapter() {
        let registry = ArchiveRegistry::embedded();
        for archive in registry.archives() {
            for collection in &archive.collections {
                assert!(
                    registry.platform(&collection.platform).is_some(),
                    "{}: no adapter for `{}`",
                    archive.id,
                    collection.platform
                );
            }
        }
    }

    fn messages() -> ArchiveViewerMessages {
        ArchiveViewerMessages::new(&oxidgene_ui::i18n::I18n::new(
            oxidgene_ui::i18n::Language::english(),
        ))
    }

    #[test]
    fn a_page_opens_on_its_own_origin_with_its_banner() {
        let messages = messages();
        let command = page_command(
            7,
            ArchivePageRequest {
                title: "AD00 - Exampleville".to_owned(),
                url: "https://archives.example.org/ark:/00000/a1/5".to_owned(),
                banner: Some("No register matches.".to_owned()),
                messages: messages.clone(),
            },
        );
        assert_eq!(
            command,
            Command::Load {
                session: 7,
                title: "AD00 - Exampleville".to_owned(),
                url: "https://archives.example.org/ark:/00000/a1/5".to_owned(),
                origins: vec!["https://archives.example.org".to_owned()],
                banner: Some("No register matches.".to_owned()),
                texts: texts(&messages),
                attach: None,
            }
        );
        // The window's own texts are the interface's.
        assert_eq!(texts(&messages).challenge, messages.challenge);
        assert!(!messages.challenge.starts_with("archive_viewer."));
    }

    #[test]
    fn the_scripts_messages_are_read() {
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "page", "state": "portal"}"#),
            Ok(Message::Page(Page {
                state: PageState::Portal,
                ..
            }))
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(
                r#"{"kind": "page", "state": "blocked", "vendor": "cloudflare", "interactive": false}"#
            ),
            Ok(Message::Page(Page {
                state: PageState::Blocked,
                ..
            }))
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(
                r#"{"kind": "fetched", "ticket": 2, "error": "network"}"#
            ),
            Ok(Message::Fetched(_))
        ));
        // The banner's button over views the reader may attach.
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "attach"}"#),
            Ok(Message::Attach)
        ));
        // The banner's close button.
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "dismiss"}"#),
            Ok(Message::Dismiss)
        ));
        assert!(serde_json::from_str::<Message>(r#"{"kind": "ready"}"#).is_err());
        assert!(serde_json::from_str::<Message>(r#"{"kind": "portal-event"}"#).is_err());
        // A page cannot claim a certificate failure: only the window knows.
        assert!(
            serde_json::from_str::<Message>(
                r#"{"kind": "untrusted", "url": "https://elsewhere.example.org/"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_messages_from_the_archive_origin_only() {
        let accepted = Mutex::new(vec!["https://archives.example.org".to_owned()]);
        let page = r#"{"kind": "page", "state": "portal"}"#;
        let uri = |text: &str| text.parse::<dioxus::desktop::wry::http::Uri>().unwrap();
        assert!(matches!(
            read_message(&accepted, &uri("https://archives.example.org/search"), page),
            Ok(Message::Page(_))
        ));
        for elsewhere in [
            "https://elsewhere.example.org/search",
            "http://archives.example.org/search",
        ] {
            assert!(
                read_message(&accepted, &uri(elsewhere), page).is_err(),
                "{elsewhere}"
            );
        }
        assert!(read_message(&accepted, &uri("https://archives.example.org/"), "{}").is_err());
    }

    /// The request names no address: the window opens the one it recorded,
    /// whichever page is on screen once the load failed.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    #[test]
    fn the_request_to_open_the_browser_comes_from_any_page() {
        let accepted = Mutex::new(vec!["https://viewer.example.org".to_owned()]);
        let uri = "https://archives.example.org/search"
            .parse::<dioxus::desktop::wry::http::Uri>()
            .unwrap();
        assert!(matches!(
            read_message(&accepted, &uri, r#"{"kind": "open_in_browser"}"#),
            Ok(Message::OpenInBrowser)
        ));
    }

    /// A page's message, posted outside any event of the event loop, wakes
    /// the pump, so that the handler carries it out at once: a regression
    /// of a window that showed no banner and whose resolution stalled until
    /// the reader moved the mouse.
    #[tokio::test]
    async fn a_posted_message_wakes_the_event_loop() {
        let inbox = Inbox::default();
        let posted = Arc::clone(&inbox.posted);
        let woken = tokio::spawn(async move { posted.notified().await });
        tokio::task::yield_now().await;
        inbox.push(
            3,
            Inbound::Posted(Message::Page(Page {
                state: PageState::Portal,
                vendor: None,
                interactive: false,
            })),
        );
        tokio::time::timeout(Duration::from_secs(1), woken)
            .await
            .expect("the pump is woken")
            .unwrap();
        let drained = inbox.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, 3);
        assert!(inbox.drain().is_empty());
        // Posted while the pump is busy, a message still wakes it next.
        inbox.push(4, Inbound::Posted(Message::Dismiss));
        tokio::time::timeout(Duration::from_secs(1), inbox.posted.notified())
            .await
            .expect("the wake-up is kept");
    }

    #[tokio::test]
    async fn a_resolution_that_never_ends_lands_as_a_timeout() {
        let deadline = Duration::from_millis(20);
        let stalled = within(deadline, std::future::pending()).await;
        assert_eq!(stalled, Err(ResolveError::Timeout));
        let target = ArchiveTarget::Results {
            url: "https://archives.example.org/search".to_owned(),
            matches: Some(0),
        };
        let quick = within(deadline, std::future::ready(Ok(target.clone()))).await;
        assert_eq!(quick, Ok(target));
    }

    fn attachable() -> Attachable {
        let (sender, _) = futures_channel::mpsc::unbounded();
        Attachable {
            sender: AttachSender::new(sender),
            target: ArchiveTarget::Results {
                url: "https://archives.example.org/search".to_owned(),
                matches: Some(1),
            },
            hint: "Keep the view.".to_owned(),
            label: "Attach".to_owned(),
        }
    }

    #[test]
    fn the_banner_shows_over_every_page_of_a_load_until_closed() {
        let banner = |text: &str| Some((text.to_owned(), None));
        let mut status = Status::new(Some("Searching…".to_owned()));
        // A check the reader was not asked to answer, its redirect, then
        // the portal's page and a page it navigates on to.
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer"),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer"),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer"),
            banner("Searching…")
        );
        // A check the reader is asked to answer shows the request, on every
        // page of the check, until the portal's page shows.
        status.ask();
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer"),
            banner("Answer")
        );
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer"),
            banner("Answer")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer"),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer"),
            banner("Searching…")
        );
        // A block shows it too.
        assert_eq!(
            status.page(PageState::Blocked, None, "Answer"),
            banner("Searching…")
        );
        // Closed, it is not shown again.
        status.dismiss();
        assert_eq!(status.page(PageState::Portal, None, "Answer"), None);
        // A load without a banner shows none, but a request to answer a check.
        let mut status = Status::new(None);
        assert_eq!(status.page(PageState::Portal, None, "Answer"), None);
        status.ask();
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer"),
            banner("Answer")
        );
    }

    #[test]
    fn views_to_attach_show_the_offer_over_the_portal() {
        let attach = attachable();
        let offer = Some(("Attach".to_owned(), "attach"));
        let mut status = Status::new(None);
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer"),
            Some(("Keep the view.".to_owned(), offer.clone()))
        );
        let mut status = Status::new(Some("Go to view 3.".to_owned()));
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer"),
            Some(("Go to view 3.".to_owned(), offer))
        );
        status.dismiss();
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer"),
            None
        );
    }

    /// A regression: a portal page taken from WebKit's cache after a
    /// restart reached no server, so its own requests met the bot
    /// mitigation's check and its loading indicators turned for good. Every
    /// page a window loads goes through `navigate`, which asks the server
    /// (the request it builds needs GTK, so it is checked in the window).
    #[test]
    fn every_page_of_a_window_is_loaded_through_navigate() {
        let source = include_str!("mod.rs");
        let with_url = concat!(".with", "_url(");
        let load_url = concat!(".load", "_url(");
        assert!(!source.contains(with_url));
        // Only the `navigate` of the platforms without WebKitGTK.
        assert_eq!(source.matches(load_url).count(), 1);
        assert_eq!(source.matches(concat!("navigate", "(&")).count(), 2);
    }

    #[test]
    fn reads_the_origin_of_the_posting_page() {
        let uri: dioxus::desktop::wry::http::Uri =
            "https://archives.example.org/search?q=1#x".parse().unwrap();
        assert_eq!(
            page_origin(&uri).as_deref(),
            Some("https://archives.example.org")
        );
    }
}
