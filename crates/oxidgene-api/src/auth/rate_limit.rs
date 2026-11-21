//! The rate limits per address (docs/authentication.md §9.2): what an
//! anonymous caller can make the server do — start a sign-in, complete
//! one, open a share link, ask the provider about an opaque token — is
//! counted by the address it comes from.
//!
//! A counter lives in the session store, so the replicas of a deployment
//! on Redis count together, and for its window only. It is named by a
//! digest of the address: neither the process nor Redis holds the address
//! itself, and nothing logs it.

use std::time::Duration;

use axum::response::{IntoResponse, Response};
use oxidgene_core::OxidGeneError;

use super::Oidc;
use super::client::ClientAddress;
use super::crypto::digest;
use crate::rest::error::ApiError;

/// How many requests an address may make in a window.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limit {
    /// The counter's name: one counter per limit and address.
    name: &'static str,
    max: u64,
    window: Duration,
}

const MINUTE: Duration = Duration::from_secs(60);

/// `GET /auth/login`: each one keeps a sign-in transaction.
pub(crate) const LOGIN: Limit = Limit {
    name: "login",
    max: 30,
    window: MINUTE,
};

/// `GET /auth/callback`: each one may ask the provider for tokens and
/// writes an access event when it fails.
pub(crate) const CALLBACK: Limit = Limit {
    name: "callback",
    max: 30,
    window: MINUTE,
};

/// `GET /auth/share/{token}`: each one is a guess at a share token.
pub(crate) const SHARE_LINK: Limit = Limit {
    name: "share-link",
    max: 30,
    window: MINUTE,
};

/// Opaque bearer tokens the provider is asked about: a token it answered
/// about within the minute is not asked again, so only new ones count.
pub(crate) const INTROSPECTION: Limit = Limit {
    name: "introspection",
    max: 30,
    window: MINUTE,
};

/// A request past its limit: `429 rate_limited`, with the seconds until
/// the window ends as `Retry-After`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RateLimited {
    pub retry_after: u64,
}

impl IntoResponse for RateLimited {
    fn into_response(self) -> Response {
        ApiError(OxidGeneError::RateLimited {
            retry_after: self.retry_after,
        })
        .into_response()
    }
}

impl Oidc {
    /// Count one request of `address` against `limit`.
    ///
    /// A request whose address the server was not given counts with every
    /// other such request, under one counter: an unknown caller is limited
    /// more, never less. A store that cannot be reached limits nothing:
    /// what these requests go on to do needs that store, and fails on it.
    ///
    /// # Errors
    ///
    /// [`RateLimited`] once the address made `limit`'s requests in the
    /// current window.
    pub(crate) async fn admit(
        &self,
        limit: Limit,
        address: Option<ClientAddress>,
    ) -> Result<(), RateLimited> {
        let caller = address.map_or_else(|| "unknown".to_string(), ClientAddress::bucket);
        let name = format!("{}:{}", limit.name, digest(&caller));
        match self.store.count(&name, limit.window).await {
            Ok(tally) if tally.count > limit.max => Err(RateLimited {
                retry_after: tally.resets_in.as_secs().max(1),
            }),
            _ => Ok(()),
        }
    }
}
