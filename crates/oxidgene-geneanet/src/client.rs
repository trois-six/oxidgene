//! Throttled HTTP client for Geneanet's media API.
//!
//! Everything here needs a logged-in `www.geneanet.org` session cookie: the
//! media are private, and the endpoints 403 without it. The transport
//! impersonates a current Chrome (see [`EMULATION`]): Cloudflare fronts
//! geneanet.org and challenges a client's TLS/HTTP2 fingerprint before the
//! cookie is ever read, and a plain HTTP client is challenged whatever cookie
//! it presents.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::sleep;
use wreq::StatusCode;
use wreq::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue};

use crate::model::{Deposit, Reference, ReferenceEntry};

pub const DEFAULT_BASE_URL: &str = "https://www.geneanet.org";
const DEPOSITS_PER_PAGE: usize = 100;

/// `/media/api/references` caps the page size at 100 whatever is asked for.
const REFERENCES_PER_PAGE: usize = 100;

/// Download path, trailing slash included.
///
/// The slash matters: without it Geneanet answers `301` to the same path *with*
/// one, so every download would cost a wasted round trip.
const DOWNLOAD_PATH: &str = "/media/download/";

/// Cookie names that carry an authenticated session.
///
/// `gntsess5` is the session itself; `REMEMBERME` is the Symfony remember-me
/// token, which mints a fresh session on its own. Either alone is enough —
/// everything else Geneanet sets (Cloudflare, consent, analytics, forum) is
/// irrelevant to this API.
const SESSION_COOKIES: [&str; 2] = ["gntsess5", "REMEMBERME"];

/// The browser profile the client presents, TLS and HTTP/2 fingerprints
/// included.
///
/// Measured against the live site (2026-08): plain reqwest+rustls is
/// challenged by Cloudflare on every data route whatever cookie it sends,
/// Chrome 131 profiles are already challenged too, and a current-Chrome
/// profile passes everything. So this pin must track current Chrome: bump it
/// when `wreq-util` is updated, and keep that dependency current. There is no
/// "latest" alias to defer to — `Profile::default()` is Chrome 100 and the
/// enum is `#[non_exhaustive]` — so the pin is explicit on purpose.
///
/// The emulation also sets the matching User-Agent and header set. Overriding
/// the agent with our own would pair a Chrome fingerprint with a non-Chrome
/// agent string, which is the one combination that looks spoofed.
const EMULATION: wreq_util::Profile = wreq_util::Emulation::Chrome149;

/// How politely to hit the API.
///
/// There is no concurrency knob: collection now costs ~15 requests, so issuing
/// them one at a time with a pause between costs nothing worth having and
/// keeps the traffic shaped like a person rather than a crawler.
#[derive(Debug, Clone, Copy)]
pub struct Throttle {
    /// Pause taken after every request.
    pub delay: Duration,
}

impl Default for Throttle {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(100),
        }
    }
}

pub struct Client {
    http: wreq::Client,
    base_url: String,
    throttle: Throttle,
}

