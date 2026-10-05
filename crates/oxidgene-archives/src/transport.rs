//! How an adapter's requests reach a portal.
//!
//! An adapter never builds a client: it sends [`PortalRequest`]s through a
//! [`PortalFetch`] bound to the collection's portal, which a
//! [`PortalTransport`] opens. A request goes to the portal's origin, or to
//! another origin the adapter's settings declare (an API host), never
//! elsewhere, and carries only a few allow-listed headers. The `native`
//! feature provides a transport over `reqwest`; the desktop provides another
//! that runs the requests inside its archive window, the only one that passes
//! a portal's anti-bot challenge.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

pub use crate::platform::markup::{ANTI_BOT_JSON, Guard, Signature, anti_bot, shows_check_widget};
use crate::platform::{BoxFuture, PortalEndpoint};

/// How OxidGene names itself to a portal.
pub const USER_AGENT: &str = concat!(
    "OxidGene/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/trois-six/oxidgene)"
);

/// The bound on one request, connection included. Requests are never retried.
///
/// A reader waits for one lookup they asked for: some engines take 15 to 20
/// seconds to search their most populated localities (100 rows of the Sarthe
/// portal's `Le Mans`, uncached), and a shorter bound fails exactly the
/// lookups that need the portal most.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// The largest response body read; a search answer is a few hundred
/// kilobytes at most.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// The headers an adapter may set, compared without case. Cookies belong to
/// the transport, which keeps them for the connection.
pub const ALLOWED_HEADERS: [&str; 3] = ["Accept", "Content-Type", "ApiKey"];

/// The method of a portal request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

/// One request an adapter sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalRequest {
    pub method: Method,
    /// A path and query on the portal's origin (`/search?q=…`), or an
    /// absolute address on an origin the endpoint declares.
    pub url: String,
    /// Allow-listed headers ([`ALLOWED_HEADERS`]).
    pub headers: Vec<(String, String)>,
    /// The body of a `POST`: form-encoded or JSON, as `Content-Type` says.
    pub body: Option<String>,
}

impl PortalRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: Method::Get,
            url: url.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    pub fn post(url: impl Into<String>, content_type: &str, body: impl Into<String>) -> Self {
        Self {
            method: Method::Post,
            url: url.into(),
            headers: vec![("Content-Type".to_owned(), content_type.to_owned())],
            body: Some(body.into()),
        }
    }

    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.to_owned(), value.into()));
        self
    }
}

/// Requests on one portal, within one resolution: cookies a response sets are
/// sent back by the following requests of the same fetcher.
pub trait PortalFetch: Send + Sync {
    /// Sends one request and returns the response body.
    fn request<'a>(
        &'a self,
        request: &'a PortalRequest,
    ) -> BoxFuture<'a, Result<String, FetchError>>;

    /// A `GET` of a path and query on the portal's origin.
    fn get<'a>(&'a self, path_and_query: &'a str) -> BoxFuture<'a, Result<String, FetchError>> {
        Box::pin(async move { self.request(&PortalRequest::get(path_and_query)).await })
    }
}

/// Opens fetchers on portals.
pub trait PortalTransport: Send + Sync {
    /// Whether requests run in a browser page, which passes the anti-bot
    /// challenge of a portal whose access is `browser`.
    fn is_browser(&self) -> bool;

    /// A fetcher bound to the endpoint's origins. A browser transport first
    /// loads the endpoint's start page.
    fn connect<'a>(
        &'a self,
        endpoint: &'a PortalEndpoint,
    ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>>;
}

/// Why a request to a portal failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// No answer within [`TIMEOUT`].
    Timeout,
    /// The connection failed, or the window carrying it closed.
    Network,
    /// The portal answered with an error status.
    Status(u16),
    /// An anti-bot challenge answered with an error status in place of the
    /// portal. A challenge answered as a success is the adapter's to tell,
    /// once it cannot read the answer (`markup::unreadable`).
    Challenged,
    /// The body exceeds [`MAX_BODY_BYTES`].
    TooLarge,
    /// The request, or a redirect, left the endpoint's origins.
    NotSameOrigin,
    /// The request carries a header outside [`ALLOWED_HEADERS`], or a body
    /// on a `GET`.
    NotAllowed,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("the portal did not answer in time"),
            Self::Network => f.write_str("the portal could not be reached"),
            Self::Status(status) => write!(f, "the portal answered with status {status}"),
            Self::Challenged => f.write_str("an anti-bot challenge answered for the portal"),
            Self::TooLarge => f.write_str("the portal's answer is too large"),
            Self::NotSameOrigin => f.write_str("the request left the portal's origins"),
            Self::NotAllowed => f.write_str("the request carries what a portal request may not"),
        }
    }
}

