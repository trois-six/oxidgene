//! Archive windows: opening a cited register on its archive's portal.
//!
//! The interface asks through [`ArchiveViewerBridge`]. Each request starts a
//! resolution on the Dioxus runtime with the shared [`Resolver`], whose
//! session cache spares a second request for a citation already opened. The
//! resolution uses the `window` transport ([`transport`]): the archive window
//! first loads the collection's search page, the portal's own page, which
//! passes any anti-bot challenge, and the adapter's requests then run in it.
//! The window finally loads the target — the cited view, or the portal's
//! filtered results — with a banner saying what OxidGene found, or, when
//! the resolution failed, the archive's website.
//!
//! Windows are top-level, since portals refuse to be framed, and share one
//! persistent web profile of their own, so that a portal's cookies spare the
//! reader its reuse licence and its challenge at every opening.

mod script;
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
    ArchiveLink, ArchiveViewerBridge, ArchiveViewerOpener, ArchiveViewerRequest, Landing,
};
use serde::Deserialize;
use tracing::{debug, warn};

use transport::{Banner, Command, Fetched, SessionId, Shared, WindowTransport};

/// One message from the scripts of [`script`].
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Message {
    Ready,
    Fetched(Fetched),
}

/// Messages from the windows, waiting for the event loop.
type Inbox = Arc<Mutex<Vec<(SessionId, Message)>>>;

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
    let banner = |text: &str| Banner {
        text: text.to_owned(),
        close: messages.close.clone(),
    };
    let transport = WindowTransport {
        shared: Arc::clone(&shared),
        session,
        title: link.title.clone(),
        searching: banner(&messages.searching),
    };
    let outcome = resolver.resolve(&link.citation, &transport).await;
    if let Err(error) = &outcome {
        warn!(
            error = error.code(),
            archive = link.archive.id.as_str(),
            "could not resolve the cited register"
        );
    }
    let Landing { url, banner: key } =
        Landing::of(link.archive, outcome.map_err(|error| error.code()));
    if shared.is_closed(session) {
        return;
    }
    shared.push(Command::Load {
        session,
        title: link.title,
        origins: origin_of(&url).map(str::to_owned).into_iter().collect(),
        url,
        banner: key.and_then(|key| messages.banner(key)).map(banner),
    });
}

/// An open archive window.
struct ArchiveWindow {
    window: Window,
    webview: WebView,
    /// The origins its IPC messages are accepted from, shared with the
    /// WebView's IPC handler.
    origins: Arc<Mutex<Vec<String>>>,
    /// The banner to show once the page being loaded is ready.
    banner: Option<Banner>,
}

impl ArchiveWindow {
    fn load(&mut self, url: &str, origins: Vec<String>, banner: Option<Banner>) {
        if let Ok(mut allowed) = self.origins.lock() {
            *allowed = origins;
        }
        self.banner = banner;
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

    /// Shows the pending banner, once.
    fn show_banner(&mut self) {
        if let Some(banner) = self.banner.take() {
            self.eval(&script::banner(&banner.text, &banner.close));
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
        for command in shared.drain() {
            match command {
                Command::Load {
                    session,
                    title,
                    url,
                    origins,
                    banner,
                } => {
                    if shared.is_closed(session) {
                        continue;
                    }
                    if let Some(window) = windows.get_mut(&session) {
                        window.load(&url, origins, banner);
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
                    };
                    match open(target, context, opening, Arc::clone(&inbox)) {
                        Some(window) => {
                            windows.insert(session, window);
                        }
                        None => shared.close(session),
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

        let messages: Vec<_> = inbox
            .lock()
            .map(|mut inbox| inbox.drain(..).collect())
            .unwrap_or_default();
        for (session, message) in messages {
            match message {
                Message::Ready => {
                    shared.ready(session);
                    if let Some(window) = windows.get_mut(&session) {
                        window.show_banner();
                    }
                }
                Message::Fetched(fetched) => shared.fetched(session, fetched),
            }
        }
    };

    (bridge, handler)
}

/// What a new window opens on.
struct Opening<'a> {
    session: SessionId,
    title: &'a str,
    url: &'a str,
    origins: Vec<String>,
    banner: Option<Banner>,
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
                inbox.push((session, message));
            }
        }
        Err(reason) => debug!(reason, "ignoring an IPC message"),
    }
}

fn read_message(
    accepted: &Mutex<Vec<String>>,
    uri: &dioxus::desktop::wry::http::Uri,
    body: &str,
) -> Result<Message, &'static str> {
    let origin = page_origin(uri).ok_or("no page origin")?;
    if !accepted
        .lock()
        .is_ok_and(|accepted| accepted.contains(&origin))
    {
        return Err("outside the archive's origin");
    }
    serde_json::from_str(body).map_err(|_| "not an archive window message")
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
    let builder = WebViewBuilder::new_with_web_context(context)
        .with_url(opening.url)
        .with_initialization_script_for_main_only(script::READY, true)
        .with_ipc_handler(move |request| {
            receive(session, &accepted, &inbox, request.uri(), request.body());
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

    Some(ArchiveWindow {
        window,
        webview,
        origins,
        banner: opening.banner,
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

    #[test]
    fn the_scripts_messages_are_read() {
        assert!(matches!(
            serde_json::from_str::<Message>(r#"{"kind": "ready"}"#),
            Ok(Message::Ready)
        ));
        assert!(matches!(
            serde_json::from_str::<Message>(
                r#"{"kind": "fetched", "ticket": 2, "error": "network"}"#
            ),
            Ok(Message::Fetched(_))
        ));
        assert!(serde_json::from_str::<Message>(r#"{"kind": "portal-event"}"#).is_err());
    }

    #[test]
    fn accepts_messages_from_the_archive_origin_only() {
        let accepted = Mutex::new(vec!["https://archives.example.org".to_owned()]);
        let ready = r#"{"kind": "ready"}"#;
        let uri = |text: &str| text.parse::<dioxus::desktop::wry::http::Uri>().unwrap();
        assert!(matches!(
            read_message(
                &accepted,
                &uri("https://archives.example.org/search"),
                ready
            ),
            Ok(Message::Ready)
        ));
        for page in [
            "https://elsewhere.example.org/search",
            "http://archives.example.org/search",
        ] {
            assert!(
                read_message(&accepted, &uri(page), ready).is_err(),
                "{page}"
            );
        }
        assert!(read_message(&accepted, &uri("https://archives.example.org/"), "{}").is_err());
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
