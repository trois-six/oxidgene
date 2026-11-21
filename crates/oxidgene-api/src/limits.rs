//! How long the standalone server lets a request run, and how much a
//! backend takes in ([`IntakeLimits`]).
//!
//! Three limits, so that a slow or stalled client cannot hold a connection,
//! an intake slot (see `crate::service::intake`) or a database transaction
//! forever:
//!
//! - a request must produce its response within [`TimeLimits::request`];
//! - an upload — an import job's source, a media file — gets
//!   [`TimeLimits::upload`] instead, since a large file over a slow link is
//!   legitimately slow;
//! - a request body that sends nothing for [`TimeLimits::body_idle`] is cut
//!   off, however long its overall allowance.
//!
//! A request over its limit answers `503 timeout` in the standard envelope.
//! A response already started — a download streaming — is not cut. The
//! desktop's embedded server applies none of this: it answers its own window
//! alone, and a long local operation is not an attack.

use std::time::Duration;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::timeout::RequestBodyTimeoutLayer;

use oxidgene_core::OxidGeneError;

use crate::access::refusal;

/// The limits [`with_time_limits`] applies.
#[derive(Clone, Copy, Debug)]
pub struct TimeLimits {
    /// Until the response of an ordinary request.
    pub request: Duration,
    /// Until the response of an upload.
    pub upload: Duration,
    /// Between two pieces of a request body.
    pub body_idle: Duration,
}

impl Default for TimeLimits {
    /// Five minutes for a request — the longest exports and rebuilds of a
    /// large tree take well under one — an hour for an upload, and a minute
    /// of silence in a body.
    fn default() -> Self {
        Self {
            request: Duration::from_secs(5 * 60),
            upload: Duration::from_secs(60 * 60),
            body_idle: Duration::from_secs(60),
        }
    }
}

/// How much a backend takes in: the sizes past which a file, an archive or
/// a tree's media are refused (docs/api.md, File import; Media).
///
/// The defaults are the standalone server's, whose users are not its
/// operator: the sizes Geneanet's own upload accepts
/// (docs/geneanet-upload-api.md) where they exist. The desktop takes
/// [`IntakeLimits::local`]: its one user fills their own disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntakeLimits {
    /// The largest genealogy text, in bytes: a `.ged` or a `.gw` as
    /// uploaded, and the `gedcom.ged` of a GEDZIP once inflated. A text is
    /// parsed whole, so this is what bounds an import's memory.
    pub genealogy_bytes: u64,
    /// The most entries an archive may hold.
    pub archive_entries: usize,
    /// The bytes of media a tree may hold in the store; none without a
    /// quota. It bounds an archive too, which is spooled to disk and read
    /// entry by entry: one that could not fit is refused as it uploads.
    pub tree_media_bytes: Option<u64>,
}

impl IntakeLimits {
    /// 350 MiB of genealogy text: what Geneanet accepts for a GEDCOM.
    pub const GENEALOGY_BYTES: u64 = oxidgene_gedcom::import::GENEALOGY_TEXT_LIMIT;
    /// 10 GiB of media per tree: what Geneanet gives an account.
    pub const TREE_MEDIA_BYTES: u64 = 10 * 1024 * 1024 * 1024;

    /// The desktop's: the same bound on a genealogy text, which is the
    /// parser's memory whoever owns the disk, ten times the entries, and
    /// no quota — an archive of any size is read where it lies.
    #[must_use]
    pub fn local() -> Self {
        Self {
            genealogy_bytes: Self::GENEALOGY_BYTES,
            archive_entries: 10 * oxidgene_gedcom::import::GEDZIP_ENTRIES_LIMIT,
            tree_media_bytes: None,
        }
    }

    /// The largest archive spooled to disk — a GEDZIP, a saved Geneanet
    /// session: what the tree's quota can hold, with the genealogy text an
    /// archive carries beside its media; unbounded without a quota.
    #[must_use]
    pub fn archive_bytes(&self) -> Option<u64> {
        self.tree_media_bytes
            .map(|quota| quota.saturating_add(self.genealogy_bytes))
    }

    /// The bounds a GEDZIP is read within.
    #[must_use]
    pub fn gedzip(&self) -> oxidgene_gedcom::import::GedzipLimits {
        oxidgene_gedcom::import::GedzipLimits {
            gedcom_bytes: self.genealogy_bytes,
            media_bytes: crate::media::MAX_UPLOAD_BYTES as u64,
            entries: self.archive_entries,
        }
    }

    /// Refuse `received` bytes of a genealogy text past its bound.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::TooLarge`].
    pub fn check_genealogy(&self, received: u64) -> Result<(), OxidGeneError> {
        if received > self.genealogy_bytes {
            return Err(OxidGeneError::TooLarge(format!(
                "the genealogy file exceeds the {}-byte limit",
                self.genealogy_bytes
            )));
        }
        Ok(())
    }

    /// Refuse `received` bytes of an archive past what a tree can hold.
    ///
    /// # Errors
    ///
    /// [`OxidGeneError::TooLarge`].
    pub fn check_archive(&self, received: u64) -> Result<(), OxidGeneError> {
        match self.archive_bytes() {
            Some(limit) if received > limit => Err(OxidGeneError::TooLarge(format!(
                "the archive exceeds the {limit}-byte limit"
            ))),
            _ => Ok(()),
        }
    }
}