impl std::error::Error for FetchError {}

/// The absolute address of `path_and_query` on `origin`, refusing anything
/// that is not a path on that origin.
pub fn portal_url(origin: &str, path_and_query: &str) -> Result<String, FetchError> {
    let on_origin = path_and_query.starts_with('/')
        && !path_and_query.starts_with("//")
        && is_clean(path_and_query);
    if on_origin {
        Ok(format!("{origin}{path_and_query}"))
    } else {
        Err(FetchError::NotSameOrigin)
    }
}

/// No fragment, backslash, whitespace or control character.
fn is_clean(text: &str) -> bool {
    !text.contains(['\\', '#']) && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The absolute address a request goes to on `endpoint`, once checked: a path
/// on the portal's origin or an address on a declared origin, allow-listed
/// headers without line breaks, and a body only on a `POST`.
pub fn request_url(
    endpoint: &PortalEndpoint,
    request: &PortalRequest,
) -> Result<String, FetchError> {
    let headers_allowed = request.headers.iter().all(|(name, value)| {
        ALLOWED_HEADERS
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(name))
            && !value.contains(['\r', '\n'])
    });
    if !headers_allowed || (request.method == Method::Get && request.body.is_some()) {
        return Err(FetchError::NotAllowed);
    }
    if request.url.starts_with('/') {
        return portal_url(&endpoint.origin, &request.url);
    }
    let declared = endpoint.origins().any(|origin| {
        request.url.strip_prefix(origin).is_some_and(|rest| {
            rest.is_empty() || (rest.starts_with('/') && !rest.starts_with("//"))
        })
    });
    if declared && is_clean(&request.url) {
        Ok(request.url.clone())
    } else {
        Err(FetchError::NotSameOrigin)
    }
}

/// The error of an answer with an error status: a challenge when its body is
/// an anti-bot page (often a `403` or a `429`), the status otherwise.
pub(crate) fn refusal(status: u16, body: &str) -> FetchError {
    if crate::platform::markup::is_challenge(body) {
        FetchError::Challenged
    } else {
        FetchError::Status(status)
    }
}

/// The answer of a request a browser page ran with its own `fetch`, as a
/// browser transport receives it: the status, final address and body, or
/// the `error` (`timeout`, anything else a network failure) that stopped it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PageAnswer {
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl PageAnswer {
    /// The body, once the answer is checked: a success status, a final
    /// address still on one of `origins` after redirects, a bounded size.
    /// An error status whose body is an anti-bot page is a challenge.
    pub fn result(self, origins: &[String]) -> Result<String, FetchError> {
        match self.error.as_deref() {
            Some("timeout") => return Err(FetchError::Timeout),
            Some(_) => return Err(FetchError::Network),
            None => {}
        }
        let status = self.status.ok_or(FetchError::Network)?;
        let on_origin = self
            .url
            .as_deref()
            .and_then(origin_of)
            .is_some_and(|origin| origins.iter().any(|allowed| allowed == origin));
        if !on_origin {
            return Err(FetchError::NotSameOrigin);
        }
        let body = self.body.unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(refusal(status, &body));
        }
        if body.len() > MAX_BODY_BYTES {
            return Err(FetchError::TooLarge);
        }
        Ok(body)
    }
}

/// The `scheme://host[:port]` of an absolute address.
pub fn origin_of(url: &str) -> Option<&str> {
    let after_scheme = url.find("://")? + 3;
    let end = url[after_scheme..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |at| after_scheme + at);
    (end > after_scheme).then(|| &url[..end])
}

#[cfg(feature = "native")]
pub use native::NativeTransport;

#[cfg(feature = "native")]
mod native {
    use std::sync::Mutex;

