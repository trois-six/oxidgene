//! How an adapter's requests reach a portal.
//!
//! An adapter never builds a client: it issues same-origin `GET`s through a
//! [`PortalFetch`] bound to the collection's portal, which a
//! [`PortalTransport`] opens. The `native` feature provides one over
//! `reqwest`; the desktop provides another that runs the requests inside its
//! archive window, the only one that passes a portal's anti-bot challenge.

use std::fmt;
use std::time::Duration;

use crate::platform::{BoxFuture, PortalEndpoint};

/// How OxidGene names itself to a portal.
pub const USER_AGENT: &str = concat!(
    "OxidGene/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/trois-six/oxidgene)"
);

/// The bound on one request, connection included. Requests are never retried.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The largest response body read; a search answer is a few hundred
/// kilobytes at most.
pub const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;

/// One same-origin `GET` on a portal, returning the body.
pub trait PortalFetch: Send + Sync {
    /// `path_and_query` starts with `/`; the origin is the fetcher's.
    fn get<'a>(&'a self, path_and_query: &'a str) -> BoxFuture<'a, Result<String, FetchError>>;
}

/// Opens fetchers on portals.
pub trait PortalTransport: Send + Sync {
    /// Whether requests run in a browser page, which passes the anti-bot
    /// challenge of a portal whose access is `browser`.
    fn is_browser(&self) -> bool;

    /// A fetcher bound to the endpoint's origin. A browser transport first
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
    /// The connection failed.
    Network,
    /// The portal answered with an error status.
    Status(u16),
    /// The body exceeds [`MAX_BODY_BYTES`].
    TooLarge,
    /// The request, or a redirect, left the portal's origin.
    NotSameOrigin,
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("the portal did not answer in time"),
            Self::Network => f.write_str("the portal could not be reached"),
            Self::Status(status) => write!(f, "the portal answered with status {status}"),
            Self::TooLarge => f.write_str("the portal's answer is too large"),
            Self::NotSameOrigin => f.write_str("the request left the portal's origin"),
        }
    }
}

impl std::error::Error for FetchError {}

/// The absolute address of `path_and_query` on `origin`, refusing anything
/// that is not a path on that origin.
pub fn portal_url(origin: &str, path_and_query: &str) -> Result<String, FetchError> {
    let on_origin = path_and_query.starts_with('/')
        && !path_and_query.starts_with("//")
        && !path_and_query.contains(['\\', '#'])
        && !path_and_query
            .chars()
            .any(|c| c.is_whitespace() || c.is_control());
    if on_origin {
        Ok(format!("{origin}{path_and_query}"))
    } else {
        Err(FetchError::NotSameOrigin)
    }
}

#[cfg(feature = "native")]
pub use native::NativeTransport;

#[cfg(feature = "native")]
mod native {
    use super::{
        BoxFuture, FetchError, MAX_BODY_BYTES, PortalEndpoint, PortalFetch, PortalTransport,
        TIMEOUT, USER_AGENT, portal_url,
    };

    /// The most redirects followed, all within the portal's origin.
    const MAX_REDIRECTS: usize = 5;

    /// Requests from this process over `reqwest`, for portals whose access is
    /// `any`.
    pub struct NativeTransport {
        client: reqwest::Client,
    }

    impl NativeTransport {
        pub fn new() -> Result<Self, FetchError> {
            let redirects = reqwest::redirect::Policy::custom(|attempt| {
                let same_origin = attempt
                    .previous()
                    .first()
                    .is_some_and(|first| first.origin() == attempt.url().origin());
                if !same_origin {
                    attempt.error("a redirect left the portal's origin")
                } else if attempt.previous().len() > MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            });
            let client = reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .timeout(TIMEOUT)
                .retry(reqwest::retry::never())
                .redirect(redirects)
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
                origin: endpoint.origin.clone(),
            };
            Box::pin(async move { Ok(Box::new(fetch) as Box<dyn PortalFetch>) })
        }
    }

    struct NativeFetch {
        client: reqwest::Client,
        origin: String,
    }

    impl PortalFetch for NativeFetch {
        fn get<'a>(&'a self, path_and_query: &'a str) -> BoxFuture<'a, Result<String, FetchError>> {
            Box::pin(async move {
                let url = portal_url(&self.origin, path_and_query)?;
                let mut response = self.client.get(url).send().await.map_err(classify)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(FetchError::Status(status.as_u16()));
                }
                let mut body = Vec::new();
                while let Some(chunk) = response.chunk().await.map_err(classify)? {
                    if body.len() + chunk.len() > MAX_BODY_BYTES {
                        return Err(FetchError::TooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(String::from_utf8_lossy(&body).into_owned())
            })
        }
    }

    fn classify(error: reqwest::Error) -> FetchError {
        if error.is_timeout() {
            FetchError::Timeout
        } else if error.is_redirect() {
            FetchError::NotSameOrigin
        } else {
            FetchError::Network
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
