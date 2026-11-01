//! Whether to offer the surfaces that only matter for the other viewers of
//! a tree: the privacy settings visitors see the tree through, and the
//! exclusions from sharing (`docs/privacy.md` §9, `docs/authentication.md`
//! §7).
//!
//! The desktop has one local user who sees everything, and so does the web
//! build run without authentication (`OXIDGENE_AUTH_MODE=none`): both render
//! exactly what they rendered before these surfaces existed. The account
//! state already knows which is the case — it asked `/auth/session` once,
//! and is [`AccountState::Absent`] on the desktop and in `none` mode — so
//! the surfaces follow it rather than asking the server again.

use dioxus::prelude::*;

use crate::account::AccountState;

/// Whether this build offers the surfaces for a tree's other viewers: the
/// server signs users in, whether or not someone is signed in yet.
pub fn use_sharing_surfaces() -> bool {
    try_use_context::<Signal<AccountState>>().is_some_and(|state| offered(&state.read()))
}

/// Whether the account state `state` offers the surfaces. A resource reads
/// the state inside its future's closure and asks this, so that it runs
/// again once the server has answered, which a full page load learns after
/// its first render.
pub fn offered(state: &AccountState) -> bool {
    matches!(state, AccountState::Anonymous | AccountState::SignedIn(_))
}
