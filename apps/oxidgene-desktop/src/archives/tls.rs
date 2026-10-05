//! Completing a certificate chain a portal serves without its intermediate,
//! on WebKitGTK (Archive Portals §6.1).
//!
//! A server that sends its certificate without the intermediate that issued
//! it is refused by WebKitGTK, whose TLS stack does not fetch a missing
//! issuer, while Chromium, Firefox and the macOS and Windows WebViews open
//! it. The window does what those verifiers do, and nothing more: when a
//! page of the archive's own origin fails to load with an unknown issuer as
//! its only error, it fetches the one issuer the certificate names in its
//! Authority Information Access extension (`caIssuers`), verifies the chain
//! — the certificate, that issuer, a root the system trusts — for the page's
//! host name and the current time with rustls's verifier, and only then
//! allows that exact certificate for that host in the archive windows' web
//! context, and loads the page again. Anything else stays refused, and the
//! window says so, with a button opening the page in the system browser.
//!
//! The fixtures are a test authority made with `openssl` for this test
//! alone: a root, an intermediate, a certificate for `archives.example.org`
//! naming the intermediate's address, and an unrelated root. Their keys were
//! not kept.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::desktop::wry::{WebView, WebViewExtUnix};
use oxidgene_archives::transport::origin_of;
use rustls::RootCertStore;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tracing::warn;
use webkit2gtk::gio::TlsCertificateFlags;
use webkit2gtk::gio::prelude::TlsCertificateExt;
use webkit2gtk::{WebContextExt, WebViewExt};

use super::transport::SessionId;
use super::{Inbound, Inbox};

/// The bound on fetching an issuer, as on any portal request.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The largest issuer certificate read; one weighs about two kilobytes.
const MAX_CERTIFICATE_BYTES: usize = 64 * 1024;

/// `1.3.6.1.5.5.7.1.1`, the Authority Information Access extension.
const AUTHORITY_INFO_ACCESS: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x01, 0x01];

/// `1.3.6.1.5.5.7.48.2`, its access method naming the issuer's certificate.
const CA_ISSUERS: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x30, 0x02];

/// Handles the TLS failures of a window's pages. `accepted` holds the
/// origins the window was sent to; failures elsewhere are not completed.
/// A host is completed once per window: a second failure there, after its
/// certificate was allowed, stays refused rather than loading again.
pub(super) fn watch(
    webview: &WebView,
    session: SessionId,
    accepted: Arc<Mutex<Vec<String>>>,
    inbox: Inbox,
) {
    let tried = RefCell::new(HashSet::new());
    webview.webview().connect_load_failed_with_tls_errors(
        move |view, failing, certificate, errors| {
            let page = failing.to_owned();
            let completion = (errors == TlsCertificateFlags::UNKNOWN_CA)
                .then(|| completion(&page, &accepted, &certificate.certificate()?))
                .flatten()
                .filter(|completion| tried.borrow_mut().insert(completion.host.clone()));
            let Some(Completion { host, leaf, issuer }) = completion else {
                untrusted(&inbox, session, page);
                return true;
            };
            let (sender, verified) = futures_channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = sender.send(complete(&leaf, &issuer, &host).then_some(host));
            });
            let (view, certificate, inbox) = (view.clone(), certificate.clone(), inbox.clone());
            webkit2gtk::glib::MainContext::default().spawn_local(async move {
                match verified.await {
                    Ok(Some(host)) => {
                        if let Some(context) = view.context() {
                            context.allow_tls_certificate_for_host(&certificate, &host);
                        }
                        view.load_uri(&page);
                    }
                    _ => {
                        warn!(
                            error = "archive_certificate",
                            "could not complete an archive's certificate chain"
                        );
                        untrusted(&inbox, session, page);
                    }
                }
            });
            // Handled here: WebKit shows no error page of its own.
            true
        },
    );
}

/// Opens `page` in the system's default browser, which verifies its
/// certificate its own way.
pub(super) fn open_in_browser(page: &str) {
    let launched = webkit2gtk::gio::AppInfo::launch_default_for_uri(
        page,
        None::<&webkit2gtk::gio::AppLaunchContext>,
    );
    if launched.is_err() {
        warn!(
            error = "archive_browser",
            "could not open an archive page in the system browser"
        );
    }
}

fn untrusted(inbox: &Inbox, session: SessionId, page: String) {
    if let Ok(mut inbox) = inbox.lock() {
        inbox.push((session, Inbound::Untrusted(page)));
    }
}

