//! The seam between the web entry point and the surfaces that only matter
//! for the other viewers of a tree: the privacy settings visitors see the
//! tree through, and the exclusions from sharing (`docs/privacy.md` §9,
//! `docs/authentication.md` §7).
//!
//! The desktop has one local user who sees everything, and so does the web
//! build run without authentication (`OXIDGENE_AUTH_MODE=none`): both render
//! exactly what they rendered before these surfaces existed. So, like the
//! desktop-only capabilities ([`crate::assistant`], [`crate::geneanet`]) in
//! the other direction, the capability is a value the web entry point puts
//! into the Dioxus context once the server says it authenticates its
//! viewers (`mode: "oidc"` on `/auth/session`); pages read it back with
//! [`use_sharing_surfaces`]. The desktop provides none.

use dioxus::prelude::*;

/// Web-only capability: the tree has viewers other than its owner, so the
/// surfaces for them are offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharingSurfaces;

/// What the web entry point provides: `None` until, and unless, the server
/// reports that it authenticates its viewers.
pub type SharingSurfacesSlot = Signal<Option<SharingSurfaces>>;

/// Whether this build offers the surfaces for a tree's other viewers.
pub fn use_sharing_surfaces() -> bool {
    try_use_context::<SharingSurfacesSlot>().is_some_and(|slot| slot().is_some())
}
