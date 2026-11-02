//! HTTP caching.
//!
//! - Versioned assets (docs/architecture.md §4.3): a picture's route and the
//!   base map take their version as `v`. Once the request is allowed, the
//!   current version is answered immutable and any other is redirected to
//!   it ([`versioned`]), so content is only ever kept under its own version.
//! - Where a tree has several viewers (docs/authentication.md §9.1,
//!   docs/privacy.md §7), the web build keeps no answer across viewers, by
//!   the browser or by an intermediary: every JSON answer of the API,
//!   REST's and GraphQL's, that sets no caching of its own is
//!   `Cache-Control: no-store`, since it holds what one viewer may see of a
//!   tree; a held picture is `private` and varies on the credential
//!   (`MEDIA_VARY`), so a browser shared by two accounts never shows one
//!   the other's copy. The desktop has one viewer and sends no `Vary`: its
//!   per-launch token would otherwise make every launch miss the pictures
//!   the previous one cached.

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

/// How a held picture is kept at its version: by the viewer's client only,
/// for a year, never asked for again.
pub const IMMUTABLE: &str = "private, max-age=31536000, immutable";

/// How an asset anyone may read is kept at its version: the base map.
pub const IMMUTABLE_PUBLIC: &str = "public, max-age=31536000, immutable";

/// The version a client names, `?v=`.
#[derive(Debug, Default, Deserialize)]
pub struct VersionQuery {
    pub v: Option<String>,
}

/// The headers of a versioned asset answered at `current`, its version, or
/// the redirect to it when the request names another (`requested`) or none.
///
/// Called once the request has been allowed to read the content, so a
/// redirect discloses the current version only to whoever may read it. The
/// redirect is never stored: a stale address must never come to hold the
/// new content, and the content is only answered at its own version.
pub fn versioned(
    uri: &Uri,
    requested: Option<&str>,
    current: &str,
    rule: &'static str,
) -> Result<HeaderMap, Moved> {
    let mut headers = HeaderMap::new();
    #[cfg(feature = "auth")]
    if rule == IMMUTABLE {
        vary_on_credentials(&mut headers);
    }
    if requested == Some(current) {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(rule));
        return Ok(headers);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // A path and a query of hexadecimal digits always make a header value.
    let moved = HeaderValue::from_str(&at_version(uri, current)).is_ok_and(|location| {
        headers.insert(header::LOCATION, location);
        true
    });
    Err(Moved { headers, moved })
}

/// The answer to an address naming another version than the current one:
/// `307` to the current one's address, never stored.
#[derive(Debug)]
pub struct Moved {
    headers: HeaderMap,
    /// Whether `headers` name where to; a `404` otherwise.
    moved: bool,
}

impl IntoResponse for Moved {
    fn into_response(self) -> Response {
        let status = if self.moved {
            StatusCode::TEMPORARY_REDIRECT
        } else {
            StatusCode::NOT_FOUND
        };
        (status, self.headers).into_response()
    }
}

/// `uri`'s path and query with `v` set to `version`, the other parameters
/// kept in their order.
fn at_version(uri: &Uri, version: &str) -> String {
    let mut query: Vec<&str> = uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty() && *pair != "v" && !pair.starts_with("v="))
        .collect();
    let version = format!("v={version}");
    query.push(&version);
    format!("{}?{}", uri.path(), query.join("&"))
}

/// What a held picture varies on in the web build: the session cookie and
/// the bearer token.
#[cfg(feature = "auth")]
pub const MEDIA_VARY: &str = "Cookie, Authorization";

/// Mark a JSON answer of the API `no-store` unless it says otherwise.
#[cfg(feature = "auth")]
pub async fn no_store_json(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
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

/// Add the credential to what a held picture varies on.
#[cfg(feature = "auth")]
pub fn vary_on_credentials(headers: &mut HeaderMap) {
    headers.append(header::VARY, HeaderValue::from_static(MEDIA_VARY));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_version_is_kept_and_any_other_redirected_to_it() {
        let uri: Uri = "/api/v1/users/u/avatar?size=64&v=old".parse().unwrap();
        let kept = versioned(&uri, Some("new"), "new", IMMUTABLE).unwrap();
        assert_eq!(kept[header::CACHE_CONTROL], IMMUTABLE);

        for requested in [Some("old"), None, Some("")] {
            let redirect = versioned(&uri, requested, "new", IMMUTABLE)
                .unwrap_err()
                .into_response();
            assert_eq!(redirect.status(), StatusCode::TEMPORARY_REDIRECT);
            assert_eq!(redirect.headers()[header::CACHE_CONTROL], "no-store");
            assert_eq!(
                redirect.headers()[header::LOCATION],
                "/api/v1/users/u/avatar?size=64&v=new"
            );
        }
        let bare: Uri = "/api/v1/reference/basemap".parse().unwrap();
        let redirect = versioned(&bare, None, "abc", IMMUTABLE_PUBLIC)
            .unwrap_err()
            .into_response();
        assert_eq!(
            redirect.headers()[header::LOCATION],
            "/api/v1/reference/basemap?v=abc"
        );
    }
}
