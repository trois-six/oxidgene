//! The `window` transport: an adapter's requests run as the portal page's
//! own `fetch` inside the archive window.
//!
//! The resolver runs on the Dioxus runtime; a window can only be driven from
//! the event loop. The two meet in [`Shared`]: the transport queues
//! [`Command`]s that the loop's handler carries out on the next event, and
//! waits on channels that the handler feeds when the window posts back over
//! IPC: what each page is, and the answer of each request. Feeding a channel
//! wakes the Dioxus task, which wakes the loop, so the exchange never waits
//! for an unrelated event.
//!
//! Before any request, the window shows the portal's own page, waiting out
//! an anti-bot check on the way ([`Gate`]): a check that clears itself is
//! given a few seconds, one that stays or shows a widget is the reader's to
//! answer in the window, and a block ends the search at once.
//!
//! Meanwhile the window's progress overlay says where the resolution stands
//! ([`Stage`]); the reader may cancel it there ([`Shared::cancel`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_channel::oneshot;
use oxidgene_archives::platform::BoxFuture;
use oxidgene_archives::transport::{Guard, PageAnswer, TIMEOUT, anti_bot, page_url, request_url};
use oxidgene_archives::{
    ArchiveTarget, FetchError, PortalEndpoint, PortalFetch, PortalRequest, PortalTransport,
};
use oxidgene_ui::archive_viewer::AttachSender;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::debug;

use super::script;

/// One opening of a register, and its window.
pub(super) type SessionId = u64;

/// How long the window may take to show a page of the portal.
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// How long an anti-bot check may take to clear itself before the reader
/// is asked to answer it.
const AUTOMATIC_CHECK: Duration = Duration::from_secs(5);

/// How long the reader has to answer a check in the window.
const READER_DEADLINE: Duration = Duration::from_secs(180);

/// A margin over the page's own request timeout, after which a request the
/// page never answered is given up.
const ANSWER_MARGIN: Duration = Duration::from_secs(5);

/// What the window says, in the interface language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Texts {
    /// Heads the progress overlay.
    pub(super) searching: String,
    /// The overlay's steps ([`Stage`]), `opening_view` with a `{view}`
    /// placeholder.
    pub(super) connecting: String,
    pub(super) looking_up: String,
    pub(super) opening: String,
    pub(super) opening_view: String,
    /// How long a step has lasted, with a `{seconds}` placeholder.
    pub(super) elapsed: String,
    /// The label of the overlay's button stopping the lookup.
    pub(super) cancel: String,
    /// The active theme's custom properties, which the overlay declares.
    pub(super) palette: String,
    /// The label of a banner's close button.
    pub(super) close: String,
    /// Asks the reader to answer an anti-bot check in the window.
    pub(super) challenge: String,
    /// Says that the portal's certificate could not be verified.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    pub(super) certificate: String,
    /// The label of the button opening the page in the system browser.
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    pub(super) open_in_browser: String,
}

/// The views of an archive whose images OxidGene may use, shown in a window:
/// a banner offers to attach them, and the reader's click sends the target
/// to the interface, which opens the document form in its own window
/// (docs/archives.md §6.1, §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Attachable {
    pub(super) sender: AttachSender,
    pub(super) target: ArchiveTarget,
    /// What the banner says, and its button's label.
    pub(super) hint: String,
    pub(super) label: String,
}

/// Where a resolution stands, as its progress overlay says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// The start page loads, through any anti-bot check.
    Connecting,
    /// An adapter's request runs.
    Searching,
    /// The window loads where the resolution ended: the cited view, when
    /// the target opens on it.
    Opening { view: Option<u16> },
}

impl Stage {
    /// The step's text.
    pub(super) fn text(self, texts: &Texts) -> String {
        match self {
            Self::Connecting => texts.connecting.clone(),
            Self::Searching => texts.looking_up.clone(),
            Self::Opening { view: None } => texts.opening.clone(),
            Self::Opening { view: Some(view) } => {
                texts.opening_view.replace("{view}", &view.to_string())
            }
        }
    }
}

/// The progress overlay covering the window while a resolution runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Progress {
    /// The archive's name.
    pub(super) archive: String,
    /// The citation being resolved: the source title.
    pub(super) citation: String,
    pub(super) stage: Stage,
}

/// Work for the event loop.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Command {
    /// Loads `url` in the session's window, creating the window if needed,
    /// and shows `banner` over the page once it is the portal's, and
    /// `progress` over the pages until the resolution lands. IPC messages
    /// are then accepted from `origins` only.
    Load {
        session: SessionId,
        title: String,
        url: String,
        origins: Vec<String>,
        banner: Option<String>,
        progress: Option<Progress>,
        texts: Box<Texts>,
        /// The views the page shows, which the reader may attach.
        attach: Option<Box<Attachable>>,
    },
    /// The resolution moved on to `stage`.
    Stage { session: SessionId, stage: Stage },
    /// Asks the reader to answer the check on screen: the banner stays over
    /// every check page of the window until the portal's page shows.
    Ask { session: SessionId },
    /// Runs one request's script in the session's window.
    Fetch {
        session: SessionId,
        ticket: u64,
        script: String,
    },
}

