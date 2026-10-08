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
//! timeout. A viewer without an address per view, which opens the register
//! on its first view, is then brought to the cited view with its own
//! page-number control ([`Driver`], `go_to.js`). Until then a progress
//! overlay covers the portal's pages ([`cover`]), from which the reader may
//! cancel the lookup. What a window's page posts wakes the event loop at
//! once ([`Inbox`]), so that neither the banner, the overlay nor the
//! resolution waits for the reader to move the mouse.
//!
//! Windows are top-level, since portals refuse to be framed, and share one
//! persistent web profile of their own, so that a portal's cookies spare the
//! reader its reuse licence and its challenge at every opening; a portal's
//! cookie banner is refused on the reader's behalf, never accepted, and a
//! listed notice that asks no consent is acknowledged (`consent.js`); the
//! profile keeps that choice too. On WebKitGTK,
//! a portal certificate served without its issuer is completed ([`tls`]).

mod cover;
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
use std::time::{Duration, Instant};

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
    ArchiveViewerOpener, ArchiveViewerRequest, AttachSender, Landing, LandingBanner,
};
use serde::Deserialize;
use tokio::sync::Notify;
use tracing::{debug, warn};

use cover::Cover;
use transport::{
    Attachable, Command, Drive, Fetched, Onward, Page, PageState, Progress, Seen, SessionId,
    Shared, Stage, Texts, WindowTransport,
};

/// One message from the scripts of [`script`].
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    /// A main-frame document starts.
    Document,
    Page(Page),
    Fetched(Fetched),
    /// The reader asks to attach the views on screen as a document.
    Attach,
    /// The reader closed the banner.
    Dismiss,
    /// The reader asks to load the page on screen again: a server's error
    /// page.
    Reload,
    Consent(Consent),
    Driven(Driven),
    /// The reader cancels the lookup from the progress overlay.
    Cancel,
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

/// What `consent.js` did with a portal's cookie banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConsentState {
    /// OxidGene refused the consent.
    Refused,
    /// The banner offers no refusal OxidGene could click: the reader's.
    Left,
    /// The banner left to the reader is gone.
    Closed,
    /// OxidGene acknowledged an information notice that asks no consent.
    Dismissed,
}

/// A cookie banner's outcome, posted by `consent.js`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Consent {
    state: ConsentState,
    /// The consent manager recognized.
    #[serde(default)]
    manager: Option<String>,
}

impl Consent {
    fn log(&self) {
        debug!(
            manager = self.manager.as_deref().unwrap_or_default(),
            state = ?self.state,
            "a cookie banner in an archive window"
        );
    }
}

/// Whether `go_to.js` brought a viewer to the cited view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DrivenState {
    Shown,
    Failed,
}

/// What `go_to.js` made of a viewer without an address per view.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Driven {
    state: DrivenState,
    /// Why it failed: `not_ready`, `beyond`, `no_control`, `not_shown` or
    /// `error`.
    #[serde(default)]
    reason: Option<String>,
}

impl Driven {
    fn log(&self) {
        debug!(
            state = ?self.state,
            reason = self.reason.as_deref().unwrap_or_default(),
            "an archive viewer brought to the cited view"
        );
    }
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
        searching: messages.searching.clone(),
        connecting: messages.step_connecting.clone(),
        looking_up: messages.step_searching.clone(),
        opening: messages.step_opening.clone(),
        opening_view: messages.step_opening_view.clone(),
        elapsed: messages.step_elapsed.clone(),
        cancel: messages.cancel.clone(),
        palette: messages.palette.clone(),
        close: messages.close.clone(),
        challenge: messages.challenge.clone(),
        page_timeout: messages.page_timeout.clone(),
        page_error: messages.page_error.clone(),
        reload: messages.reload.clone(),
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
        hold: hold(None, &url),
        url,
        banner,
        progress: None,
        texts: Box::new(texts(&messages)),
        attach: None,
        onward: None,
        drive: None,
    }
}

/// Resolves the citation through the session's window, then loads the
/// target in it; or, when the reader cancels, stops the resolution and
/// loads the collection's filtered search page.
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
        archive: link.archive.name.clone(),
        texts: texts(&messages),
    };
    let citation = refined(&link).await;
    let started = std::time::Instant::now();
    let mut cancelled = shared.cancellable(session);
    // Dropping the resolution drops its requests: an answer the window
    // posts later finds nothing waiting.
    let outcome = tokio::select! {
        outcome = within(LOOKUP_DEADLINE, resolver.resolve(&citation, &transport)) => Some(outcome),
        Ok(()) = &mut cancelled => None,
    };
    shared.settle(session);
    if let Some(outcome) = &outcome {
        log_outcome(&link, outcome, started);
    }
    if shared.is_closed(session) {
        return;
    }
    shared.push(match outcome {
        Some(outcome) => landing(&link, &messages, attach, &transport, outcome),
        None => cancelled_landing(&link, &transport),
    });
}