// `wreq::Client` has no `Debug` impl, so format the fields that matter by hand.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("throttle", &self.throttle)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Builds a client authenticated with a raw `Cookie:` header value, as
    /// copied from the browser's developer tools.
    ///
    /// Only the session cookie is needed — see [`SESSION_COOKIES`]. Passing the
    /// whole browser cookie works too, it just carries more than it needs to.
    pub fn new(cookie: &str, base_url: Option<String>, throttle: Throttle) -> Result<Self> {
        if !carries_a_session(cookie) {
            bail!(
                "the cookie holds neither {} nor {}, so Geneanet will reject every request. \
                 Copy one of them from the browser: developer tools → Application → Cookies → \
                 www.geneanet.org.",
                SESSION_COOKIES[0],
                SESSION_COOKIES[1],
            );
        }

        let mut headers = HeaderMap::new();
        let mut cookie_value = HeaderValue::from_str(cookie.trim())
            .context("the cookie contains characters that cannot go in an HTTP header")?;
        cookie_value.set_sensitive(true);
        headers.insert(COOKIE, cookie_value);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        // The media API only answers JSON when it believes it is talking to the
        // manager's XHR layer.
        headers.insert(
            "X-Requested-With",
            HeaderValue::from_static("XMLHttpRequest"),
        );

        let http = wreq::Client::builder()
            .emulation(EMULATION)
            .default_headers(headers)
            .timeout(Duration::from_secs(120))
            .build()
            .context("could not build the HTTP client")?;

        Ok(Self {
            http,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            throttle,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Walks `/media/api/deposits` to the end and returns every deposit.
    pub async fn list_deposits(&self) -> Result<Vec<Deposit>> {
        let mut deposits = Vec::new();

        for page in 1.. {
            let url = format!("{}/media/api/deposits", self.base_url);
            let response = self
                .http
                .get(&url)
                .query(&[
                    ("page", page.to_string()),
                    ("per_page", DEPOSITS_PER_PAGE.to_string()),
                ])
                .send()
                .await
                .with_context(|| format!("requesting deposits page {page}"))?;

            let response = check_status(response, &format!("deposits page {page}"))?;
            let batch: Vec<Deposit> = response
                .json()
                .await
                .with_context(|| format!("decoding deposits page {page}"))?;

            let batch_len = batch.len();
            deposits.extend(batch);

            if batch_len < DEPOSITS_PER_PAGE {
                break;
            }
            sleep(self.throttle.delay).await;
        }

        Ok(deposits)
    }

    /// Walks `/media/api/references` and returns every person↔media link.
    ///
    /// This is the cheap path, and the one to prefer: each entry carries its
    /// whole deposit inline, so the entire mapping arrives in a handful of
    /// paginated calls rather than one request per view. On the reference tree
    /// that is 6 requests instead of 618 — which matters well beyond speed,
    /// because request volume is what draws Cloudflare's attention.
    ///
    /// The endpoint caps `per_page` at 100 whatever you ask for.
    pub async fn list_references(&self) -> Result<Vec<ReferenceEntry>> {
        let mut entries = Vec::new();

        for page in 1.. {
            let url = format!("{}/media/api/references", self.base_url);
            let response = self
                .http
                .get(&url)
                .query(&[
                    ("page", page.to_string()),
                    ("per_page", REFERENCES_PER_PAGE.to_string()),
                ])
                .send()
                .await
                .with_context(|| format!("requesting references page {page}"))?;

            let response = check_status(response, &format!("references page {page}"))?;
            let batch: Vec<ReferenceEntry> = response
                .json()
                .await
                .with_context(|| format!("decoding references page {page}"))?;

            let batch_len = batch.len();
            entries.extend(batch);

            if batch_len < REFERENCES_PER_PAGE {
                break;
            }
            sleep(self.throttle.delay).await;
        }

        Ok(entries)
    }

    /// Fetches the persons linked to one view.
    ///
    /// Only needed to locate links inside a multi-page deposit, which
    /// [`Self::list_references`] cannot pin to a page.
    pub async fn view_references(&self, deposit_id: i64, view_id: i64) -> Result<Vec<Reference>> {
        let url = format!(
            "{}/media/api/deposits/{deposit_id}/views/{view_id}/references",
            self.base_url
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("requesting references for view {view_id}"))?;

        let response = check_status(
            response,
            &format!("references for deposit {deposit_id} view {view_id}"),
        )?;

        response
            .json()
            .await
            .with_context(|| format!("decoding references for view {view_id}"))
    }

    /// Asks how many bytes a deposit's original is, without fetching it.
    ///
    /// Returns `None` when the server does not say. That is not a failure —
    /// it is how a multi-page deposit answers, because it is assembled into an
    /// archive on the fly and streamed without a `Content-Length`. A `None`
    /// therefore also means "this one cannot be matched by size".
    pub async fn content_length(&self, deposit_id: i64) -> Result<Option<u64>> {
        let url = format!("{}{DOWNLOAD_PATH}", self.base_url);
        let response = self
            .http
            .head(&url)
            .query(&[("deposits[]", deposit_id.to_string())])
            .send()
            .await
            .with_context(|| format!("sizing deposit {deposit_id}"))?;

        let response = check_status(response, &format!("size of deposit {deposit_id}"))?;
        let length = response
            .headers()
            .get(wreq::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&n| n > 0);

        sleep(self.throttle.delay).await;

        Ok(length)
    }

    /// Fetches an arbitrary media URL, such as a per-page rendition.
    pub async fn download_url(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("downloading {url}"))?;

        let response = check_status(response, &format!("download of {url}"))?;
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("reading the body of {url}"))?;

        sleep(self.throttle.delay).await;

        Ok(bytes.to_vec())
    }

    /// Downloads a deposit's original file, byte for byte.
    ///
    /// One deposit per request on purpose: the response for several deposits is
    /// a ZIP whose entries are named after the *original upload*, which neither
    /// matches the deposit title nor carries the deposit id — so batching would
    /// leave us guessing which file came from which deposit. One request, one
    /// deposit, no guessing.
    ///
    /// Returns the bytes and the filename Geneanet suggests, if any. A deposit
    /// with several views yields a ZIP of its pages.
    pub async fn download_deposit(&self, deposit_id: i64) -> Result<(Vec<u8>, Option<String>)> {
        let url = format!("{}{DOWNLOAD_PATH}", self.base_url);
        let response = self
            .http
            .get(&url)
            .query(&[("deposits[]", deposit_id.to_string())])
            .send()
            .await
            .with_context(|| format!("downloading deposit {deposit_id}"))?;

        let response = check_status(response, &format!("download of deposit {deposit_id}"))?;

        let filename = response
            .headers()
            .get(wreq::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_disposition_filename);

        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("reading the body of deposit {deposit_id}"))?;

        sleep(self.throttle.delay).await;

        Ok((bytes.to_vec(), filename))
    }
}