/// What a page of the window is, as `page.js` posts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PageState {
    /// The portal's own page, rendered.
    Portal,
    /// An anti-bot check, which a browser passes.
    Challenge,
    /// An anti-bot refusal, which nobody passes from here.
    Blocked,
}

/// One page of the window, classified.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct Page {
    pub(super) state: PageState,
    /// The anti-bot vendor of a check or a block.
    #[serde(default)]
    pub(super) vendor: Option<String>,
    /// Whether a check shows a widget for the reader to answer.
    #[serde(default)]
    pub(super) interactive: bool,
}

/// What a waiting connection hears of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Seen {
    Page(Page),
    /// The page's certificate could not be verified: nothing will load.
    Untrusted,
}

/// Where a connection stands while the window shows the start page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    /// The page is loading.
    Loading,
    /// A check is on screen, given a few seconds to clear itself.
    Automatic,
    /// The reader was asked to answer the check.
    Interactive,
}

/// What a connection does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Step {
    /// Waits for the next page, or until [`Gate::until`].
    Wait,
    /// Asks the reader to answer the check, then waits.
    Ask,
    /// The portal's page is on screen: requests may run.
    Pass,
    Fail(FetchError),
}

/// The decisions of a connection waiting for the portal's page, apart
/// from the clock and the window so that they can be tested.
///
/// - The portal's page passes; a block fails at once as a challenge.
/// - A check gives the page [`AUTOMATIC_CHECK`] to clear itself, as
///   Anubis's proof of work, F5's script or a bot-mitigation redirect do;
///   one still on screen after that, or showing a widget, is the reader's
///   to answer, within [`READER_DEADLINE`], after which it is a challenge.
/// - A page that never shows anything is a timeout after [`LOAD_TIMEOUT`];
///   one whose certificate is refused is unreachable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Gate {
    wait: Wait,
    until: Instant,
}

impl Gate {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            wait: Wait::Loading,
            until: now + LOAD_TIMEOUT,
        }
    }

    /// When the gate decides on its own if nothing is seen before.
    pub(super) fn until(&self) -> Instant {
        self.until
    }

    pub(super) fn seen(&mut self, seen: &Seen, now: Instant) -> Step {
        let page = match seen {
            Seen::Untrusted => return Step::Fail(FetchError::Network),
            Seen::Page(page) => page,
        };
        match page.state {
            PageState::Portal => Step::Pass,
            PageState::Blocked => Step::Fail(FetchError::Challenged),
            PageState::Challenge if self.wait == Wait::Interactive => Step::Wait,
            PageState::Challenge if page.interactive => self.ask(now),
            PageState::Challenge => {
                if self.wait == Wait::Loading {
                    self.wait = Wait::Automatic;
                    self.until = now + AUTOMATIC_CHECK;
                }
                Step::Wait
            }
        }
    }

    /// [`until`](Self::until) has passed with nothing decisive seen.
    pub(super) fn elapsed(&mut self, now: Instant) -> Step {
        match self.wait {
            Wait::Loading => Step::Fail(FetchError::Timeout),
            Wait::Automatic => self.ask(now),
            Wait::Interactive => Step::Fail(FetchError::Challenged),
        }
    }

    fn ask(&mut self, now: Instant) -> Step {
        self.wait = Wait::Interactive;
        self.until = now + READER_DEADLINE;
        Step::Ask
    }
}

/// How the window answered a request.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reply {
    Answer(Result<String, FetchError>),
    /// An anti-bot check answered in place of the portal: one the reader may
    /// pass on the start page.
    Challenge,
}

impl Reply {
    /// Reads the page's answer to a request: checked by
    /// [`PageAnswer::result`], with an answer bearing an anti-bot check's
    /// signature, whatever its status, set apart, and a block's a challenge
    /// error.
    pub(super) fn of(answer: PageAnswer, origins: &[String]) -> Self {
        let guard = answer
            .body
            .as_deref()
            .and_then(anti_bot)
            .map(|signature| signature.guard);
        match (answer.result(origins), guard) {
            (Ok(_) | Err(FetchError::Challenged), Some(Guard::Challenge)) => Self::Challenge,
            (Ok(_), Some(Guard::Block)) => Self::Answer(Err(FetchError::Challenged)),
            (result, _) => Self::Answer(result),
        }
    }
}

/// A request waiting for the window's answer.
struct Waiting {
    session: SessionId,
    origins: Vec<String>,
    reply: oneshot::Sender<Reply>,
}

