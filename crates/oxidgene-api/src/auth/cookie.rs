//! The cookies of the Backend-for-Frontend (docs/authentication.md §4.3,
//! §4.4): `__Host-oxidgene`, the session, `__Host-oxidgene-login`, the
//! binding of a sign-in under way, and `__Host-oxidgene-share`, the share
//! links a browser opened (docs/authorization.md §6.3). All are `Secure` — `localhost` is a
//! secure context to browsers — `HttpOnly`, host-only (`__Host-`, `Path=/`,
//! no `Domain`).

use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, header};

/// The session cookie.
pub(crate) const SESSION: &str = "__Host-oxidgene";
/// The sign-in binding cookie.
pub(crate) const LOGIN: &str = "__Host-oxidgene-login";
/// The share-link grants of a browser, signed in or not
/// (docs/authorization.md §6.3).
pub(crate) const SHARE: &str = "__Host-oxidgene-share";

/// The value of cookie `name` in `headers`.
pub(crate) fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The session cookie: `SameSite=Strict`, for at most `lifetime`.
pub(crate) fn session(value: &str, lifetime: Duration) -> HeaderValue {
    build(SESSION, value, "Strict", lifetime.as_secs())
}

/// The share-link grants: `SameSite=Strict`, for at most `lifetime`.
pub(crate) fn share(value: &str, lifetime: Duration) -> HeaderValue {
    build(SHARE, value, "Strict", lifetime.as_secs())
}

/// The sign-in binding: `SameSite=Lax`, so that the provider's redirect back
/// carries it; ten minutes.
pub(crate) fn login(value: &str) -> HeaderValue {
    build(LOGIN, value, "Lax", 600)
}

/// A header deleting cookie `name`.
pub(crate) fn clear(name: &str) -> HeaderValue {
    let same_site = if name == LOGIN { "Lax" } else { "Strict" };
    build(name, "", same_site, 0)
}

fn build(name: &str, value: &str, same_site: &str, max_age: u64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; Secure; HttpOnly; SameSite={same_site}; Max-Age={max_age}"
    ))
    .expect("cookie values are base64url")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_cookie_has_every_attribute() {
        let cookie = session("abc", Duration::from_secs(604_800));
        let cookie = cookie.to_str().unwrap();
        assert!(cookie.starts_with("__Host-oxidgene=abc;"));
        for attribute in [
            "Path=/",
            "Secure",
            "HttpOnly",
            "SameSite=Strict",
            "Max-Age=604800",
        ] {
            assert!(cookie.contains(attribute), "{attribute} in {cookie}");
        }
        assert!(!cookie.contains("Domain"));
        let login = login("xyz");
        let login = login.to_str().unwrap();
        assert!(login.contains("SameSite=Lax") && login.contains("Max-Age=600"));
        assert!(clear(SESSION).to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn cookies_are_read_by_exact_name() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-oxidgene-login=b; __Host-oxidgene=a; other=c"),
        );
        assert_eq!(read(&headers, SESSION).as_deref(), Some("a"));
        assert_eq!(read(&headers, LOGIN).as_deref(), Some("b"));
        assert_eq!(read(&headers, "missing"), None);
    }
}
