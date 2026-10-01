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
//! - **Both** answer only under a host name they are known by
//!   ([`allowed_hosts`]). A page that rebinds its own DNS name to the
//!   server's address becomes same-origin with it, past CORS and the origin
//!   check; the `Host` its browser sends still names the page's domain.

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

/// The host names a server answers under.
///
/// Names are compared without their port and without regard to case. The
/// loopback names are always included: the server binds to loopback by
/// default, and a probe or a developer reaches it as `127.0.0.1`,
/// `localhost` or `[::1]`.
#[derive(Clone, Debug)]
pub struct AllowedHosts(Arc<[String]>);

impl AllowedHosts {
    /// The loopback names alone.
    #[must_use]
    pub fn loopback() -> Self {
        Self::new(std::iter::empty::<&str>())
    }

    /// The loopback names and `hosts`, each a host name or a URL whose host
    /// is taken (`https://genealogy.example.invalid` allows
    /// `genealogy.example.invalid`). Blank entries are ignored.
    #[must_use]
    pub fn new(hosts: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let mut names: Vec<String> = ["localhost", "127.0.0.1", "::1"]
            .into_iter()
            .map(str::to_string)
            .collect();
        for host in hosts {
            let host = host.as_ref().trim();
            let authority = host.split_once("://").map_or(host, |(_, rest)| rest);
            let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
            let name = host_name(authority).to_ascii_lowercase();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        Self(names.into())
    }

    fn allows(&self, authority: &str) -> bool {
        let name = host_name(authority);
        self.0
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name))
    }
}

/// The host name of an authority (`host`, `host:port`, `[v6]:port`), without
/// user information or port.
fn host_name(authority: &str) -> &str {
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    match host.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    }
}

/// Refuse every request to `router` whose `Host` (or HTTP/2 `:authority`)
/// is not one of `hosts`, with `403 forbidden`.
///
/// This is what stops DNS rebinding: the rebinding page is same-origin with
/// the server, so neither CORS nor [`same_origin_writes`] objects, but its
/// browser still names the page's own domain as the host.
pub fn allowed_hosts(router: Router, hosts: AllowedHosts) -> Router {
    router.layer(middleware::from_fn_with_state(hosts, check_host))
}

async fn check_host(State(hosts): State<AllowedHosts>, request: Request, next: Next) -> Response {
    let authority = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .or_else(|| {
            request
                .uri()
                .authority()
                .map(|authority| authority.as_str())
        });
    if authority.is_some_and(|authority| hosts.allows(authority)) {
        return next.run(request).await;
    }
    refusal(
        StatusCode::FORBIDDEN,
        "forbidden",
        "The request host is not allowed",
    )
}

/// Give `response` the security headers every API response carries, unless
/// its handler set them itself.
///
/// - `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY` and
///   `Referrer-Policy: no-referrer` on every response: nothing the API serves
///   is meant to be sniffed, framed or followed with a referrer.
/// - `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'` on
///   JSON, which is data and never a page. Other responses keep the policy
///   their handler chose: stored files are sandboxed except PDFs, which a
///   browser's viewer must be free to render, and GraphiQL is a page.
pub(crate) async fn security_headers(mut response: Response) -> Response {
    let json = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let headers = response.headers_mut();
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert(HeaderValue::from_static("DENY"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("no-referrer"));
    if json {
        headers
            .entry(header::CONTENT_SECURITY_POLICY)
            .or_insert(HeaderValue::from_static(
                "default-src 'none'; frame-ancestors 'none'",
            ));
    }
    response
}

/// `status` with the standard error envelope of `code` and `message`.
pub(crate) fn refusal(status: StatusCode, code: &str, message: &str) -> Response {
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
    fn hosts_match_by_name_whatever_the_port_and_case() {
        let hosts = AllowedHosts::new(["https://Genealogy.example.invalid/app", "api.internal"]);
        for allowed in [
            "localhost",
            "localhost:8080",
            "127.0.0.1:18080",
            "[::1]:8080",
            "genealogy.example.invalid",
            "GENEALOGY.example.invalid:443",
            "api.internal:8080",
        ] {
            assert!(hosts.allows(allowed), "{allowed}");
        }
        for refused in [
            "attacker.example",
            "attacker.example:8080",
            "genealogy.example.invalid.attacker.example",
            "127.0.0.2",
            "",
        ] {
            assert!(!hosts.allows(refused), "{refused}");
        }
    }

    #[test]
    fn the_loopback_set_names_only_loopback() {
        let hosts = AllowedHosts::loopback();
        assert!(hosts.allows("127.0.0.1:8080"));
        assert!(hosts.allows("localhost"));
        assert!(!hosts.allows("oxidgene.example.invalid"));
    }

    #[test]
    fn comparison_needs_every_byte() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"secret"));
    }
}