/// What the resolution tasks and the event loop share.
#[derive(Default)]
pub(super) struct Shared {
    commands: Mutex<Vec<Command>>,
    pages: Mutex<HashMap<SessionId, mpsc::UnboundedSender<Seen>>>,
    waiting: Mutex<HashMap<u64, Waiting>>,
    closed: Mutex<HashSet<SessionId>>,
    /// How to stop each running resolution, when the reader cancels it.
    cancels: Mutex<HashMap<SessionId, oneshot::Sender<()>>>,
    next: AtomicU64,
}

/// What the fetch script posts back: the ticket of its request and the
/// page's answer, read by [`Reply::of`].
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub(super) struct Fetched {
    ticket: u64,
    #[serde(flatten)]
    answer: PageAnswer,
}

impl Shared {
    pub(super) fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn push(&self, command: Command) {
        if let Ok(mut commands) = self.commands.lock() {
            commands.push(command);
        }
    }

    pub(super) fn drain(&self) -> Vec<Command> {
        self.commands
            .lock()
            .map(|mut commands| commands.drain(..).collect())
            .unwrap_or_default()
    }

    pub(super) fn is_closed(&self, session: SessionId) -> bool {
        self.closed
            .lock()
            .is_ok_and(|closed| closed.contains(&session))
    }

    /// The session's window showed a page, or refused one: a connection
    /// waiting on it hears so.
    pub(super) fn seen(&self, session: SessionId, seen: Seen) {
        if let Ok(pages) = self.pages.lock()
            && let Some(listener) = pages.get(&session)
        {
            let _ = listener.send(seen);
        }
    }

    /// The session's window answered a request. An answer for a request of
    /// another session, or one no longer waited for, is ignored.
    pub(super) fn fetched(&self, session: SessionId, fetched: Fetched) {
        let Ok(mut waiting) = self.waiting.lock() else {
            return;
        };
        if waiting
            .get(&fetched.ticket)
            .is_none_or(|request| request.session != session)
        {
            return;
        }
        if let Some(request) = waiting.remove(&fetched.ticket) {
            let _ = request
                .reply
                .send(Reply::of(fetched.answer, &request.origins));
        }
    }

    /// The request could not be run: its window is gone.
    pub(super) fn abandon(&self, ticket: u64) {
        let request = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut waiting| waiting.remove(&ticket));
        if let Some(request) = request {
            let _ = request.reply.send(Reply::Answer(Err(FetchError::Network)));
        }
    }

    /// The reader closed the session's window, or it could not be opened:
    /// what waits on it fails, and it is not reopened.
    pub(super) fn close(&self, session: SessionId) {
        if let Ok(mut closed) = self.closed.lock() {
            closed.insert(session);
        }
        // The resolution fails on its own as its waits do.
        if let Ok(mut cancels) = self.cancels.lock() {
            cancels.remove(&session);
        }
        self.forget(session);
    }

    /// Drops what waits on the session's window.
    fn forget(&self, session: SessionId) {
        if let Ok(mut pages) = self.pages.lock() {
            pages.remove(&session);
        }
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.retain(|_, request| request.session != session);
        }
    }

    /// A resolution starts in the session's window: the receiver hears when
    /// the reader cancels it, until [`Self::settle`].
    pub(super) fn cancellable(&self, session: SessionId) -> oneshot::Receiver<()> {
        let (cancel, cancelled) = oneshot::channel();
        if let Ok(mut cancels) = self.cancels.lock() {
            cancels.insert(session, cancel);
        }
        cancelled
    }

    /// The session's resolution ended: it can no longer be cancelled.
    pub(super) fn settle(&self, session: SessionId) {
        if let Ok(mut cancels) = self.cancels.lock() {
            cancels.remove(&session);
        }
    }

    /// The reader cancelled the session's resolution: it stops, and what
    /// it waited on in the window is dropped. The window stays open.
    pub(super) fn cancel(&self, session: SessionId) {
        let cancel = self
            .cancels
            .lock()
            .ok()
            .and_then(|mut cancels| cancels.remove(&session));
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
            self.forget(session);
        }
    }

    /// Listens to the pages of the session's window until [`Self::ignore`].
    fn listen(&self, session: SessionId) -> mpsc::UnboundedReceiver<Seen> {
        let (sender, receiver) = mpsc::unbounded_channel();
        if let Ok(mut pages) = self.pages.lock() {
            pages.insert(session, sender);
        }
        receiver
    }

    fn ignore(&self, session: SessionId) {
        if let Ok(mut pages) = self.pages.lock() {
            pages.remove(&session);
        }
    }

    fn wait_for_answer(
        &self,
        session: SessionId,
        origins: Vec<String>,
    ) -> (u64, oneshot::Receiver<Reply>) {
        let ticket = self.next_id();
        let (reply, receiver) = oneshot::channel();
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(
                ticket,
                Waiting {
                    session,
                    origins,
                    reply,
                },
            );
        }
        (ticket, receiver)
    }
}

/// Requests through one session's archive window.
pub(super) struct WindowTransport {
    pub(super) shared: Arc<Shared>,
    pub(super) session: SessionId,
    /// The window's title: the source title.
    pub(super) title: String,
    /// The archive's name, which the progress overlay shows.
    pub(super) archive: String,
    pub(super) texts: Texts,
}

