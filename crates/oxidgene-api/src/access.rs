//! Who may call the backend before authentication ships.
//!
//! There are no accounts yet (Cross-cutting Rules §7.1), so nothing here
//! identifies a user. What it does is keep the callers that are certainly not
//! the application out.
//!
//! - **The desktop's embedded server** listens on a loopback port, and loopback
//!   is not private: every local account, every process, and every page open in
//!   the user's browser can reach it. A page cannot read a cross-origin
//!   response, but it can send a form-encoded or multipart POST without a
//!   preflight, and through DNS rebinding it can make itself same-origin and read
//!   the tree outright. The shell is the only legitimate caller, so it generates
//!   a [`LocalToken`] at launch, hands it to its own client, and the server
//!   refuses every request that does not carry it.
//! - **The standalone server** is reached by a browser frontend on one trusted
//!   origin. CORS already stops other origins from *reading*; [`same_origin_writes`]
//!   stops them from *writing* through the requests CORS lets through unasked.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use uuid::Uuid;

use crate::rest::error::ErrorBody;

/// Paths every caller may read without the token: the API description holds
/// no tree data, and App Settings opens it in the system browser, which has no
/// way to present a credential.
const PUBLIC_PATHS: &[&str] = &["/api/v1/openapi.json"];

/// A per-launch secret the embedded backend requires as a bearer token.
#[derive(Clone)]
pub struct LocalToken(Arc<str>);

impl LocalToken {
    /// A fresh token with 244 bits from the operating system's CSPRNG.
    #[must_use]
    pub fn generate() -> Self {
        Self(format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()).into())
    }

    /// The secret itself, to hand to the one client that may use it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for LocalToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalToken(..)")
    }
}

/// Refuse every request to `router` that does not present `token` as
/// `Authorization: Bearer <token>`, except the [`PUBLIC_PATHS`].
pub fn require_local_token(router: Router, token: LocalToken) -> Router {
    router.layer(middleware::from_fn_with_state(token, check_local_token))
}

async fn check_local_token(
    State(token): State<LocalToken>,
    request: Request,
    next: Next,
) -> Response {
    if PUBLIC_PATHS.contains(&request.uri().path()) || presents(request.headers(), &token) {
        return next.run(request).await;
    }
    refusal(
        StatusCode::UNAUTHORIZED,
        "unauthenticated",
        "Authentication is required",
    )
}

fn presents(headers: &HeaderMap, token: &LocalToken) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "))
        .is_some_and(|given| constant_time_eq(given, token.as_str().as_bytes()))
}

/// Compare without stopping at the first difference, so response timing says
/// nothing about how much of a guess was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Refuse state-changing requests that a browser sent from any origin other
/// than `allowed`.
///
/// A browser names the page's origin on every cross-origin request and on
/// every same-origin one that can change state, so an `Origin` that is present
/// and foreign is always a page the operator did not deploy. Requests without
/// one — `curl`, scripts, the desktop client — are not a browser acting on a
/// user's behalf and pass. Reads are left to CORS, which already withholds
/// their responses.
pub fn same_origin_writes(router: Router, allowed: HeaderValue) -> Router {
    router.layer(middleware::from_fn_with_state(allowed, check_origin))
}

async fn check_origin(
    State(allowed): State<HeaderValue>,
    request: Request,
    next: Next,
) -> Response {
    let safe = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let foreign = request
        .headers()
        .get(header::ORIGIN)
        .is_some_and(|origin| origin != allowed);
    if safe || !foreign {
        return next.run(request).await;
    }
    refusal(
        StatusCode::FORBIDDEN,
        "forbidden",
        "The request origin is not allowed",
    )
}

fn refusal(status: StatusCode, code: &str, message: &str) -> Response {
    let body = ErrorBody {
        error: code.to_string(),
        message: message.to_string(),
        request_id: None,
    };
    (status, axum::Json(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_distinct() {
        let a = LocalToken::generate();
        let b = LocalToken::generate();
        assert_eq!(a.as_str().len(), 64);
        assert_ne!(a.as_str(), b.as_str());
        assert_eq!(format!("{a:?}"), "LocalToken(..)");
    }

    #[test]
    fn comparison_needs_every_byte() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"secret"));
    }
}