/// What completing a chain needs: the page's host, its certificate, and
/// the address of that certificate's issuer.
struct Completion {
    host: String,
    leaf: Vec<u8>,
    issuer: String,
}

/// The completion to try for a page whose certificate's issuer is unknown:
/// a page on an origin the window was sent to, over `https`, whose
/// certificate names its issuer's address.
fn completion(page: &str, accepted: &Mutex<Vec<String>>, leaf: &[u8]) -> Option<Completion> {
    let origin = origin_of(page)?;
    let on_origin = accepted
        .lock()
        .is_ok_and(|accepted| accepted.iter().any(|known| known == origin));
    let authority = origin.strip_prefix("https://").filter(|_| on_origin)?;
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    Some(Completion {
        host: host.to_owned(),
        issuer: ca_issuers(leaf)?,
        leaf: leaf.to_vec(),
    })
}

/// Fetches the issuer and verifies the completed chain against the system's
/// roots, now. Runs on a thread of its own: the event loop does not wait.
fn complete(leaf: &[u8], issuer: &str, host: &str) -> bool {
    let Some(intermediate) = fetch(issuer) else {
        return false;
    };
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    verifies(leaf, &intermediate, host, UnixTime::now(), roots)
}

/// The issuer's certificate, DER or PEM, read once without a retry.
fn fetch(url: &str) -> Option<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let body = runtime.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent(oxidgene_archives::transport::USER_AGENT)
            .timeout(TIMEOUT)
            .retry(reqwest::retry::never())
            .build()
            .ok()?;
        let mut response = client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if body.len() + chunk.len() > MAX_CERTIFICATE_BYTES {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        Some(body)
    })?;
    der_or_pem(body)
}

/// A certificate's DER bytes, from DER or from one PEM block.
fn der_or_pem(body: Vec<u8>) -> Option<Vec<u8>> {
    use base64::Engine;

    if body.first() == Some(&0x30) {
        return Some(body);
    }
    let text = std::str::from_utf8(&body).ok()?;
    let start = text.find("-----BEGIN CERTIFICATE-----")? + "-----BEGIN CERTIFICATE-----".len();
    let end = start + text[start..].find("-----END CERTIFICATE-----")?;
    let encoded: String = text[start..end].split_whitespace().collect();
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()
}

/// Whether `leaf`, issued by `intermediate`, chains to one of `roots` and is
/// valid for `host` at `now`.
fn verifies(
    leaf: &[u8],
    intermediate: &[u8],
    host: &str,
    now: UnixTime,
    roots: RootCertStore,
) -> bool {
    let Ok(name) = ServerName::try_from(host.to_owned()) else {
        return false;
    };
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let Ok(verifier) =
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider).build()
    else {
        return false;
    };
    verifier
        .verify_server_cert(
            &CertificateDer::from(leaf),
            &[CertificateDer::from(intermediate)],
            &name,
            &[],
            now,
        )
        .is_ok()
}

/// One DER element: its tag, its content, and what follows it.
fn element(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (length, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        let length = bytes
            .iter()
            .fold(0usize, |length, &byte| (length << 8) | usize::from(byte));
        (length, rest)
    };
    (rest.len() >= length).then(|| (tag, &rest[..length], &rest[length..]))
}

/// The elements of a constructed element's content, in order.
fn elements(mut content: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (tag, value, rest) = element(content)?;
        content = rest;
        Some((tag, value))
    })
}

