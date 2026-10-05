//! Archive viewer windows.
//!
//! The interface asks for a cited register through [`ArchiveViewerBridge`];
//! the request is queued and served on the event loop, the only place a
//! window can be created. Each request opens a top-level window, since
//! archive portals refuse to be framed, with an ephemeral WebView whose
//! initialization script is produced by the driver of the archive's portal
//! platform.

mod arkotheque;

use std::sync::{Arc, Mutex};

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::tao::event::{Event, WindowEvent};
use dioxus::desktop::tao::event_loop::EventLoopWindowTarget;
use dioxus::desktop::tao::window::{Window, WindowBuilder};
use dioxus::desktop::wry::{WebView, WebViewBuilder};
use oxidgene_archives::ArchiveRegistry;
use oxidgene_ui::archive_viewer::{
    ArchiveLink, ArchiveViewerBridge, ArchiveViewerOpener, ArchiveViewerRequest,
};
use tracing::warn;

type Pending = Arc<Mutex<Vec<ArchiveViewerRequest>>>;

/// Where a window starts, and what it runs on every page load.
struct Start {
    url: String,
    script: String,
}

/// The driver settings of the first collection to try for `link` whose
/// portal a driver can search for the citation.
fn driven_portal(link: &ArchiveLink) -> Option<arkotheque::Portal> {
    let (_, collections) = ArchiveRegistry::embedded().candidates(&link.citation)?;
    collections.into_iter().find_map(|collection| {
        if collection.platform != arkotheque::PLATFORM {
            return None;
        }
        let portal = arkotheque::Portal::of(collection).ok()?;
        portal.drives(&link.citation).then_some(portal)
    })
}

fn start(request: &ArchiveViewerRequest) -> Result<Start, String> {
    let portal = driven_portal(&request.link)
        .ok_or_else(|| format!("no driver for `{}`", request.link.archive.id))?;
    Ok(Start {
        url: portal.start_url(),
        script: arkotheque::script(&portal, request)?,
    })
}

struct QueueingArchiveViewer(Pending);

impl ArchiveViewerOpener for QueueingArchiveViewer {
    fn supports(&self, link: &ArchiveLink) -> bool {
        driven_portal(link).is_some()
    }

    fn open(&self, request: ArchiveViewerRequest) {
        if let Ok(mut pending) = self.0.lock() {
            pending.push(request);
        }
    }
}

struct ArchiveWindow {
    window: Window,
    _webview: WebView,
}

pub fn install<T: 'static>() -> (
    ArchiveViewerBridge,
    impl FnMut(&Event<'_, T>, &EventLoopWindowTarget<T>) + 'static,
) {
    let pending: Pending = Arc::new(Mutex::new(Vec::new()));
    let bridge = ArchiveViewerBridge::new(Arc::new(QueueingArchiveViewer(Arc::clone(&pending))));
    let handler_pending = Arc::clone(&pending);
    let mut windows = Vec::<ArchiveWindow>::new();

    let handler = move |event: &Event<'_, T>, target: &EventLoopWindowTarget<T>| {
        let requests: Vec<_> = handler_pending
            .lock()
            .map(|mut pending| pending.drain(..).collect())
            .unwrap_or_default();

        for request in requests {
            if let Some(window) = open(target, &request) {
                windows.push(window);
            }
        }

        if let Event::WindowEvent {
            window_id,
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            windows.retain(|open| open.window.id() != *window_id);
        }
    };

    (bridge, handler)
}

fn open<T>(
    target: &EventLoopWindowTarget<T>,
    request: &ArchiveViewerRequest,
) -> Option<ArchiveWindow> {
    let start = start(request)
        .inspect_err(|_| {
            warn!(
                error = "archive_driver",
                archive = request.link.archive.id.as_str(),
                "could not prepare the archive lookup"
            );
        })
        .ok()?;

    let window = WindowBuilder::new()
        .with_title(request.link.title.clone())
        .with_inner_size(LogicalSize::new(1280.0, 900.0))
        .build(target)
        .inspect_err(|_| {
            warn!(
                error = "archive_window_creation",
                "could not create the archive window"
            );
        })
        .ok()?;

    let builder = WebViewBuilder::new()
        .with_url(&start.url)
        .with_incognito(true)
        .with_initialization_script(&start.script);

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
        _webview: webview,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A catalogue entry for a platform without a driver would offer a link
    /// that opens nothing.
    #[test]
    fn every_catalogued_platform_has_a_driver() {
        for archive in ArchiveRegistry::embedded().archives() {
            for collection in &archive.collections {
                assert!(
                    collection.platform == arkotheque::PLATFORM,
                    "{}: no driver for `{}`",
                    archive.id,
                    collection.platform
                );
            }
        }
    }

    #[test]
    fn supports_only_the_citations_a_driver_can_search() {
        let link = |title: &str| ArchiveLink::from_source_title(title).expect("a catalogued act");
        assert!(
            driven_portal(&link(
                "AD44 - Exampleville - (aucun) - B - 1791 - vue 3g/12"
            ))
            .is_some()
        );
        // Held by the collection, but without settings for the driver.
        assert!(driven_portal(&link("AD44 - Exampleville - (aucun) - M - 1850")).is_none());
        // The driver searches by year.
        assert!(driven_portal(&link("AD44 - Exampleville - (aucun) - N - acte 3")).is_none());
    }
}
