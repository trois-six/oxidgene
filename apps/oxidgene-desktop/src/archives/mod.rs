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
//! failure.
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
use std::sync::{Arc, Mutex};

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::event_loop::EventLoopWindowTarget;
use dioxus::desktop::tao::window::{Window, WindowBuilder};
use dioxus::desktop::wry::{WebContext, WebView, WebViewBuilder};
use oxidgene_archives::transport::origin_of;
use oxidgene_archives::{ArchiveRegistry, Resolver};
use oxidgene_ui::archive_viewer::{
    ArchiveLink, ArchivePageRequest, ArchiveViewerBridge, ArchiveViewerMessages,
    ArchiveViewerOpener, ArchiveViewerRequest, Landing,
};
use serde::Deserialize;
use tracing::{debug, warn};

use transport::{
    Command, Fetched, Page, PageState, Seen, SessionId, Shared, Texts, WindowTransport,
};

/// One message from the scripts of [`script`].
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    Page(Page),
    Fetched(Fetched),
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
type Inbox = Arc<Mutex<Vec<(SessionId, Inbound)>>>;

struct WindowOpener {
    shared: Arc<Shared>,
    resolver: Arc<Resolver<'static>>,
}

impl ArchiveViewerOpener for WindowOpener {
    fn supports(&self, link: &ArchiveLink) -> bool {
        ArchiveRegistry::embedded()
            .candidates(&link.citation)
            .is_some()
    }

    fn open(&self, request: ArchiveViewerRequest) {
        let shared = Arc::clone(&self.shared);
        let resolver = Arc::clone(&self.resolver);
        // The task outlives the page that asked: the reader may navigate on
        // while the window opens.
        dioxus::core::spawn_forever(open_register(shared, resolver, request));
    }

    fn open_page(&self, request: ArchivePageRequest) {
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
    }
}

/// Resolves the citation through the session's window, then loads the
/// target in it.
async fn open_register(
    shared: Arc<Shared>,
    resolver: Arc<Resolver<'static>>,
    request: ArchiveViewerRequest,
) {
    let ArchiveViewerRequest { link, messages } = request;
    let session = shared.next_id();
    let transport = WindowTransport {
        shared: Arc::clone(&shared),
        session,
        title: link.title.clone(),
        searching: messages.searching.clone(),
        texts: texts(&messages),
    };
    let outcome = resolver.resolve(&link.citation, &transport).await;
    if let Err(error) = &outcome {
        warn!(
            error = error.code(),
            archive = link.archive.id.as_str(),
            "could not resolve the cited register"
        );
    }
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
    });
}

/// An open archive window.
struct ArchiveWindow {
    window: Window,
    webview: WebView,
    /// The origins its IPC messages are accepted from, shared with the
    /// WebView's IPC handler.
    origins: Arc<Mutex<Vec<String>>>,
    /// The banner to show once the page being loaded is the portal's.
    banner: Option<String>,
    /// Whether the reader was asked to answer an anti-bot check.
    asking: bool,
    texts: Texts,
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
    fn load(&mut self, url: &str, origins: Vec<String>, banner: Option<String>, texts: Texts) {
        if let Ok(mut allowed) = self.origins.lock() {
            *allowed = origins;
        }
        self.banner = banner;
        self.asking = false;
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
        if self.webview.load_url(url).is_err() {
            warn!(
                error = "archive_navigation",
                "could not load a page in the archive window"
            );
        }
    }

    fn eval(&self, script: &str) -> bool {
        self.webview.evaluate_script(script).is_ok()
    }

    fn show(&self, text: &str) {
        self.eval(&script::banner(text, &self.texts.close, None));
    }

    /// A page loaded. The portal's page, or a block, shows the pending
    /// banner, once; a check shows the request to answer it once the
    /// reader was asked, and the pending banner until then.
    fn on_page(&mut self, state: PageState) {
        match state {
            PageState::Portal | PageState::Blocked => {
                self.asking = false;
                if let Some(banner) = self.banner.take() {
                    self.show(&banner);
                }
            }
            PageState::Challenge if self.asking => self.show(&self.texts.challenge),
            PageState::Challenge => {
                if let Some(banner) = &self.banner {
                    self.show(banner);
                }
            }
        }
    }

    /// Asks the reader to answer the check on screen.
    fn ask(&mut self) {
        self.asking = true;
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
            Some(&self.texts.open_in_browser),
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
    let bridge = ArchiveViewerBridge::new(Arc::new(WindowOpener {
        shared: Arc::clone(&shared),
        resolver: Arc::new(Resolver::new(ArchiveRegistry::embedded())),
    }));
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    let mut windows = HashMap::<SessionId, ArchiveWindow>::new();
    let mut context: Option<WebContext> = None;

    let handler = move |event: &Event<'_, T>, target: &EventLoopWindowTarget<T>| {
        // What the pages posted first: a message posted before a load this
        // round carries out belongs to the page being left.
        let messages: Vec<_> = inbox
            .lock()
            .map(|mut inbox| inbox.drain(..).collect())
            .unwrap_or_default();
        for (session, inbound) in messages {
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
                } => {
                    if shared.is_closed(session) {
                        continue;
                    }
                    if let Some(window) = windows.get_mut(&session) {
                        window.load(&url, origins, banner, texts);
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
                    };
                    match open(target, context, opening, Arc::clone(&inbox)) {
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
        Ok(message) => {
            if let Ok(mut inbox) = inbox.lock() {
                inbox.push((session, Inbound::Posted(message)));
            }
        }
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
    let posted = Arc::clone(&inbox);
    let builder = WebViewBuilder::new_with_web_context(context)
        .with_url(opening.url)
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

    Some(ArchiveWindow {
        window,
        webview,
        origins,
        banner: opening.banner,
        asking: false,
        texts: opening.texts,
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