impl WindowTransport {
    /// The progress overlay at `stage`.
    pub(super) fn progress(&self, stage: Stage) -> Progress {
        Progress {
            archive: self.archive.clone(),
            citation: self.title.clone(),
            stage,
        }
    }

    /// Loads the endpoint's start page and waits until the window shows the
    /// portal's own page, through any anti-bot check ([`Gate`]).
    async fn show_portal(&self, endpoint: &PortalEndpoint) -> Result<(), FetchError> {
        self.show(&endpoint.start, endpoint, Stage::Connecting, "start_page")
            .await
    }

    /// Loads `url`, a page of the endpoint's portal, under the overlay at
    /// `stage`, and waits until the window shows the portal's own page,
    /// through any anti-bot check ([`Gate`]). `step` names the load in the
    /// log.
    async fn show(
        &self,
        url: &str,
        endpoint: &PortalEndpoint,
        stage: Stage,
        step: &'static str,
    ) -> Result<(), FetchError> {
        if self.shared.is_closed(self.session) {
            return Err(FetchError::Network);
        }
        let mut pages = self.shared.listen(self.session);
        self.shared.push(Command::Load {
            session: self.session,
            title: self.title.clone(),
            url: url.to_owned(),
            origins: vec![endpoint.origin.clone()],
            banner: None,
            progress: Some(self.progress(stage)),
            texts: Box::new(self.texts.clone()),
            attach: None,
        });
        let started = Instant::now();
        let mut gate = Gate::new(started);
        let shown = loop {
            let until = tokio::time::Instant::from_std(gate.until());
            let step = match tokio::time::timeout_at(until, pages.recv()).await {
                Ok(Some(seen)) => gate.seen(&seen, Instant::now()),
                // The window closed.
                Ok(None) => Step::Fail(FetchError::Network),
                Err(_) => gate.elapsed(Instant::now()),
            };
            match step {
                Step::Wait => {}
                Step::Ask => self.shared.push(Command::Ask {
                    session: self.session,
                }),
                Step::Pass => break Ok(()),
                Step::Fail(error) => break Err(error),
            }
        };
        self.shared.ignore(self.session);
        // What the reader may tell, from a terminal, of a lookup that ended
        // badly: which step, how, and after how long; never the citation
        // nor an address, which carries it.
        debug!(
            step,
            outcome = shown.as_ref().err().map_or("shown", fetch_error_code),
            elapsed_ms = elapsed_ms(started),
            "archive window"
        );
        shown
    }
}

impl PortalTransport for WindowTransport {
    fn is_browser(&self) -> bool {
        true
    }

    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
        Box::pin(async move {
            self.show_portal(endpoint).await?;
            Ok(Box::new(WindowFetch {
                transport: self,
                endpoint: endpoint.clone(),
                passed_again: AtomicBool::new(false),
            }) as Box<dyn PortalFetch>)
        })
    }
}

struct WindowFetch<'a> {
    transport: &'a WindowTransport,
    endpoint: PortalEndpoint,
    /// Whether the reader was already sent back to the start page once.
    passed_again: AtomicBool,
}

impl WindowFetch<'_> {
    async fn send(&self, url: &str, request: &PortalRequest) -> Reply {
        let started = Instant::now();
        let reply = self
            .exchange(|ticket| {
                script::fetch(
                    ticket,
                    request.method,
                    url,
                    &request.headers,
                    request.body.as_deref(),
                )
            })
            .await;
        log_reply("request", request.method.as_str(), &reply, started);
        reply
    }

    /// Runs the script `script` builds for a ticket in the window's page,
    /// and waits for the answer it posts under that ticket.
    async fn exchange(&self, script: impl FnOnce(u64) -> String) -> Reply {
        let shared = &self.transport.shared;
        let origins = self.endpoint.origins().map(str::to_owned).collect();
        let (ticket, answer) = shared.wait_for_answer(self.transport.session, origins);
        shared.push(Command::Stage {
            session: self.transport.session,
            stage: Stage::Searching,
        });
        shared.push(Command::Fetch {
            session: self.transport.session,
            ticket,
            script: script(ticket),
        });
        match tokio::time::timeout(TIMEOUT + ANSWER_MARGIN, answer).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Reply::Answer(Err(FetchError::Network)),
            Err(_) => {
                shared.abandon(ticket);
                Reply::Answer(Err(FetchError::Timeout))
            }
        }
    }
}

/// Logs how the window answered a request or a page load: which step, how,
/// and after how long; never the citation nor an address, which carries it.
fn log_reply(step: &'static str, method: &'static str, reply: &Reply, started: Instant) {
    let (outcome, status) = match reply {
        Reply::Answer(Ok(_)) => ("answered", None),
        Reply::Answer(Err(FetchError::Status(status))) => ("status", Some(*status)),
        Reply::Answer(Err(error)) => (fetch_error_code(error), None),
        Reply::Challenge => ("anti_bot_check", None),
    };
    debug!(
        step,
        method,
        outcome,
        status,
        elapsed_ms = elapsed_ms(started),
        "archive window"
    );
}