    use super::{
        BoxFuture, FetchError, MAX_BODY_BYTES, Method, PortalEndpoint, PortalFetch, PortalRequest,
        PortalTransport, TIMEOUT, USER_AGENT, origin_of, refusal, request_url,
    };

    /// The most redirects followed, all within the endpoint's origins.
    const MAX_REDIRECTS: usize = 5;

    /// Requests from this process over `reqwest`, for portals whose access is
    /// `any`.
    ///
    /// Redirects are followed here rather than by `reqwest`, so that each
    /// hop's address is checked against the endpoint's origins and the
    /// cookies a redirect sets reach the fetcher's jar.
    pub struct NativeTransport {
        client: reqwest::Client,
    }

    impl NativeTransport {
        pub fn new() -> Result<Self, FetchError> {
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(TIMEOUT)
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                // A fresh connection per request: some portals answer
                // `HTTP/1.0` with `Connection: Keep-Alive`, then drop the
                // reused connection mid-answer (Pas-de-Calais). A resolution
                // sends a handful of requests, so pooling saves little.
                .pool_max_idle_per_host(0)
                .build()
                .map_err(|_| FetchError::Network)?;
            Ok(Self { client })
        }
    }

    impl PortalTransport for NativeTransport {
        fn is_browser(&self) -> bool {
            false
        }

