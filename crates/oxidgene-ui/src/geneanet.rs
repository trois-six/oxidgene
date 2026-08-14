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
//! # Why the requests come from the window rather than from Rust
//!
//! geneanet.org sits behind Cloudflare, which challenges clients on their
//! TLS/HTTP2 fingerprint — a challenge an HTTP client cannot pass and that
//! OxidGene deliberately does not try to defeat. Issuing the requests in a
//! real browser engine, on the user's own session, against their own data, is
//! not a way around that check: it is the thing the check is asking for.
//! See `docs/specifications/geneanet-media-import.md` §8.

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
    },
    /// The user closed the window before signing in. Not an error — the step
    /// simply returns to its initial state.
    Cancelled,
    Failed(String),
}

/// Opens the Geneanet login window and drives the collection.
///
/// Implemented by `oxidgene-desktop`. One method, because there is exactly one
/// thing the UI cannot do for itself.
pub trait GeneanetCollector: Send + Sync {
    /// Opens the window and reports progress on `events` until it sends a
    /// terminal event ([`GeneanetEvent::Collected`],
    /// [`GeneanetEvent::Cancelled`] or [`GeneanetEvent::Failed`]).
    fn start(&self, events: UnboundedSender<GeneanetEvent>);
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

    pub fn start(&self, events: UnboundedSender<GeneanetEvent>) {
        self.0.start(events);
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