/// How a step of the window failed, for its log.
fn fetch_error_code(error: &FetchError) -> &'static str {
    match error {
        FetchError::Timeout => "timeout",
        FetchError::Network => "network",
        FetchError::Status(_) => "status",
        FetchError::Challenged => "challenged",
        FetchError::TooLarge => "too_large",
        FetchError::NotSameOrigin => "not_same_origin",
        FetchError::NotAllowed => "not_allowed",
    }
}

fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

impl PortalFetch for WindowFetch<'_> {
    /// Sends the request from the portal's page. When an anti-bot check
    /// answers it, the window loads the start page again, where the reader
    /// faces the check, and once the portal's page shows, the same request
    /// is sent once more: at most once per connection, and never for any
    /// other failure.
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let url = request_url(&self.endpoint, request)?;
            match self.send(&url, request).await {
                Reply::Answer(result) => result,
                Reply::Challenge if self.passed_again.swap(true, Ordering::Relaxed) => {
                    Err(FetchError::Challenged)
                }
                Reply::Challenge => {
                    self.transport.show_portal(&self.endpoint).await?;
                    match self.send(&url, request).await {
                        Reply::Answer(result) => result,
                        Reply::Challenge => Err(FetchError::Challenged),
                    }
                }
            }
        })
    }

    /// Loads the page in the window, through any anti-bot check the reader
    /// may answer there ([`Gate`]), then waits for `ready` in its document
    /// and reads the document back, as a request's answer.
    fn page<'a>(
        &'a self,
        path_and_query: &'a str,
        ready: &'a str,
    ) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move {
            let url = page_url(&self.endpoint, path_and_query, ready)?;
            let started = Instant::now();
            self.transport
                .show(&url, &self.endpoint, Stage::Searching, "page")
                .await?;
            let reply = self
                .exchange(|ticket| script::rendered(ticket, ready))
                .await;
            log_reply("page", "GET", &reply, started);
            match reply {
                Reply::Answer(result) => result,
                Reply::Challenge => Err(FetchError::Challenged),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_archives::Access;

    use super::*;

    fn origins() -> Vec<String> {
        vec!["https://archives.example.org".to_owned()]
    }

    fn page_answer(status: u16, url: &str, body: &str) -> PageAnswer {
        PageAnswer {
            status: Some(status),
            url: Some(url.to_owned()),
            body: Some(body.to_owned()),
            error: None,
        }
    }

    fn answer(ticket: u64, status: u16, url: &str, body: &str) -> Fetched {
        Fetched {
            ticket,
            answer: page_answer(status, url, body),
        }
    }

    fn page(state: PageState, interactive: bool) -> Seen {
        Seen::Page(Page {
            state,
            vendor: None,
            interactive,
        })
    }

    fn texts() -> Texts {
        Texts {
            searching: "Looking for the register…".to_owned(),
            connecting: "Connecting…".to_owned(),
            looking_up: "Searching…".to_owned(),
            opening: "Opening…".to_owned(),
            opening_view: "Opening view {view}…".to_owned(),
            elapsed: "{seconds} s".to_owned(),
            cancel: "Cancel".to_owned(),
            palette: "    --bg-deep: #000001;\n".to_owned(),
            close: "Close".to_owned(),
            challenge: "Answer the check.".to_owned(),
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            certificate: "Unverified.".to_owned(),
            #[cfg(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            ))]
            open_in_browser: "Open".to_owned(),
        }
    }

    const PORTAL: &str = "<html><body>Registres</body></html>";
    const CHECK: &str = "<html><script>window.location.href='/redirect_0000/x'</script></html>";
    const BLOCK: &str =
        "<html><div id=\"cf-error-details\">Sorry, you have been blocked</div></html>";

    /// The checks themselves are `PageAnswer`'s, tested in
    /// `oxidgene-archives`.
    #[test]
    fn reads_the_ticket_beside_the_answer() {
        let parsed: Fetched = serde_json::from_str(
            r#"{"ticket": 3, "status": 200, "url": "https://archives.example.org/", "body": "x"}"#,
        )
        .unwrap();
        assert_eq!(parsed, answer(3, 200, "https://archives.example.org/", "x"));
        assert_eq!(
            Reply::of(parsed.answer, &origins()),
            Reply::Answer(Ok("x".to_owned()))
        );
        let failed: Fetched = serde_json::from_str(r#"{"ticket": 4, "error": "timeout"}"#).unwrap();
        assert_eq!(failed.ticket, 4);
        assert_eq!(
            Reply::of(failed.answer, &origins()),
            Reply::Answer(Err(FetchError::Timeout))
        );
    }

    #[test]
    fn sets_a_check_answering_a_request_apart_from_a_block() {
        let url = "https://archives.example.org/api";
        for status in [200, 403] {
            assert_eq!(
                Reply::of(page_answer(status, url, CHECK), &origins()),
                Reply::Challenge
            );
            assert_eq!(
                Reply::of(page_answer(status, url, BLOCK), &origins()),
                Reply::Answer(Err(FetchError::Challenged))
            );
        }
        // A check elsewhere is still an answer from elsewhere.
        assert_eq!(
            Reply::of(
                page_answer(200, "https://elsewhere.example.org/", CHECK),
                &origins()
            ),
            Reply::Answer(Err(FetchError::NotSameOrigin))
        );
        assert_eq!(
            Reply::of(page_answer(200, url, PORTAL), &origins()),
            Reply::Answer(Ok(PORTAL.to_owned()))
        );
    }

    #[test]
    fn answers_reach_their_own_request_only() {
        let shared = Shared::default();
        let (ticket, mut receiver) = shared.wait_for_answer(1, origins());
        let url = "https://archives.example.org/";

        // Another session's window cannot answer it.
        shared.fetched(2, answer(ticket, 200, url, "forged"));
        assert_eq!(receiver.try_recv(), Ok(None));

        shared.fetched(1, answer(ticket, 200, url, "body"));
        assert_eq!(
            receiver.try_recv(),
            Ok(Some(Reply::Answer(Ok("body".to_owned()))))
        );
        // A second answer to the same ticket is ignored.
        shared.fetched(1, answer(ticket, 200, url, "again"));
    }

    #[test]
    fn closing_a_window_fails_what_waits_on_it() {
        let shared = Shared::default();
        let mut pages = shared.listen(4);
        let (_, mut answer) = shared.wait_for_answer(4, origins());
        let (other_ticket, mut other) = shared.wait_for_answer(5, origins());

        shared.close(4);
        assert!(shared.is_closed(4));
        assert!(pages.try_recv().is_err());
        assert!(answer.try_recv().is_err());
        assert_eq!(other.try_recv(), Ok(None));

        shared.abandon(other_ticket);
        assert_eq!(
            other.try_recv(),
            Ok(Some(Reply::Answer(Err(FetchError::Network))))
        );
    }

    #[test]
    fn pages_reach_the_connection_of_their_window() {
        let shared = Shared::default();
        let mut pages = shared.listen(6);
        shared.seen(7, page(PageState::Portal, false));
        assert!(pages.try_recv().is_err());
        shared.seen(6, page(PageState::Portal, false));
        assert_eq!(pages.try_recv(), Ok(page(PageState::Portal, false)));
        shared.ignore(6);
        shared.seen(6, page(PageState::Portal, false));
        assert!(pages.try_recv().is_err());
    }

    #[test]
    fn commands_are_queued_for_the_event_loop() {
        let shared = Shared::default();
        let command = Command::Fetch {
            session: 1,
            ticket: shared.next_id(),
            script: String::new(),
        };
        shared.push(command);
        assert_eq!(shared.drain().len(), 1);
        assert!(shared.drain().is_empty());
    }

    #[test]
    fn a_check_that_clears_itself_passes_without_the_reader() {
        let start = Instant::now();
        let mut gate = Gate::new(start);
        assert_eq!(gate.until(), start + LOAD_TIMEOUT);
        let soon = start + Duration::from_secs(1);
        assert_eq!(
            gate.seen(&page(PageState::Challenge, false), soon),
            Step::Wait
        );
        assert_eq!(gate.until(), soon + AUTOMATIC_CHECK);
        // The check's next page, then the portal's.
        let later = soon + Duration::from_secs(2);
        assert_eq!(
            gate.seen(&page(PageState::Challenge, false), later),
            Step::Wait
        );
        assert_eq!(gate.until(), soon + AUTOMATIC_CHECK);
        assert_eq!(
            gate.seen(&page(PageState::Portal, false), later),
            Step::Pass
        );
    }

    #[test]
    fn a_check_that_stays_is_the_reader_s_within_a_deadline() {
        let start = Instant::now();
        let mut gate = Gate::new(start);
        gate.seen(&page(PageState::Challenge, false), start);
        let asked = start + AUTOMATIC_CHECK;
        assert_eq!(gate.elapsed(asked), Step::Ask);
        assert_eq!(gate.until(), asked + READER_DEADLINE);
        // Asked once: the check's widget, and its next pages, wait.
        assert_eq!(
            gate.seen(&page(PageState::Challenge, true), asked),
            Step::Wait
        );
        assert_eq!(
            gate.seen(&page(PageState::Challenge, false), asked),
            Step::Wait
        );
        assert_eq!(gate.until(), asked + READER_DEADLINE);

        let mut answered = gate.clone();
        assert_eq!(
            answered.seen(
                &page(PageState::Portal, false),
                asked + Duration::from_secs(60)
            ),
            Step::Pass
        );
        assert_eq!(
            gate.elapsed(asked + READER_DEADLINE),
            Step::Fail(FetchError::Challenged)
        );
    }

    #[test]
    fn a_widget_asks_the_reader_at_once() {
        let start = Instant::now();
        let mut gate = Gate::new(start);
        assert_eq!(
            gate.seen(&page(PageState::Challenge, true), start),
            Step::Ask
        );
        assert_eq!(gate.until(), start + READER_DEADLINE);
    }

    #[test]
    fn a_block_a_blank_page_and_a_refused_certificate_fail() {
        let start = Instant::now();
        assert_eq!(
            Gate::new(start).seen(&page(PageState::Blocked, false), start),
            Step::Fail(FetchError::Challenged)
        );
        let mut asked = Gate::new(start);
        asked.seen(&page(PageState::Challenge, true), start);
        assert_eq!(
            asked.seen(&page(PageState::Blocked, false), start),
            Step::Fail(FetchError::Challenged)
        );
        assert_eq!(
            Gate::new(start).elapsed(start + LOAD_TIMEOUT),
            Step::Fail(FetchError::Timeout)
        );
        assert_eq!(
            Gate::new(start).seen(&Seen::Untrusted, start),
            Step::Fail(FetchError::Network)
        );
    }

    #[test]
    fn reads_the_page_messages() {
        let parsed: Page = serde_json::from_str(
            r#"{"kind": "page", "state": "challenge", "vendor": "anubis", "interactive": false}"#,
        )
        .unwrap();
        assert_eq!(parsed.state, PageState::Challenge);
        assert_eq!(parsed.vendor.as_deref(), Some("anubis"));
        let portal: Page = serde_json::from_str(r#"{"state": "portal"}"#).unwrap();
        assert_eq!(portal.state, PageState::Portal);
        assert!(!portal.interactive);
    }

    fn endpoint() -> PortalEndpoint {
        PortalEndpoint {
            origin: "https://archives.example.org".to_owned(),
            other_origins: Vec::new(),
            start: "https://archives.example.org/search".to_owned(),
            access: Access::Browser,
        }
    }

    /// Plays the event loop and the window: each start page shows as
    /// `pages` says in turn, and each request is answered by `bodies` in
    /// turn. Returns how many pages were loaded and requests sent, and the
    /// overlay's stages in order.
    async fn window(
        shared: &Shared,
        session: SessionId,
        pages: &[Seen],
        bodies: &[&str],
        done: &AtomicBool,
    ) -> (usize, usize, Vec<Stage>) {
        let (mut loads, mut fetches, mut stages) = (0, 0, Vec::new());
        while !done.load(Ordering::Relaxed) {
            for command in shared.drain() {
                match command {
                    Command::Load { progress, .. } => {
                        stages.extend(progress.map(|progress| progress.stage));
                        shared.seen(session, pages[loads].clone());
                        loads += 1;
                    }
                    Command::Stage { stage, .. } => stages.push(stage),
                    Command::Fetch { ticket, .. } => {
                        let url = "https://archives.example.org/api";
                        shared.fetched(session, answer(ticket, 200, url, bodies[fetches]));
                        fetches += 1;
                    }
                    Command::Ask { .. } => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        (loads, fetches, stages)
    }

    fn transport(shared: &Arc<Shared>) -> WindowTransport {
        WindowTransport {
            shared: Arc::clone(shared),
            session: 1,
            title: "AD00 - Exampleville - (aucun) - N - 1877".to_owned(),
            archive: "Archives of Example".to_owned(),
            texts: texts(),
        }
    }

    async fn search(pages: &[Seen], bodies: &[&str]) -> (Result<String, FetchError>, usize, usize) {
        let (result, loads, fetches, _) = search_stages(pages, bodies).await;
        (result, loads, fetches)
    }

    async fn search_stages(
        pages: &[Seen],
        bodies: &[&str],
    ) -> (Result<String, FetchError>, usize, usize, Vec<Stage>) {
        let shared = Arc::new(Shared::default());
        let transport = transport(&shared);
        let endpoint = endpoint();
        let done = AtomicBool::new(false);
        let resolution = async {
            let result = match transport.connect(&endpoint).await {
                Ok(fetch) => fetch.get("/api").await,
                Err(error) => Err(error),
            };
            done.store(true, Ordering::Relaxed);
            result
        };
        let (result, (loads, fetches, stages)) =
            tokio::join!(resolution, window(&shared, 1, pages, bodies, &done));
        (result, loads, fetches, stages)
    }

    #[tokio::test]
    async fn the_overlay_follows_the_search() {
        let portal = page(PageState::Portal, false);
        let (_, _, _, stages) = search_stages(&[portal.clone(), portal], &[CHECK, PORTAL]).await;
        // The start page, the request, the start page again for the
        // reader, the request once more.
        assert_eq!(
            stages,
            [
                Stage::Connecting,
                Stage::Searching,
                Stage::Connecting,
                Stage::Searching
            ]
        );
    }

    #[test]
    fn each_stage_says_where_the_resolution_stands() {
        let texts = texts();
        assert_eq!(Stage::Connecting.text(&texts), "Connecting…");
        assert_eq!(Stage::Searching.text(&texts), "Searching…");
        assert_eq!(Stage::Opening { view: None }.text(&texts), "Opening…");
        assert_eq!(
            Stage::Opening { view: Some(12) }.text(&texts),
            "Opening view 12…"
        );
    }

    #[test]
    fn cancelling_stops_the_resolution_and_what_it_waits_on() {
        let shared = Shared::default();
        let mut cancelled = shared.cancellable(3);
        let mut pages = shared.listen(3);
        let (_, mut answer) = shared.wait_for_answer(3, origins());
        let mut other = shared.cancellable(4);

        shared.cancel(3);
        assert_eq!(cancelled.try_recv(), Ok(Some(())));
        assert!(pages.try_recv().is_err());
        assert!(answer.try_recv().is_err());
        // The window stays open, and another session's resolution runs on.
        assert!(!shared.is_closed(3));
        assert_eq!(other.try_recv(), Ok(None));

        // Once settled, a resolution is no longer cancelled.
        shared.settle(4);
        shared.cancel(4);
        assert!(other.try_recv().is_err());
    }

    #[test]
    fn closing_a_window_does_not_cancel_its_resolution() {
        let shared = Shared::default();
        let mut cancelled = shared.cancellable(5);
        shared.close(5);
        // Dropped, not sent: the resolution fails as its waits do.
        assert!(cancelled.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_request_answered_by_a_check_is_sent_once_more_after_the_reader_passed_it() {
        let portal = page(PageState::Portal, false);
        let (result, loads, fetches) = search(&[portal.clone(), portal], &[CHECK, PORTAL]).await;
        assert_eq!(result, Ok(PORTAL.to_owned()));
        // The start page again, for the reader, then the same request.
        assert_eq!((loads, fetches), (2, 2));
    }

    #[tokio::test]
    async fn a_request_is_sent_again_once_only() {
        let portal = page(PageState::Portal, false);
        let pages = [portal.clone(), portal.clone(), portal];
        let (result, loads, fetches) = search(&pages, &[CHECK, CHECK, PORTAL]).await;
        assert_eq!(result, Err(FetchError::Challenged));
        assert_eq!((loads, fetches), (2, 2));
    }

    /// Loads `path` as a page of a portal that admits page loads only, after
    /// trying a script's request there, with the window's pages and bodies.
    async fn load_page(
        pages: &[Seen],
        bodies: &[&str],
    ) -> (
        Result<String, FetchError>,
        Result<String, FetchError>,
        (usize, usize, Vec<Stage>),
    ) {
        let shared = Arc::new(Shared::default());
        let transport = transport(&shared);
        let endpoint = PortalEndpoint {
            access: Access::Page,
            ..endpoint()
        };
        let done = AtomicBool::new(false);
        let resolution = async {
            let results = match transport.connect(&endpoint).await {
                Ok(fetch) => (
                    fetch.get("/api").await,
                    fetch.page("/search?q=1", "table").await,
                ),
                Err(error) => (Err(error.clone()), Err(error)),
            };
            done.store(true, Ordering::Relaxed);
            results
        };
        let ((request, page), window) =
            tokio::join!(resolution, window(&shared, 1, pages, bodies, &done));
        (request, page, window)
    }

    #[tokio::test]
    async fn a_page_loads_in_the_window_and_its_rendered_document_answers() {
        let portal = page(PageState::Portal, false);
        let (request, rendered, (loads, fetches, stages)) =
            load_page(&[portal.clone(), portal], &[PORTAL]).await;
        // No script's request reaches such a portal.
        assert_eq!(request, Err(FetchError::NotAllowed));
        assert_eq!(rendered, Ok(PORTAL.to_owned()));
        // The start page, the page, then its document read back.
        assert_eq!((loads, fetches), (2, 1));
        assert_eq!(
            stages,
            [Stage::Connecting, Stage::Searching, Stage::Searching]
        );

        // A block in place of the page ends the search before reading it.
        let blocked = page(PageState::Blocked, false);
        let (_, rendered, (loads, fetches, _)) =
            load_page(&[page(PageState::Portal, false), blocked], &[]).await;
        assert_eq!(rendered, Err(FetchError::Challenged));
        assert_eq!((loads, fetches), (2, 0));
    }

    #[tokio::test]
    async fn a_block_ends_the_search_without_a_request() {
        let (result, loads, fetches) = search(&[page(PageState::Blocked, false)], &[]).await;
        assert_eq!(result, Err(FetchError::Challenged));
        assert_eq!((loads, fetches), (1, 0));

        // A block answering a request is not sent again.
        let (result, loads, fetches) = search(&[page(PageState::Portal, false)], &[BLOCK]).await;
        assert_eq!(result, Err(FetchError::Challenged));
        assert_eq!((loads, fetches), (1, 1));
    }
}