impl Default for IntakeLimits {
    fn default() -> Self {
        Self {
            genealogy_bytes: Self::GENEALOGY_BYTES,
            archive_entries: oxidgene_gedcom::import::GEDZIP_ENTRIES_LIMIT,
            tree_media_bytes: Some(Self::TREE_MEDIA_BYTES),
        }
    }
}

/// A size as an operator writes one: bytes, or a number with a binary
/// unit — `350M`, `10G`, `512K`, with or without `B` or `iB`.
///
/// # Errors
///
/// When `text` is no such size.
pub fn parse_size(text: &str) -> Result<u64, InvalidSize> {
    let text = text.trim();
    let digits = text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(digits);
    let number: u64 = number.parse().map_err(|_| InvalidSize)?;
    let unit = unit.trim().to_ascii_uppercase();
    let unit = unit
        .strip_suffix("IB")
        .or_else(|| unit.strip_suffix('B'))
        .unwrap_or(&unit);
    let shift = match unit {
        "" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        "T" => 40,
        _ => return Err(InvalidSize),
    };
    number.checked_mul(1 << shift).ok_or(InvalidSize)
}

/// A setting that is no size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSize;

/// `router` under `limits`.
pub fn with_time_limits(router: Router, limits: TimeLimits) -> Router {
    router
        .layer(middleware::from_fn_with_state(limits, limit_time))
        .layer(RequestBodyTimeoutLayer::new(limits.body_idle))
}

/// Whether `request` uploads a file: a POST to an import job's or a media
/// file's upload route.
fn is_upload(request: &Request) -> bool {
    let path = request.uri().path();
    request.method() == Method::POST
        && (path.ends_with("/import-jobs") || path.ends_with("/media/upload"))
}

async fn limit_time(State(limits): State<TimeLimits>, request: Request, next: Next) -> Response {
    let limit = if is_upload(&request) {
        limits.upload
    } else {
        limits.request
    };
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "timeout",
            "The request took longer than the server allows",
        ),
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::routing::{get, post};
    use tower::ServiceExt as _;

    use super::*;

    #[test]
    fn sizes_are_bytes_or_a_number_with_a_binary_unit() {
        for (text, bytes) in [
            ("1024", 1024),
            ("512K", 512 * 1024),
            ("350M", 350 * 1024 * 1024),
            ("350 MiB", 350 * 1024 * 1024),
            ("10g", 10 * 1024 * 1024 * 1024),
            ("10GB", 10 * 1024 * 1024 * 1024),
            ("0", 0),
        ] {
            assert_eq!(parse_size(text), Ok(bytes), "{text}");
        }
        for refused in [
            "",
            "M",
            "-1",
            "1.5G",
            "10X",
            "99999999999999999999",
            "16777216T",
        ] {
            assert_eq!(parse_size(refused), Err(InvalidSize), "{refused}");
        }
    }

    #[test]
    fn the_defaults_are_the_servers_and_geneanets_sizes() {
        let server = IntakeLimits::default();
        assert_eq!(server.genealogy_bytes, 350 * 1024 * 1024);
        assert_eq!(server.tree_media_bytes, Some(10 * 1024 * 1024 * 1024));
        assert_eq!(server.archive_entries, 100_000);
        // An archive holds a tree's media and its genealogy text.
        assert_eq!(
            server.archive_bytes(),
            Some(10 * 1024 * 1024 * 1024 + 350 * 1024 * 1024)
        );
        assert!(server.check_archive(1 << 30).is_ok());
        assert!(matches!(
            server.check_archive(11 << 30),
            Err(OxidGeneError::TooLarge(_))
        ));
        assert!(server.check_genealogy(350 * 1024 * 1024).is_ok());
        assert!(matches!(
            server.check_genealogy(350 * 1024 * 1024 + 1),
            Err(OxidGeneError::TooLarge(_))
        ));
    }

    #[test]
    fn the_desktop_has_no_quota_and_keeps_the_bound_on_a_text() {
        let server = IntakeLimits::default();
        let desktop = IntakeLimits::local();
        assert_eq!(desktop.tree_media_bytes, None);
        assert_eq!(desktop.archive_bytes(), None);
        assert!(desktop.check_archive(u64::MAX).is_ok());
        assert_eq!(desktop.genealogy_bytes, server.genealogy_bytes);
        assert_eq!(desktop.gedzip().media_bytes, 128 * 1024 * 1024);
    }

    fn limits() -> TimeLimits {
        TimeLimits {
            request: Duration::from_millis(100),
            upload: Duration::from_secs(5),
            body_idle: Duration::from_secs(5),
        }
    }

    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_millis(400)).await;
        "done"
    }

    async fn status(app: &Router, method: Method, uri: &str) -> (StatusCode, String) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("infallible router");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn a_request_past_its_limit_answers_the_timeout_envelope() {
        let app = with_time_limits(Router::new().route("/api/v1/slow", get(slow)), limits());
        let (status, body) = status(&app, Method::GET, "/api/v1/slow").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.contains(r#""error":"timeout""#), "{body}");
    }

    #[tokio::test]
    async fn an_upload_gets_the_longer_limit() {
        let app = with_time_limits(
            Router::new()
                .route("/api/v1/trees/{tree_id}/import-jobs", post(slow))
                .route("/api/v1/trees/{tree_id}/media/upload", post(slow)),
            limits(),
        );
        for uri in [
            "/api/v1/trees/t/import-jobs",
            "/api/v1/trees/t/media/upload",
        ] {
            assert_eq!(
                status(&app, Method::POST, uri).await.0,
                StatusCode::OK,
                "{uri}"
            );
        }
    }
}