/// Loads where the resolution ended, under the overlay's last step.
fn landing(
    link: &ArchiveLink,
    messages: &ArchiveViewerMessages,
    attach: Option<AttachSender>,
    transport: &WindowTransport,
    outcome: Result<ArchiveTarget, ResolveError>,
) -> Command {
    let attach =
        attach.and_then(|sender| attachable(link, outcome.as_ref().ok()?, sender, messages));
    let addressed = match &outcome {
        Ok(ArchiveTarget::View { views, .. }) => views.first().map(|view| view.view),
        _ => None,
    };
    let Landing { url, banner, then } = Landing::of(link, outcome.map_err(|error| error.code()));
    // On a licence page the reader is asked to accept, only dialogs hold
    // the banner asking it; over the target, its viewer's licence too.
    let hold = hold(Some(link), then.as_ref().map_or(url.as_str(), |_| ""));
    let onward = then.map(|then| {
        Box::new(Onward {
            url: then.url,
            licence: then.licence,
            banner: then.banner.and_then(|banner| messages.banner(banner)),
        })
    });
    let drive = banner.and_then(|banner| drive(link, &url, banner, messages));
    let view = addressed.or(drive.as_ref().map(|(view, _)| *view));
    // The banner naming the view to go to waits for the drive's outcome.
    let banner = match drive {
        Some(_) => None,
        None => banner.and_then(|banner| messages.banner(banner)),
    };
    Command::Load {
        session: transport.session,
        title: link.title.clone(),
        origins: origin_of(&url).map(str::to_owned).into_iter().collect(),
        url,
        banner,
        progress: Some(transport.progress(Stage::Opening { view })),
        texts: Box::new(transport.texts.clone()),
        attach,
        onward,
        drive: drive.map(|(_, drive)| Box::new(drive)),
        hold,
    }
}

/// For a landing whose banner names the view to go to — a register whose
/// portal has no address per view —, the view and the drive bringing the
/// viewer there, when the catalogue says how (`go_to` in
/// oxidgene-archives' `viewers.json`) for the collection serving the
/// target.
fn drive(
    link: &ArchiveLink,
    url: &str,
    banner: LandingBanner,
    messages: &ArchiveViewerMessages,
) -> Option<(u16, Drive)> {
    let view = banner.go_to_view()?;
    let viewer = ArchiveRegistry::embedded()
        .viewer_at(&link.citation, url)
        .filter(|viewer| viewer.go_to.is_some())?;
    Some((
        view,
        Drive {
            script: script::go_to(viewer, view),
            fallback: messages.banner(banner),
        },
    ))
}