/// Whether a `Cookie:` header value carries something that authenticates.
///
/// Matches on the cookie *name*, so a value that merely contains the word does
/// not count.
fn carries_a_session(cookie: &str) -> bool {
    cookie.split(';').any(|pair| {
        let name = pair.split('=').next().unwrap_or_default().trim();
        SESSION_COOKIES.contains(&name)
    })
}

/// Whether a response is a Cloudflare challenge rather than Geneanet's answer.
///
/// Cloudflare fronts geneanet.org and can decide, from the client's TLS and
/// HTTP/2 fingerprint, that a non-browser deserves an interactive challenge. It
/// answers `403` with `cf-mitigated: challenge` and a "Just a moment…" page —
/// which looks exactly like an auth failure but has nothing to do with the
/// cookie, and telling the user to refresh their cookie sends them chasing the
/// wrong thing.
fn is_cloudflare_challenge(response: &wreq::Response) -> bool {
    response.headers().contains_key("cf-mitigated")
        || response
            .headers()
            .get(wreq::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html"))
}

/// Turns an HTTP failure into a message that says what to do about it.
fn check_status(response: wreq::Response, what: &str) -> Result<wreq::Response> {
    match response.status() {
        s if s.is_success() => Ok(response),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN if is_cloudflare_challenge(&response) => {
            bail!(
                "Cloudflare challenged the {what} (HTTP {}, an HTML challenge page rather than \
                 Geneanet's answer). This is not an authentication problem and a fresh cookie \
                 will not fix it: Cloudflare decided this client looks automated. The emulated \
                 browser profile may have gone stale — update the wreq-util dependency and the \
                 EMULATION pin — or wait for the challenge to lapse and re-run more gently \
                 (higher --delay-ms).",
                response.status()
            )
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => bail!(
            "Geneanet refused the {what} (HTTP {}). The session cookie is most likely \
             expired — copy a fresh one from the browser.",
            response.status()
        ),
        StatusCode::TOO_MANY_REQUESTS => bail!(
            "Geneanet rate-limited the {what} (HTTP 429). Stopping rather than retrying. \
             Re-run with a higher --delay-ms; already-downloaded files are skipped."
        ),
        status => bail!("the {what} failed with HTTP {status}"),
    }
}

/// Pulls the filename out of a `Content-Disposition` header.
///
/// Prefers the RFC 5987 `filename*=utf-8''…` form, which is the one that keeps
/// accents; Geneanet sends both and the plain `filename=` is transliterated
/// (`Renée.jpg` arrives as `Ren_e.jpg`).
fn parse_content_disposition_filename(header: &str) -> Option<String> {
    if let Some(start) = header.find("filename*=") {
        let value = &header[start + "filename*=".len()..];
        let value = value.split(';').next()?.trim().trim_matches('"');
        if let Some(encoded) = value
            .strip_prefix("utf-8''")
            .or_else(|| value.strip_prefix("UTF-8''"))
            && let Some(decoded) = percent_decode(encoded)
        {
            return Some(decoded);
        }
    }

    let start = header.find("filename=")?;
    let value = &header[start + "filename=".len()..];
    let value = value.split(';').next()?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

/// Minimal percent-decoder for `Content-Disposition` filenames.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = input.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_utf8_filename_over_the_transliterated_one() {
        let header = "attachment; filename=Ren_e.jpg; filename*=utf-8''Ren%C3%A9e.jpg";

        assert_eq!(
            parse_content_disposition_filename(header),
            Some("Renée.jpg".to_string())
        );
    }

    #[test]
    fn falls_back_to_the_plain_filename() {
        let header = "attachment; filename=geneanet_05_08_2026-18_00.zip";

        assert_eq!(
            parse_content_disposition_filename(header),
            Some("geneanet_05_08_2026-18_00.zip".to_string())
        );
    }

    #[test]
    fn handles_a_quoted_filename() {
        let header = "attachment; filename=\"Mariage Pfrimmer.jpg\"";

        assert_eq!(
            parse_content_disposition_filename(header),
            Some("Mariage Pfrimmer.jpg".to_string())
        );
    }

    #[test]
    fn returns_none_when_there_is_no_filename() {
        assert_eq!(parse_content_disposition_filename("inline"), None);
    }

    #[test]
    fn a_malformed_percent_escape_does_not_panic() {
        assert_eq!(percent_decode("Dani%C3"), None);
        assert_eq!(percent_decode("Dani%ZZle"), None);
    }

    #[test]
    fn a_cookie_with_a_newline_is_rejected() {
        let error = Client::new("gntsess5=abc\nInjected: yes", None, Throttle::default())
            .expect_err("a header value cannot contain a newline");

        assert!(error.to_string().contains("cannot go in an HTTP header"));
    }

    #[test]
    fn either_session_cookie_alone_is_accepted() {
        // Measured against the live API: `gntsess5` on its own answers 200, and
        // so does `REMEMBERME`, which mints a fresh session. Nothing else in a
        // browser cookie matters.
        assert!(carries_a_session("gntsess5=abc"));
        assert!(carries_a_session("REMEMBERME=Geneanet.Bundle...%3Aabc"));
        assert!(carries_a_session("autolang=fr; gntsess5=abc; ismobile=0"));
    }

    #[test]
    fn a_cookie_with_no_session_is_refused_before_any_request() {
        // Cloudflare and consent cookies look plausible and authenticate
        // nothing; failing here beats 378 identical 403s.
        let error = Client::new(
            "cf_clearance=abc; __cf_bm=def; autolang=fr",
            None,
            Throttle::default(),
        )
        .expect_err("no session cookie");

        assert!(error.to_string().contains("gntsess5"));
    }

    #[test]
    fn a_session_name_inside_a_value_does_not_count() {
        assert!(!carries_a_session("tracking=gntsess5"));
        assert!(!carries_a_session("x=REMEMBERME"));
    }
}