/// The first `http` or `https` address of the `caIssuers` access method in
/// a certificate's Authority Information Access extension.
fn ca_issuers(certificate: &[u8]) -> Option<String> {
    const SEQUENCE: u8 = 0x30;
    const EXTENSIONS: u8 = 0xa3;
    const OID: u8 = 0x06;
    const OCTET_STRING: u8 = 0x04;
    const URI: u8 = 0x86;

    let (_, certificate, _) = element(certificate).filter(|(tag, ..)| *tag == SEQUENCE)?;
    let (_, signed, _) = element(certificate).filter(|(tag, ..)| *tag == SEQUENCE)?;
    let (_, extensions) = elements(signed).find(|(tag, _)| *tag == EXTENSIONS)?;
    let (_, extensions, _) = element(extensions).filter(|(tag, ..)| *tag == SEQUENCE)?;
    let access = elements(extensions).find_map(|(_, extension)| {
        let mut fields = elements(extension);
        (fields.next()? == (OID, AUTHORITY_INFO_ACCESS))
            .then(|| fields.find(|(tag, _)| *tag == OCTET_STRING))
            .flatten()
            .map(|(_, value)| value)
    })?;
    let (_, descriptions, _) = element(access).filter(|(tag, ..)| *tag == SEQUENCE)?;
    elements(descriptions).find_map(|(_, description)| {
        let mut fields = elements(description);
        if fields.next()? != (OID, CA_ISSUERS) {
            return None;
        }
        let (tag, location) = fields.next()?;
        let location = std::str::from_utf8(location).ok()?;
        (tag == URI && (location.starts_with("http://") || location.starts_with("https://")))
            .then(|| location.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &[u8] = include_bytes!("fixtures/root.der");
    const INTERMEDIATE: &[u8] = include_bytes!("fixtures/intermediate.der");
    const INTERMEDIATE_PEM: &str = include_str!("fixtures/intermediate.pem");
    const LEAF: &[u8] = include_bytes!("fixtures/leaf.der");
    const STRANGER: &[u8] = include_bytes!("fixtures/stranger.der");

    /// 2027-01-15, within every fixture's validity.
    fn during() -> UnixTime {
        UnixTime::since_unix_epoch(Duration::from_secs(1_800_000_000))
    }

    fn trusting(root: &[u8]) -> RootCertStore {
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(root.to_vec())).unwrap();
        roots
    }

    #[test]
    fn reads_the_issuer_address_of_a_certificate() {
        assert_eq!(
            ca_issuers(LEAF).as_deref(),
            Some("http://ca.example.org/intermediate.der")
        );
        // A root names no issuer; damaged bytes name none either.
        assert_eq!(ca_issuers(ROOT), None);
        assert_eq!(ca_issuers(&LEAF[..LEAF.len() / 2]), None);
        assert_eq!(ca_issuers(&[]), None);
    }

    #[test]
    fn a_completed_chain_to_a_trusted_root_verifies() {
        assert!(verifies(
            LEAF,
            INTERMEDIATE,
            "archives.example.org",
            during(),
            trusting(ROOT)
        ));
    }

    #[test]
    fn anything_else_stays_refused() {
        let host = "archives.example.org";
        // Another name.
        assert!(!verifies(
            LEAF,
            INTERMEDIATE,
            "elsewhere.example.org",
            during(),
            trusting(ROOT)
        ));
        // A root the system does not trust.
        assert!(!verifies(
            LEAF,
            INTERMEDIATE,
            host,
            during(),
            trusting(STRANGER)
        ));
        // An issuer that did not sign the certificate.
        assert!(!verifies(LEAF, STRANGER, host, during(), trusting(ROOT)));
        // Expired: the certificate ends in 2031.
        let later = UnixTime::since_unix_epoch(Duration::from_secs(2_000_000_000));
        assert!(!verifies(LEAF, INTERMEDIATE, host, later, trusting(ROOT)));
        // No root at all.
        assert!(!verifies(
            LEAF,
            INTERMEDIATE,
            host,
            during(),
            RootCertStore::empty()
        ));
    }

    #[test]
    fn reads_an_issuer_served_as_der_or_pem() {
        assert_eq!(
            der_or_pem(INTERMEDIATE.to_vec()).as_deref(),
            Some(INTERMEDIATE)
        );
        assert_eq!(
            der_or_pem(INTERMEDIATE_PEM.as_bytes().to_vec()).as_deref(),
            Some(INTERMEDIATE)
        );
        assert_eq!(der_or_pem(b"<html>not found</html>".to_vec()), None);
    }

    #[test]
    fn completes_only_pages_of_the_window_s_origins_over_https() {
        let accepted = Mutex::new(vec![
            "https://archives.example.org".to_owned(),
            "https://viewer.example.org:8443".to_owned(),
        ]);
        let page = completion(
            "https://archives.example.org/ark:/00000/a1?vue=5",
            &accepted,
            LEAF,
        )
        .expect("a completion");
        assert_eq!(page.host, "archives.example.org");
        assert_eq!(page.issuer, "http://ca.example.org/intermediate.der");
        assert_eq!(
            completion("https://viewer.example.org:8443/v", &accepted, LEAF).map(|c| c.host),
            Some("viewer.example.org".to_owned())
        );
        for elsewhere in [
            "https://elsewhere.example.org/",
            "http://archives.example.org/",
            "about:blank",
        ] {
            assert!(
                completion(elsewhere, &accepted, LEAF).is_none(),
                "{elsewhere}"
            );
        }
        // A certificate naming no issuer has nothing to complete.
        assert!(completion("https://archives.example.org/", &accepted, ROOT).is_none());
    }
}
