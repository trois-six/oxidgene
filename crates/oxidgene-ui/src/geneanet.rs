//! The seam between the import wizard and a Geneanet login window.
//!
//! Step 3 of the wizard needs something this crate cannot do and must not
//! know how to do: open a second browser window on geneanet.org, let the user
//! sign in, and issue the collection requests *from inside it*. That is a
//! `wry`/`tao` job, and pulling `dioxus-desktop` in here would make the whole
//! UI unbuildable for the web target.
//!
//! So the wizard declares what it needs and the desktop binary supplies it.
//! [`GeneanetBridge`] is put into the Dioxus context by
//! `oxidgene-desktop`; the web build provides none, `use_geneanet_bridge`
//! returns `None`, and the step renders the "this needs the desktop app"
//! explanation instead of a button that could not work.
//!
//! # Why *every* request comes from the window rather than from Rust
//!
//! geneanet.org sits behind Cloudflare, which challenges clients on their
//! TLS/HTTP2 fingerprint. Measured against the live site: no direct download
//! succeeds, whatever the cookie and whatever the stack. So this is not an
//! optimisation and not a preference — the window is the only place the bytes
//! can come from, for the media as much as for the metadata.
//!
//! Issuing the requests in a real browser engine, on the user's own session,
//! against their own data, is not a way around that check: it is the thing the
//! check is asking for. See `docs/specifications/geneanet-media-import.md` §8.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedSender;

/// What the login window reports back as it goes.
///
/// A channel rather than a callback: the window runs on the platform event
/// loop, not on Dioxus's, so what crosses between them has to be `Send`.
#[derive(Debug, Clone)]
pub enum GeneanetEvent {
    /// The window is open and waiting for the user to sign in.
    Opened,
    /// The session is established; collection is starting.
    SignedIn,
    /// Stage 1 — reading the photo list. `total` is 0 until known.
    Collecting {
        done: usize,
        total: usize,
    },
    /// Stage 2 — asking Geneanet each photo's exact byte length, which is what
    /// the archives are matched against.
    Sizing {
        done: usize,
        total: usize,
    },
    /// Everything gathered. `collection` is the JSON the server turns into a
    /// manifest; `deposit_sizes` is stage 2's output; `cookie` is the session
    /// the download step will need if the archives do not cover every photo.
    Collected {
        collection: String,
        deposit_sizes: HashMap<i64, u64>,
        cookie: Option<String>,
        account: Option<String>,
        /// How many media the account actually holds — pages included.
        ///
        /// Reported rather than inferred: `deposit_sizes` only covers the
        /// single-page deposits that could be measured, so using its length
        /// would undercount an account with documents in it.
        photo_count: usize,
    },
    /// One medium came back from the window, written to disk.
    ///
    /// A **path**, not the bytes. The gather only runs on the desktop, where
    /// the server is in-process and shares the filesystem, so carrying several
    /// hundred pictures through the UI and back out in a request body would
    /// buffer them three times over and inflate them by a third on the way —
    /// the same reasoning that makes step 2 hand over archive paths.
    ///
    /// `error` is set instead when that one could not be fetched; the run
    /// carries on and the import reports it as skipped.
    Fetched {
        url: String,
        path: Option<String>,
        error: Option<String>,
    },
    /// Progress through a fetch batch.
    Fetching {
        done: usize,
        total: usize,
    },
    /// Every URL of the batch has been attempted.
    FetchDone,
    /// The user closed the window before signing in. Not an error — the step
    /// simply returns to its initial state.
    Cancelled,
    Failed(String),
}

/// What the login window puts on screen, already translated.
///
/// The window is opened by the desktop binary, which has no access to the
/// UI's translation tables — so the wizard hands it the finished strings
/// rather than keys. Nothing here is a number: the modal owns the progress
/// bars, and two counters for one operation could only disagree.
#[derive(Debug, Clone)]
pub struct WindowStrings {
    /// The window's own title bar.
    pub title: String,
    /// Heading of the status panel, once signing in is done.
    pub heading: String,
    /// Shown while the person↔photo list is being read.
    pub reading_list: String,
    /// Shown while photos are being matched against the local archives.
    pub matching: String,
    /// The line telling the user that closing the window cancels.
    pub cancel_hint: String,
    /// Shown once collection is done and the window is only being kept for the
    /// import — otherwise the panel would go on claiming to be working.
    pub idle: String,
}

/// Opens the Geneanet login window and drives the collection.
///
/// Implemented by `oxidgene-desktop`. One method, because there is exactly one
/// thing the UI cannot do for itself.
pub trait GeneanetCollector: Send + Sync {
    /// Opens the window and reports progress on `events` until it sends a
    /// terminal event ([`GeneanetEvent::Collected`],
    /// [`GeneanetEvent::Cancelled`] or [`GeneanetEvent::Failed`]).
    fn start(&self, events: UnboundedSender<GeneanetEvent>, strings: WindowStrings);

    /// Fetches media bytes through the window that is already signed in.
    ///
    /// Every direct download is challenged by Cloudflare whatever the cookie,
    /// so this is not an optimisation — it is the only way the bytes arrive.
    /// Reports one [`GeneanetEvent::Fetched`] per URL and a final
    /// [`GeneanetEvent::FetchDone`].
    ///
    /// The window must still be open; the wizard keeps it so until the import
    /// finishes rather than closing it after collection.
    fn fetch(&self, urls: Vec<String>, events: UnboundedSender<GeneanetEvent>);

    /// Closes the window.
    ///
    /// It is deliberately *not* closed when collection ends — the import
    /// fetches through the same session — so the wizard says when it is done
    /// with it: the import finished, or the modal was dismissed.
    fn close(&self);
}

/// Context handle the wizard looks for.
///
/// Cloneable and cheap: it is read on every render of the Geneanet tab, which
/// only wants to know whether the capability exists at all.
#[derive(Clone)]
pub struct GeneanetBridge(Arc<dyn GeneanetCollector>);

impl GeneanetBridge {
    pub fn new(collector: Arc<dyn GeneanetCollector>) -> Self {
        Self(collector)
    }

    pub fn start(&self, events: UnboundedSender<GeneanetEvent>, strings: WindowStrings) {
        self.0.start(events, strings);
    }

    pub fn fetch(&self, urls: Vec<String>, events: UnboundedSender<GeneanetEvent>) {
        self.0.fetch(urls, events);
    }

    pub fn close(&self) {
        self.0.close();
    }
}

impl std::fmt::Debug for GeneanetBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GeneanetBridge")
    }
}

/// The bridge, if this build has one.
///
/// `None` on the web target, and on any desktop build that did not install
/// one. The wizard treats both the same way — it says the step needs the
/// desktop app rather than offering a button that cannot work.
pub fn use_geneanet_bridge() -> Option<GeneanetBridge> {
    try_use_context::<GeneanetBridge>()
}