/// Loads the collection's filtered search page — the archive's website
/// when there is none —, without a banner, for a reader who cancelled:
/// the portal's page where they may search themselves.
fn cancelled_landing(link: &ArchiveLink, transport: &WindowTransport) -> Command {
    debug!(
        archive = link.archive.id.as_str(),
        "the reader cancelled a lookup"
    );
    let url = ArchiveRegistry::embedded()
        .offline_target(&link.citation)
        .map_or_else(
            |_| link.archive.website.clone(),
            |target| target.url().to_owned(),
        );
    Command::Load {
        session: transport.session,
        title: link.title.clone(),
        origins: origin_of(&url).map(str::to_owned).into_iter().collect(),
        url,
        banner: None,
        progress: None,
        texts: Box::new(transport.texts.clone()),
        attach: None,
        onward: None,
        drive: None,
        hold: hold(Some(link), ""),
    }
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

/// A portal's modal dialog, which the reader answers before OxidGene's
/// banner shows over the page: a viewer's reuse licence (Val-d'Oise's
/// Monocle viewer).
const MODAL_DIALOGS: &str = r#"dialog[open], [aria-modal="true"]"#;

/// What the reader answers on a portal's page before OxidGene's banner shows
/// over it, so that the banner never covers it (docs/archives.md §6.1): any
/// modal dialog, a cookie banner of the managers `consent.js` knows, and the
/// reuse licence of the viewer the target `url` opens in (Arkothèque's
/// « licence clic »), for a link's target.
fn hold(link: Option<&ArchiveLink>, url: &str) -> Vec<String> {
    let licence = link.and_then(|link| {
        ArchiveRegistry::embedded()
            .viewer_at(&link.citation, url)?
            .licence
            .clone()
    });
    std::iter::once(MODAL_DIALOGS.to_owned())
        .chain(licence)
        .chain(script::consent_banners())
        .collect()
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
/// A server's error page shows why the portal failed, with a button loading
/// it again, in place of the load's banner. Over views the reader may
/// attach, the banner offers to attach them.
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

    /// What to show over a page of `state`; `failed`, what to show over a
    /// server's error page.
    fn page(
        &mut self,
        state: PageState,
        attach: Option<&Attachable>,
        challenge: &str,
        failed: Option<Shown>,
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
        if state == PageState::Error && failed.is_some() {
            return failed;
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

    /// The banner of the load is now `banner`: a drive's fallback.
    fn fall_back(&mut self, banner: Option<String>) {
        self.banner = banner;
    }
}

/// How many times a drive starts: a document replacing the page it runs in
/// stops it, and it starts again on the next portal page.
const DRIVE_STARTS: u8 = 2;

/// How a drive ended.
#[derive(Debug, PartialEq, Eq)]
enum Drove {
    /// The viewer shows the cited view.
    Shown,
    /// It does not: the banner naming the view to go to.
    Failed(Option<String>),
}

/// Brings a viewer without an address per view to the cited view, apart
/// from the window so that it can be tested (docs/archives.md §6.1).
///
/// The drive starts on the landing's first portal page, under the progress
/// overlay, which stays until it ends. A document replacing that page stops
/// it: it starts again on the next portal page, [`DRIVE_STARTS`] times in
/// all, and fails after that. When it fails, the banner names the view to
/// go to.
#[derive(Debug, Default)]
struct Driver {
    drive: Option<Box<Drive>>,
    running: bool,
    starts: u8,
}

impl Driver {
    fn new(drive: Option<Box<Drive>>) -> Self {
        Self {
            drive,
            ..Self::default()
        }
    }

    /// The script to run over a page of `state`: a portal page while the
    /// drive waits.
    fn page(&mut self, state: PageState) -> Option<String> {
        let drive = self.drive.as_ref()?;
        if state != PageState::Portal || self.running {
            return None;
        }
        self.running = true;
        self.starts += 1;
        Some(drive.script.clone())
    }

    /// Whether the drive runs, which holds the overlay.
    fn running(&self) -> bool {
        self.running
    }

    /// A new document starts: a drive running in the page left stops with
    /// it, and fails once it has started [`DRIVE_STARTS`] times.
    fn document(&mut self) -> Option<Drove> {
        if !std::mem::take(&mut self.running) || self.starts < DRIVE_STARTS {
            return None;
        }
        self.drive.take().map(|drive| Drove::Failed(drive.fallback))
    }

    /// The outcome `go_to.js` posted, of a drive that runs.
    fn driven(&mut self, driven: &Driven) -> Option<Drove> {
        if !std::mem::take(&mut self.running) {
            return None;
        }
        let drive = self.drive.take()?;
        Some(match driven.state {
            DrivenState::Shown => Drove::Shown,
            DrivenState::Failed => Drove::Failed(drive.fallback),
        })
    }
}

/// A landing on a portal's reuse licence, waiting for the reader to accept
/// it (docs/archives.md §6.1). Nothing is accepted on the reader's behalf:
/// the reader has passed the licence once a page behind it shows after the
/// licence page did. A page shown before the licence page — the page being
/// left, the entry's redirect — does not count.
#[derive(Debug, PartialEq, Eq)]
struct Pending {
    onward: Onward,
    licence_shown: bool,
}

impl Pending {
    fn new(onward: Onward) -> Self {
        Self {
            onward,
            licence_shown: false,
        }
    }

    /// Whether the portal page at `url` says that the reader has passed the
    /// licence, so that the window goes on to the target.
    fn passed(&mut self, state: PageState, url: &str) -> bool {
        if state != PageState::Portal {
            return false;
        }
        if self.onward.licence.is_page(url) {
            self.licence_shown = true;
            return false;
        }
        self.licence_shown && self.onward.licence.guards(url)
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
    /// The progress overlay.
    cover: Cover,
    texts: Box<Texts>,
    /// The views on screen the reader may attach.
    attach: Option<Box<Attachable>>,
    /// The target to go on to once the reader has passed the portal's
    /// reuse licence on screen.
    pending: Option<Pending>,
    /// The viewer to bring to the cited view.
    driver: Driver,
    /// What the reader answers before the banner shows over the portal.
    hold: Vec<String>,
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
    /// Loads `url`. The overlay of `progress` shows at once over the page
    /// being left, then over the new one.
    fn load(&mut self, url: &str, origins: Vec<String>, landing: Landed) {
        let Landed {
            banner,
            progress,
            texts,
            attach,
            onward,
            drive,
            hold,
        } = landing;
        if let Ok(mut allowed) = self.origins.lock() {
            *allowed = origins;
        }
        self.status = Status::new(banner);
        self.attach = attach;
        self.pending = onward.map(|onward| Pending::new(*onward));
        self.driver = Driver::new(drive);
        self.hold = hold;
        self.texts = texts;
        self.cover.load(progress, Instant::now());
        self.render();
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
        self.eval(&script::banner(text, &self.texts.close, None, &[]));
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

    /// Shows the progress overlay as the cover stands, or removes it.
    fn render(&self) {
        self.eval(&script::overlay(
            self.cover.shown(Instant::now(), self.status.asking),
            &self.texts,
        ));
    }

    /// A document starts: the overlay covers it at once, if it shows.
    fn on_document(&mut self) {
        if let Some(Drove::Failed(banner)) = self.driver.document() {
            self.status.fall_back(banner);
        }
        self.cover.document();
        if self
            .cover
            .shown(Instant::now(), self.status.asking)
            .is_some()
        {
            self.render();
        }
    }

    /// A page was classified: shows what [`Status::page`] says over it, and
    /// the overlay as it now stands; the landing's page ends the overlay. A
    /// page behind the reuse licence the reader was asked to accept goes on
    /// to the target. A server's error page says why once no overlay
    /// covers it: one the resolution meets fails it, and its landing then
    /// shows. A drive starts on the landing's portal page, under the
    /// overlay.
    fn on_page(&mut self, page: &Page) {
        if self
            .pending
            .as_mut()
            .is_some_and(|pending| pending.passed(page.state, &page.url))
            && let Some(Pending { onward, .. }) = self.pending.take()
        {
            debug!("the reader passed the portal's licence: opening the target");
            let origins = origin_of(&onward.url)
                .map(str::to_owned)
                .into_iter()
                .collect();
            let landed = Landed {
                banner: onward.banner,
                progress: None,
                texts: self.texts.clone(),
                attach: None,
                onward: None,
                drive: None,
                hold: self.hold.clone(),
            };
            self.load(&onward.url, origins, landed);
            return;
        }
        let state = page.state;
        let drive = self.driver.page(state);
        self.cover.page(self.driver.running());
        let covered = self
            .cover
            .shown(Instant::now(), self.status.asking)
            .is_some();
        let failed = (!covered).then(|| failure_banner(page.status, &self.texts));
        self.show_status(state, failed);
        if let Some(script) = drive {
            self.eval(&script);
        }
        self.render();
    }

    /// Shows what [`Status::page`] says over a page of `state`; `failed`,
    /// what to show over a server's error page.
    fn show_status(&mut self, state: PageState, failed: Option<Shown>) {
        let shown = self
            .status
            .page(state, self.attach.as_deref(), &self.texts.challenge, failed);
        if let Some((text, action)) = shown {
            // Over the portal's page, the banner waits for its dialogs; the
            // request to answer a check, or why a server failed, shows at
            // once.
            let hold = match state {
                PageState::Portal => self.hold.as_slice(),
                PageState::Challenge | PageState::Blocked | PageState::Error => &[],
            };
            self.eval(&script::banner(
                &text,
                &self.texts.close,
                action.as_ref().map(|(label, kind)| (label.as_str(), *kind)),
                hold,
            ));
        }
    }

    /// The drive ended: the overlay is gone, and when the viewer does not
    /// show the cited view, the banner names the view to go to.
    fn on_driven(&mut self, driven: &Driven) {
        driven.log();
        let Some(drove) = self.driver.driven(driven) else {
            return;
        };
        if let Drove::Failed(banner) = drove {
            self.status.fall_back(banner);
            self.show_status(PageState::Portal, None);
        }
        self.cover.release();
        self.render();
    }

    /// Loads the page on screen again, for a reader facing a server's error
    /// page.
    fn reload(&self) {
        if self.webview.reload().is_err() {
            debug!("the archive window could not reload its page");
        }
    }

    /// Asks the reader to answer the check on screen: the overlay gives way.
    fn ask(&mut self) {
        self.status.ask();
        self.render();
        self.show(&self.texts.challenge);
    }

    fn on_stage(&mut self, stage: Stage) {
        self.cover.stage(stage, Instant::now());
        self.render();
    }

    /// A cookie banner left to the reader: the overlay gives way until it
    /// is gone.
    fn on_consent(&mut self, consent: &Consent) {
        consent.log();
        if !matches!(
            consent.state,
            ConsentState::Refused | ConsentState::Dismissed
        ) {
            self.cover.consent(consent.state);
            self.render();
        }
    }

    /// The reader cancelled the lookup: the overlay is gone.
    fn cancel(&mut self) {
        self.cover.cancel();
        self.render();
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
            &[],
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
                    progress,
                    texts,
                    attach,
                    onward,
                    drive,
                    hold,
                } => {
                    if shared.is_closed(session) {
                        continue;
                    }
                    let landed = Landed {
                        banner,
                        progress,
                        texts,
                        attach,
                        onward,
                        drive,
                        hold,
                    };
                    if let Some(window) = windows.get_mut(&session) {
                        window.load(&url, origins, landed);
                        continue;
                    }
                    let context =
                        context.get_or_insert_with(|| WebContext::new(Some(profile.clone())));
                    let opening = Opening {
                        session,
                        title: &title,
                        url: &url,
                        origins,
                        landed,
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
                Command::Stage { session, stage } => {
                    if let Some(window) = windows.get_mut(&session) {
                        window.on_stage(stage);
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
        Inbound::Posted(message) => receive_posted(shared, window, session, message),
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

/// Carries out what a window's page posted.
fn receive_posted(
    shared: &Shared,
    window: Option<&mut ArchiveWindow>,
    session: SessionId,
    message: Message,
) {
    match message {
        Message::Document => {
            if let Some(window) = window {
                window.on_document();
            }
        }
        Message::Page(page) => {
            log_page(&page);
            if let Some(window) = window {
                window.on_page(&page);
            }
            shared.seen(session, Seen::Page(page));
        }
        Message::Fetched(fetched) => shared.fetched(session, fetched),
        Message::Attach => {
            if let Some(window) = window {
                window.attach();
            }
        }
        Message::Dismiss => {
            if let Some(window) = window {
                window.status.dismiss();
            }
        }
        Message::Reload => {
            if let Some(window) = window {
                window.reload();
            }
        }
        Message::Consent(consent) => match window {
            Some(window) => window.on_consent(&consent),
            None => consent.log(),
        },
        Message::Driven(driven) => match window {
            Some(window) => window.on_driven(&driven),
            None => driven.log(),
        },
        Message::Cancel => {
            shared.cancel(session);
            if let Some(window) = window {
                window.cancel();
            }
        }
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        Message::OpenInBrowser => {
            if let Some(window) = window {
                window.open_in_browser();
            }
        }
    }
}

/// The banner over a server's error page of `status`: the portal did not
/// answer in time behind its gateway (`504`), or failed otherwise; with the
/// button loading the page again.
fn failure_banner(status: Option<u16>, texts: &Texts) -> Shown {
    let text = if status == Some(oxidgene_archives::GATEWAY_TIMEOUT) {
        &texts.page_timeout
    } else {
        &texts.page_error
    };
    (text.clone(), Some((texts.reload.clone(), "reload")))
}

fn log_page(page: &Page) {
    if page.state != PageState::Portal {
        debug!(
            vendor = page.vendor.as_deref().unwrap_or_default(),
            state = ?page.state,
            status = page.status,
            "an anti-bot or server error page in an archive window"
        );
    }
}

/// What a window shows over a page it loads, from a [`Command::Load`].
struct Landed {
    banner: Option<String>,
    progress: Option<Progress>,
    texts: Box<Texts>,
    attach: Option<Box<Attachable>>,
    /// For a landing on a portal's reuse licence, the target to go on to.
    onward: Option<Box<Onward>>,
    drive: Option<Box<Drive>>,
    hold: Vec<String>,
}

/// What a new window opens on.
struct Opening<'a> {
    session: SessionId,
    title: &'a str,
    url: &'a str,
    origins: Vec<String>,
    landed: Landed,
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
        Ok(Message::Page(mut page)) => {
            page.url = uri.to_string();
            inbox.push(session, Inbound::Posted(Message::Page(page)));
        }
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
        .with_initialization_script_for_main_only(script::consent(), true)
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

    let Landed {
        banner,
        progress,
        texts,
        attach,
        onward,
        drive,
        hold,
    } = opening.landed;
    let mut cover = Cover::default();
    cover.load(progress, Instant::now());
    Some(ArchiveWindow {
        window,
        webview,
        origins,
        status: Status::new(banner),
        cover,
        texts,
        attach,
        pending: onward.map(|onward| Pending::new(*onward)),
        driver: Driver::new(drive),
        hold,
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
        ArchiveViewerMessages::new(
            &oxidgene_ui::i18n::I18n::new(oxidgene_ui::i18n::Language::english()),
            &oxidgene_ui::theme::BUILTIN_THEMES[0],
        )
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
                // A page opened as it is has no progress overlay.
                progress: None,
                texts: Box::new(texts(&messages)),
                attach: None,
                onward: None,
                drive: None,
                // Its banner waits for the portal's dialogs.
                hold: hold(None, ""),
            }
        );
        // The window's own texts are the interface's.
        let texts = texts(&messages);
        assert_eq!(texts.challenge, messages.challenge);
        assert!(!messages.challenge.starts_with("archive_viewer."));
        for text in [
            &texts.connecting,
            &texts.looking_up,
            &texts.opening,
            &texts.opening_view,
            &texts.elapsed,
            &texts.cancel,
        ] {
            assert!(!text.starts_with("archive_viewer."), "{text}");
        }
        assert!(texts.opening_view.contains("{view}"));
        assert!(texts.elapsed.contains("{seconds}"));
        // The overlay is drawn in the application's theme.
        assert!(texts.palette.contains("--bg-deep:"));
        assert!(texts.palette.contains("--orange:"));
    }

    fn transport(link: &ArchiveLink, messages: &ArchiveViewerMessages) -> WindowTransport {
        WindowTransport {
            shared: Arc::new(Shared::default()),
            session: 9,
            title: link.title.clone(),
            archive: link.archive.name.clone(),
            texts: texts(messages),
        }
    }

    fn link() -> ArchiveLink {
        link_of("AD44 - Exampleville - (aucun) - N - 1877")
    }

    fn link_of(title: &str) -> ArchiveLink {
        let evidence = oxidgene_archives::CitationEvidence {
            title: title.to_owned(),
            ..Default::default()
        };
        match oxidgene_ui::archive_viewer::ArchiveOffer::of(Default::default(), None, evidence) {
            Some(oxidgene_ui::archive_viewer::ArchiveOffer::Register(link)) => link,
            _ => panic!("a catalogued citation"),
        }
    }

    #[test]
    fn a_viewer_without_an_address_per_view_is_brought_to_the_cited_view() {
        let messages = messages();
        let link = link_of("AD61 - Exampleville - (aucun) - BMS - 1760 - vue 178/396");
        let transport = transport(&link, &messages);
        let register = |url: &str| {
            Ok(ArchiveTarget::View {
                url: url.to_owned(),
                views: Vec::new(),
                view_count: None,
                call_number: Some("9 E 1".to_owned()),
                attribution: None,
                renumbering: None,
            })
        };
        let go_to = messages.banner(LandingBanner {
            key: "archive_viewer.go_to_view",
            view: Some(178),
            counts: None,
        });
        assert!(go_to.as_deref().is_some_and(|text| text.contains("178")));

        let viewer = "https://gaia.orne.fr/mdr/index.php/docnumViewer/calculHierarchieDocNum/1/1:2:3:4/900/1400";
        let Command::Load {
            banner,
            progress,
            drive,
            ..
        } = landing(&link, &messages, None, &transport, register(viewer))
        else {
            panic!("a load");
        };
        // The banner waits for the drive, under the step opening the view.
        assert_eq!(banner, None);
        assert_eq!(
            progress.map(|progress| progress.stage),
            Some(Stage::Opening { view: Some(178) })
        );
        let drive = drive.expect("a drive");
        assert!(drive.script.contains("\nconst view = 178;\n"));
        assert!(drive.script.contains("#pagination input[type=text]"));
        assert_eq!(drive.fallback, go_to);

        // A target no collection of the archive serves keeps the banner.
        let Command::Load { banner, drive, .. } = landing(
            &link,
            &messages,
            None,
            &transport,
            register("https://elsewhere.example.org/viewer"),
        ) else {
            panic!("a load");
        };
        assert_eq!((banner, drive), (go_to, None));
    }

    fn drive() -> Option<Box<Drive>> {
        Some(Box::new(Drive {
            script: "drive()".to_owned(),
            fallback: Some("Go to view 5.".to_owned()),
        }))
    }

    fn driven(state: DrivenState) -> Driven {
        Driven {
            state,
            reason: None,
        }
    }

    fn fallback() -> Option<Drove> {
        Some(Drove::Failed(Some("Go to view 5.".to_owned())))
    }

    #[test]
    fn a_drive_runs_on_the_landing_s_portal_page_until_it_ends() {
        // A check first: the drive waits for the portal's page.
        let mut driver = Driver::new(drive());
        assert_eq!(driver.page(PageState::Challenge), None);
        assert!(!driver.running());
        assert_eq!(driver.page(PageState::Portal).as_deref(), Some("drive()"));
        assert!(driver.running());
        assert_eq!(driver.page(PageState::Portal), None);
        assert_eq!(
            driver.driven(&driven(DrivenState::Shown)),
            Some(Drove::Shown)
        );
        assert!(!driver.running());
        // Over: what follows changes nothing.
        assert_eq!(driver.page(PageState::Portal), None);
        assert_eq!(driver.driven(&driven(DrivenState::Failed)), None);
        assert_eq!(driver.document(), None);

        let mut driver = Driver::new(drive());
        driver.page(PageState::Portal);
        assert_eq!(driver.driven(&driven(DrivenState::Failed)), fallback());
    }

    #[test]
    fn a_drive_whose_page_is_replaced_starts_once_more() {
        let mut driver = Driver::new(drive());
        driver.page(PageState::Portal);
        assert_eq!(driver.document(), None);
        assert!(!driver.running());
        assert!(driver.page(PageState::Portal).is_some());
        // Then the banner.
        assert_eq!(driver.document(), fallback());
        assert_eq!(driver.page(PageState::Portal), None);
    }

    #[test]
    fn nothing_changes_while_no_drive_runs() {
        assert_eq!(Driver::new(drive()).document(), None);
        assert_eq!(
            Driver::new(drive()).driven(&driven(DrivenState::Shown)),
            None
        );
        assert_eq!(Driver::new(None).page(PageState::Portal), None);
    }

    #[test]
    fn the_landing_opens_under_the_overlay_s_last_step() {
        let (link, messages) = (link(), messages());
        let transport = transport(&link, &messages);
        let target = ArchiveTarget::View {
            url: "https://archives.example.org/viewer/12".to_owned(),
            views: vec![oxidgene_archives::ArchiveView {
                view: 12,
                url: "https://archives.example.org/viewer/12".to_owned(),
                ark: None,
                image: None,
            }],
            view_count: Some(40),
            call_number: None,
            attribution: None,
            renumbering: None,
        };
        let Command::Load { progress, .. } =
            landing(&link, &messages, None, &transport, Ok(target))
        else {
            panic!("a load");
        };
        assert_eq!(
            progress.map(|progress| (progress.archive, progress.stage)),
            Some((link.archive.name.clone(), Stage::Opening { view: Some(12) }))
        );
        let Command::Load {
            progress, banner, ..
        } = landing(
            &link,
            &messages,
            None,
            &transport,
            Err(ResolveError::NoAdapter),
        )
        else {
            panic!("a load");
        };
        assert_eq!(
            progress.map(|progress| progress.stage),
            Some(Stage::Opening { view: None })
        );
        assert!(banner.is_some());
    }

    #[test]
    fn a_cancelled_lookup_lands_on_the_filtered_search_page_without_overlay() {
        let (link, messages) = (link(), messages());
        let Command::Load {
            url,
            banner,
            progress,
            ..
        } = cancelled_landing(&link, &transport(&link, &messages))
        else {
            panic!("a load");
        };
        assert_eq!(
            url,
            ArchiveRegistry::embedded()
                .offline_target(&link.citation)
                .unwrap()
                .url()
        );
        assert_eq!((banner, progress), (None, None));
    }

    #[test]
    fn the_overlay_s_messages_are_read() {
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "document"}"#),
            Ok(Message::Document)
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "cancel"}"#),
            Ok(Message::Cancel)
        ));
        // Like every other message, only from the archive's origin.
        let accepted = Mutex::new(vec!["https://archives.example.org".to_owned()]);
        let uri = |text: &str| text.parse::<dioxus::desktop::wry::http::Uri>().unwrap();
        let cancel = r#"{"kind": "cancel"}"#;
        assert!(matches!(
            read_message(
                &accepted,
                &uri("https://archives.example.org/robots.txt"),
                cancel
            ),
            Ok(Message::Cancel)
        ));
        assert!(read_message(&accepted, &uri("https://elsewhere.example.org/"), cancel).is_err());
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

    /// A server's error page, and the button of its banner.
    #[test]
    fn a_server_error_page_and_its_reload_are_read() {
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "page", "state": "error", "status": 504}"#),
            Ok(Message::Page(Page {
                state: PageState::Error,
                status: Some(504),
                ..
            }))
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "reload"}"#),
            Ok(Message::Reload)
        ));
    }

    #[test]
    fn the_drive_s_messages_are_read() {
        assert!(matches!(
            serde_json::from_str::<Message>(
                r#"{"kind": "driven", "state": "failed", "reason": "not_shown", "shown": 1}"#
            ),
            Ok(Message::Driven(Driven {
                state: DrivenState::Failed,
                ..
            }))
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "driven", "state": "shown"}"#),
            Ok(Message::Driven(Driven {
                state: DrivenState::Shown,
                reason: None,
            }))
        ));
    }

    #[test]
    fn the_consent_script_s_messages_are_read() {
        let read = |body: &str| match serde_json::from_str::<Message>(body) {
            Ok(Message::Consent(consent)) => Some(consent),
            _ => None,
        };
        assert_eq!(
            read(r#"{"kind": "consent", "state": "refused", "manager": "tarteaucitron"}"#),
            Some(Consent {
                state: ConsentState::Refused,
                manager: Some("tarteaucitron".to_owned()),
            })
        );
        for (state, expected) in [
            ("left", ConsentState::Left),
            ("closed", ConsentState::Closed),
            ("dismissed", ConsentState::Dismissed),
        ] {
            let consent = read(&format!(r#"{{"kind": "consent", "state": "{state}"}}"#));
            assert_eq!(consent.map(|consent| consent.state), Some(expected));
        }
        // OxidGene never accepts: there is no such outcome.
        assert_eq!(read(r#"{"kind": "consent", "state": "accepted"}"#), None);
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
                url: String::new(),
                status: None,
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
            status.page(PageState::Challenge, None, "Answer", None),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer", None),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer", None),
            banner("Searching…")
        );
        // A check the reader is asked to answer shows the request, on every
        // page of the check, until the portal's page shows.
        status.ask();
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer", None),
            banner("Answer")
        );
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer", None),
            banner("Answer")
        );
        assert_eq!(
            status.page(PageState::Portal, None, "Answer", None),
            banner("Searching…")
        );
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer", None),
            banner("Searching…")
        );
        // A block shows it too.
        assert_eq!(
            status.page(PageState::Blocked, None, "Answer", None),
            banner("Searching…")
        );
        // Closed, it is not shown again.
        status.dismiss();
        assert_eq!(status.page(PageState::Portal, None, "Answer", None), None);
        // A load without a banner shows none, but a request to answer a check.
        let mut status = Status::new(None);
        assert_eq!(status.page(PageState::Portal, None, "Answer", None), None);
        status.ask();
        assert_eq!(
            status.page(PageState::Challenge, None, "Answer", None),
            banner("Answer")
        );
    }

    /// A regression: a lookup timed out on a city's search, its landing on
    /// the same search met the portal's gateway timing out, and the window
    /// showed the gateway's bare `504` page.
    #[test]
    fn a_server_error_page_says_why_the_portal_failed() {
        let texts = texts(&messages());
        let timed_out = failure_banner(Some(504), &texts);
        let reload = Some((texts.reload.clone(), "reload"));
        assert_eq!(timed_out, (texts.page_timeout.clone(), reload.clone()));
        assert_ne!(texts.page_timeout, texts.page_error);
        for status in [Some(500), Some(503), None] {
            assert_eq!(
                failure_banner(status, &texts),
                (texts.page_error.clone(), reload.clone())
            );
        }
        // Over the landing of a failed lookup, and over a page whose load
        // had nothing to say.
        for banner in [Some("Timed out.".to_owned()), None] {
            let mut status = Status::new(banner);
            assert_eq!(
                status.page(PageState::Error, None, "Answer", Some(timed_out.clone())),
                Some(timed_out.clone())
            );
        }
        // Under the overlay of a lookup, the load's own banner, if any.
        let mut status = Status::new(None);
        assert_eq!(status.page(PageState::Error, None, "Answer", None), None);
        // Closed, it is not shown again.
        status.dismiss();
        assert_eq!(
            status.page(PageState::Error, None, "Answer", Some(timed_out)),
            None
        );
    }

    #[test]
    fn views_to_attach_show_the_offer_over_the_portal() {
        let attach = attachable();
        let offer = Some(("Attach".to_owned(), "attach"));
        let mut status = Status::new(None);
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer", None),
            Some(("Keep the view.".to_owned(), offer.clone()))
        );
        let mut status = Status::new(Some("Go to view 3.".to_owned()));
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer", None),
            Some(("Go to view 3.".to_owned(), offer))
        );
        status.dismiss();
        assert_eq!(
            status.page(PageState::Portal, Some(&attach), "Answer", None),
            None
        );
    }

    /// Regressions: the offer to attach showed over the Val-d'Oise viewer's
    /// reuse-licence dialog, and a banner over the Cantal viewer's « licence
    /// clic ». Banners over a portal's page wait for its modal dialogs, the
    /// cookie banners OxidGene recognizes, and the target viewer's licence.
    #[test]
    fn banners_wait_for_the_portal_s_dialogs() {
        let messages = messages();
        let transport = |link: &ArchiveLink| transport(link, &messages);
        let view = |url: &str, view: u16| ArchiveTarget::View {
            url: url.to_owned(),
            views: vec![oxidgene_archives::ArchiveView {
                view,
                url: url.to_owned(),
                ark: None,
                image: None,
            }],
            view_count: Some(396),
            call_number: Some("9 E 1".to_owned()),
            attribution: None,
            renumbering: None,
        };

        let link = link_of("AD95 - Exampleville - (aucun) - N - 1877 - vue 3/40");
        let url = "https://archives.valdoise.fr/ark:/00000/a1/daogrp/0/3";
        let Command::Load { hold, .. } =
            landing(&link, &messages, None, &transport(&link), Ok(view(url, 3)))
        else {
            panic!("a load");
        };
        assert_eq!(hold[0], MODAL_DIALOGS);
        assert!(hold.contains(&"#tarteaucitronAlertBig".to_owned()));

        // An Arkothèque viewer behind its licence: the banner waits for the
        // licence, and the window opens the target as it is — the viewer
        // shows the cited view itself once the reader accepts the licence.
        let link = link_of("AD15 - Exampleville - (aucun) - BMS - 1760 - vue 191g/396");
        let url = "https://www.archives.cantal.fr/viewer#/_recherche-api/visionneuse-infos/1/2/3/image/4/191";
        let Command::Load {
            hold,
            url: loaded,
            drive,
            ..
        } = landing(
            &link,
            &messages,
            None,
            &transport(&link),
            Ok(view(url, 191)),
        )
        else {
            panic!("a load");
        };
        assert!(hold.contains(&r#"button[data-cy="accept-license"]"#.to_owned()));
        assert_eq!((loaded.as_str(), drive), (url, None));
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

    /// The owner's case, anonymized: a register of the Côtes-d'Armor behind
    /// the portal's reuse licence. The window lands on the site's entry,
    /// asking the reader to accept the licence, and goes on to the cited view
    /// only once a page behind the licence shows after the licence page.
    #[test]
    fn a_landing_on_a_licence_goes_on_once_the_reader_has_passed_it() {
        let messages = messages();
        let evidence = oxidgene_archives::CitationEvidence {
            title: "AD22 - Exampleville - N - 1796-1800 - acte 65 - vue 36/248".to_owned(),
            ..Default::default()
        };
        let Some(oxidgene_ui::archive_viewer::ArchiveOffer::Register(link)) =
            oxidgene_ui::archive_viewer::ArchiveOffer::of(Default::default(), None, evidence)
        else {
            panic!("a catalogued citation");
        };
        let site = "https://sallevirtuelle.cotesdarmor.fr/EC/ecx";
        let view = format!("{site}/consult.aspx?image=910020100000036");
        let target = ArchiveTarget::View {
            url: view.clone(),
            views: vec![oxidgene_archives::ArchiveView {
                view: 36,
                url: view.clone(),
                ark: None,
                image: None,
            }],
            view_count: Some(248),
            call_number: None,
            attribution: None,
            renumbering: None,
        };
        let Command::Load {
            url,
            banner,
            onward,
            ..
        } = landing(
            &link,
            &messages,
            None,
            &transport(&link, &messages),
            Ok(target),
        )
        else {
            panic!("a load");
        };
        assert_eq!(url, format!("{site}/connexion.aspx?ref=demo&res=1920x1080"));
        assert!(banner.is_some_and(|banner| !banner.starts_with("archive_viewer.")));
        let onward = *onward.expect("the target behind the licence");
        assert_eq!(onward.url, view);

        let mut pending = Pending::new(onward);
        // The page being left, before the licence showed: no.
        assert!(!pending.passed(PageState::Portal, &format!("{site}/commune.aspx")));
        // The licence page, then an anti-bot page: no.
        assert!(!pending.passed(PageState::Portal, &format!("{site}/licence.aspx")));
        assert!(!pending.passed(PageState::Challenge, &format!("{site}/commune.aspx")));
        // Another site's page: no.
        assert!(!pending.passed(PageState::Portal, "https://archives.example.org/"));
        // The page the reader's acceptance leads to: the window goes on.
        assert!(pending.passed(PageState::Portal, &format!("{site}/commune.aspx")));
    }
}
