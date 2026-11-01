//! HTTP caching where a tree has several viewers (docs/authentication.md
//! §9.1, docs/privacy.md §7): no answer is kept across viewers, by the
//! browser or by an intermediary.
//!
//! - Every JSON answer of the API, REST's and GraphQL's, that sets no
//!   caching of its own is `Cache-Control: no-store`: it holds what one
//!   viewer may see of a tree, redacted for them.
//! - Media bytes keep their `Cache-Control: private` and their `ETag`, and
//!   vary on the credential ([`MEDIA_VARY`]): a browser shared by two
//!   accounts revalidates rather than serving one account's picture to the
//!   other.
//!
//! The web build only: the desktop has one viewer, and keeps the headers it
//! had — its per-launch token would otherwise make every launch miss the
//! pictures the previous one cached.

use axum::extract::Request;
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::Response;

/// What a medium's bytes vary on: the session cookie and the bearer token.
pub const MEDIA_VARY: &str = "Cookie, Authorization";

/// Mark a JSON answer of the API `no-store` unless it says otherwise.
pub async fn no_store_json(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let api = path.starts_with("/api/") || path == "/graphql";
    let mut response = next.run(request).await;
    let json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        // `application/json`, and GraphQL's `application/graphql-response+json`.
        .is_some_and(|value| value.starts_with("application/") && value.contains("json"));
    if api && json {
        response
            .headers_mut()
            .entry(header::CACHE_CONTROL)
            .or_insert(HeaderValue::from_static("no-store"));
    }
    response
}

/// Add the credential to what a medium's bytes vary on.
pub fn vary_on_credentials(headers: &mut axum::http::HeaderMap) {
    headers.append(header::VARY, HeaderValue::from_static(MEDIA_VARY));
}