        fn connect<'a>(
            &'a self,
            endpoint: &'a PortalEndpoint,
        ) -> BoxFuture<'a, Result<Box<dyn PortalFetch + 'a>, FetchError>> {
            let fetch = NativeFetch {
                client: self.client.clone(),
                endpoint: endpoint.clone(),
                jar: CookieJar::default(),
            };
            Box::pin(async move { Ok(Box::new(fetch) as Box<dyn PortalFetch>) })
        }
    }

    struct NativeFetch {
        client: reqwest::Client,
        endpoint: PortalEndpoint,
        jar: CookieJar,
    }

    impl NativeFetch {
        async fn send(&self, request: &PortalRequest) -> Result<String, FetchError> {
            let mut hop = request.clone();
            hop.url = request_url(&self.endpoint, request)?;
            for _ in 0..=MAX_REDIRECTS {
                let response = self.send_one(&hop).await?;
                let status = response.status();
                if !status.is_redirection() {
                    if !status.is_success() {
                        // Read, bounded, only to tell an anti-bot page apart.
                        let body = read_body(response).await.unwrap_or_default();
                        return Err(refusal(status.as_u16(), &body));
                    }
                    return read_body(response).await;
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or(FetchError::Status(status.as_u16()))?;
                let next = match origin_of(&hop.url) {
                    Some(origin) if location.starts_with('/') => format!("{origin}{location}"),
                    _ => location.to_owned(),
                };
                hop.url = request_url(&self.endpoint, &PortalRequest::get(next))?;
                // A 303, and in practice a 301 or 302 after a form, turn the
                // request into a `GET`; 307 and 308 keep it.
                if !matches!(status.as_u16(), 307 | 308) {
                    hop.method = Method::Get;
                    hop.body = None;
                }
            }
            Err(FetchError::NotSameOrigin)
        }

        /// One hop, with the jar's cookies for its origin, keeping the
        /// cookies its response sets.
        async fn send_one(&self, hop: &PortalRequest) -> Result<reqwest::Response, FetchError> {
            let origin = origin_of(&hop.url).ok_or(FetchError::NotSameOrigin)?;
            let method = match hop.method {
                Method::Get => reqwest::Method::GET,
                Method::Post => reqwest::Method::POST,
            };
            let mut builder = self.client.request(method, &hop.url);
            for (name, value) in &hop.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            if let Some(cookies) = self.jar.header(origin) {
                builder = builder.header(reqwest::header::COOKIE, cookies);
            }
            if let Some(body) = &hop.body {
                builder = builder.body(body.clone());
            }
            let response = builder.send().await.map_err(classify)?;
            for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
                if let Ok(value) = value.to_str() {
                    self.jar.store(origin, value);
                }
            }
            Ok(response)
        }
    }

    impl PortalFetch for NativeFetch {
        fn request<'a>(
            &'a self,
            request: &'a PortalRequest,
        ) -> BoxFuture<'a, Result<String, FetchError>> {
            Box::pin(self.send(request))
        }
    }

    async fn read_body(mut response: reqwest::Response) -> Result<String, FetchError> {
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(classify)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    fn classify(error: reqwest::Error) -> FetchError {
        if error.is_timeout() {
            FetchError::Timeout
        } else {
            FetchError::Network
        }
    }

    /// The cookies of one fetcher, by origin, for as long as the resolution
    /// it serves.
    ///
    /// Kept by hand rather than with `reqwest`'s cookie store: a fetcher
    /// lives for one resolution on the few origins of one portal, so the
    /// attributes that scope a cookie in a browser (domain, path, expiry)
    /// have nothing to separate, and the store would add two crates to the
    /// server for what a name-value list does. A cookie set empty, or with
    /// `Max-Age=0`, is removed.
    #[derive(Default)]
    pub(super) struct CookieJar(Mutex<Vec<(String, String, String)>>);

    impl CookieJar {
        pub(super) fn store(&self, origin: &str, set_cookie: &str) {
            let mut attributes = set_cookie.split(';');
            let Some((name, value)) = attributes.next().and_then(|pair| pair.split_once('='))
            else {
                return;
            };
            let (name, value) = (name.trim(), value.trim());
            if name.is_empty() {
                return;
            }
            let expired = value.is_empty()
                || attributes.any(|attribute| {
                    attribute.trim().split_once('=').is_some_and(|(key, age)| {
                        key.eq_ignore_ascii_case("max-age") && age.trim().starts_with(['0', '-'])
                    })
                });
            let Ok(mut cookies) = self.0.lock() else {
                return;
            };
            cookies.retain(|(at, known, _)| !(at == origin && known == name));
            if !expired {
                cookies.push((origin.to_owned(), name.to_owned(), value.to_owned()));
            }
        }

        pub(super) fn header(&self, origin: &str) -> Option<String> {
            let cookies = self.0.lock().ok()?;
            let header = cookies
                .iter()
                .filter(|(at, _, _)| at == origin)
                .map(|(_, name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ");
            (!header.is_empty()).then_some(header)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn builds_a_client_that_is_not_a_browser() {
            let transport = NativeTransport::new().expect("a client");
            assert!(!transport.is_browser());
        }

        #[test]
        fn keeps_cookies_per_origin_until_removed() {
            let jar = CookieJar::default();
            let portal = "https://archives.example.org";
            jar.store(portal, "session=abc; Path=/; HttpOnly");
            jar.store(portal, "licence=1");
            jar.store("https://api.example.org", "key=z");
            assert_eq!(
                jar.header(portal).as_deref(),
                Some("session=abc; licence=1")
            );

            jar.store(portal, "session=def");
            assert_eq!(
                jar.header(portal).as_deref(),
                Some("licence=1; session=def")
            );
            jar.store(portal, "licence=; Max-Age=0");
            jar.store(portal, "session=gone; Max-Age=0");
            assert_eq!(jar.header(portal), None);
            assert_eq!(
                jar.header("https://api.example.org").as_deref(),
                Some("key=z")
            );
            jar.store(portal, "malformed");
            assert_eq!(jar.header(portal), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Access;

    fn endpoint() -> PortalEndpoint {
        PortalEndpoint {
            origin: "https://archives.example.org".to_owned(),
            other_origins: vec!["https://api.example.org".to_owned()],
            start: "https://archives.example.org/search".to_owned(),
            access: Access::Any,
        }
    }

    #[test]
    fn identifies_oxidgene() {
        assert!(USER_AGENT.starts_with("OxidGene/"));
        assert!(USER_AGENT.ends_with("(+https://github.com/trois-six/oxidgene)"));
    }

    #[test]
    fn stays_on_the_portal_origin() {
        let origin = "https://archives.example.org";
        assert_eq!(
            portal_url(origin, "/_recherche-api/moteur?refUnique=x").as_deref(),
            Ok("https://archives.example.org/_recherche-api/moteur?refUnique=x")
        );
        for path in [
            "//elsewhere.example.org/",
            "https://elsewhere.example.org/",
            "relative",
            "/path with space",
            "/path#fragment",
            "/\\elsewhere",
            "",
        ] {
            assert_eq!(
                portal_url(origin, path),
                Err(FetchError::NotSameOrigin),
                "{path}"
            );
        }
    }

    #[test]
    fn reaches_only_the_declared_origins() {
        let endpoint = endpoint();
        assert_eq!(
            request_url(&endpoint, &PortalRequest::get("/search?q=1")).as_deref(),
            Ok("https://archives.example.org/search?q=1")
        );
        let api = PortalRequest::post("https://api.example.org/v1/Query", "application/json", "{}")
            .header("ApiKey", "public");
        assert_eq!(
            request_url(&endpoint, &api).as_deref(),
            Ok("https://api.example.org/v1/Query")
        );
        for url in [
            "https://elsewhere.example.org/v1",
            "https://api.example.org.elsewhere.example/v1",
            "http://archives.example.org/search",
            "https://api.example.org//elsewhere.example/",
        ] {
            assert_eq!(
                request_url(&endpoint, &PortalRequest::get(url)),
                Err(FetchError::NotSameOrigin),
                "{url}"
            );
        }
    }

    #[test]
    fn sends_only_allow_listed_headers_and_bodies_on_posts() {
        let endpoint = endpoint();
        let accepted = PortalRequest::get("/search").header("accept", "application/json");
        assert!(request_url(&endpoint, &accepted).is_ok());
        for request in [
            PortalRequest::get("/search").header("Cookie", "session=stolen"),
            PortalRequest::get("/search").header("Accept", "text/html\r\nX-Other: 1"),
            PortalRequest {
                body: Some("q=1".to_owned()),
                ..PortalRequest::get("/search")
            },
        ] {
            assert_eq!(
                request_url(&endpoint, &request),
                Err(FetchError::NotAllowed),
                "{request:?}"
            );
        }
        let form = PortalRequest::post("/search", "application/x-www-form-urlencoded", "q=1");
        assert_eq!(form.method.as_str(), "POST");
        assert!(request_url(&endpoint, &form).is_ok());
    }

    #[test]
    fn checks_the_answer_of_a_page() {
        let origins = vec!["https://archives.example.org".to_owned()];
        let url = "https://archives.example.org/_recherche-api/moteur";
        let answer = |status: u16, url: &str, body: &str| PageAnswer {
            status: Some(status),
            url: Some(url.to_owned()),
            body: Some(body.to_owned()),
            error: None,
        };
        assert_eq!(answer(200, url, "{}").result(&origins), Ok("{}".to_owned()));
        assert_eq!(
            answer(503, url, "").result(&origins),
            Err(FetchError::Status(503))
        );
        assert_eq!(
            answer(200, "https://elsewhere.example.org/", "").result(&origins),
            Err(FetchError::NotSameOrigin)
        );
        // A challenge refusing the request, and one let through as a success,
        // which the adapter tells apart when it cannot read it.
        assert_eq!(
            answer(403, url, "<html><title>Request Rejected</title></html>").result(&origins),
            Err(FetchError::Challenged)
        );
        assert_eq!(
            answer(403, url, "Forbidden").result(&origins),
            Err(FetchError::Status(403))
        );
        let failed = |error: &str| PageAnswer {
            error: Some(error.to_owned()),
            ..PageAnswer::default()
        };
        assert_eq!(failed("timeout").result(&origins), Err(FetchError::Timeout));
        assert_eq!(failed("network").result(&origins), Err(FetchError::Network));
        let parsed: PageAnswer = serde_json::from_str(
            r#"{"status": 200, "url": "https://archives.example.org/", "body": "x"}"#,
        )
        .unwrap();
        assert_eq!(parsed.result(&origins), Ok("x".to_owned()));
    }

    #[test]
    fn reads_the_origin_of_an_address() {
        assert_eq!(
            origin_of("https://archives.example.org/a/b?c"),
            Some("https://archives.example.org")
        );
        assert_eq!(
            origin_of("https://archives.example.org:8443"),
            Some("https://archives.example.org:8443")
        );
        assert_eq!(origin_of("/path"), None);
        assert_eq!(origin_of("https:///path"), None);
    }
}
