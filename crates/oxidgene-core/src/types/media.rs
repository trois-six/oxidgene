use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Calendar, DateQualifier, DocumentCategory, SourceMediaType};

/// A media file (image, PDF, video, etc.).
///
/// `PartialEq` so Dioxus props holding one can diff — a gallery tile is keyed
/// on the media it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    /// Path as it appears in GEDCOM (`OBJE.FILE`) — the producer's own path,
    /// preserved verbatim so an export round-trips. Not where our copy lives.
    pub file_path: String,
    /// Key of the stored bytes in the media store, or `None` when the record
    /// names a file we have never received — every GEDCOM-imported row starts
    /// that way.
    pub storage_key: Option<String>,
    /// Hex SHA-256 of the stored bytes. Doubles as the `ETag`.
    pub sha256: Option<String>,
    /// Key of the generated thumbnail. `None` for formats we cannot rasterise
    /// (PDFs) and for records with no bytes.
    pub thumbnail_key: Option<String>,
    /// Intrinsic pixel size, after applying any EXIF orientation.
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// Pages in the document; `1` for photos and single-page files. For a
    /// [`Media::is_document`] row it is the number of page images assembled
    /// into it.
    pub page_count: i32,
    /// The document this is a page of, if it is one. A page is a media in its
    /// own right — it has bytes, a thumbnail and crops — and only this field
    /// says it belongs to something larger.
    pub parent_media_id: Option<Uuid>,
    /// Zero-based position within that document.
    #[serde(default)]
    pub page_index: i32,
    /// `true` when this row *is* a multi-page document assembled from page
    /// images rather than a file of its own. Such a row carries the title,
    /// date, place, description and note that describe the document as a
    /// whole, and usually holds no bytes.
    #[serde(default)]
    pub is_document: bool,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Date the media was created or applies to — the same shape as an event's,
    /// down to the qualifier and calendar, so one date widget edits both.
    pub date_value: Option<String>,
    /// Normalized Gregorian date for sorting. Derived server-side from
    /// `calendar` + `date_value`; never accepted from a client.
    pub date_sort: Option<NaiveDate>,
    #[serde(default)]
    pub date_qualifier: DateQualifier,
    /// The second date of a range (`Between`, `From`/`To`).
    pub date_value2: Option<String>,
    #[serde(default)]
    pub calendar: Calendar,
    /// What the medium physically is, in GEDCOM's own vocabulary. Exported as
    /// `OBJE.FILE.FORM.TYPE` and read back from it, so this round-trips.
    #[serde(default)]
    pub source_media_type: SourceMediaType,
    /// What kind of *record* it is — the distinction GEDCOM's enumeration
    /// cannot draw, since a census return and a marriage contract are both
    /// `Manuscript` to it. `None` when unclassified, which a photograph
    /// ordinarily is.
    #[serde(default)]
    pub document_category: Option<DocumentCategory>,
    /// Location where the media was created or applies to.
    pub place_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A link between a media item and a person, event, source, or family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaLink {
    pub id: Uuid,
    pub media_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub sort_order: i32,
    /// `true` if this image is the linked person's profile photo.
    /// Only one `MediaLink` per person may have this set.
    pub is_profile: bool,
}

/// A rectangular region of a stored media file, kept as coordinates rather
/// than as a second copy of the pixels.
///
/// One parish-register page routinely documents several unrelated families.
/// Recording each entry as a rectangle on the single stored scan means the
/// scan is stored once, a better scan can replace it without orphaning
/// anything, and the crop can still be served as if it were its own image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vignette {
    pub id: Uuid,
    /// The media this is a region of.
    pub media_id: Uuid,
    /// Zero-based page of a multi-page document; `0` for a photo.
    pub page: i32,
    /// Crop rectangle, in the source image's own pixel coordinates.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub title: Option<String>,
    /// Who the region shows, if attributed.
    pub person_id: Option<Uuid>,
    /// The event this region is evidence for, if any.
    pub event_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Whether a media's `file_path` points at something on the web.
///
/// A media does not have to be a file we hold. A GEDCOM `OBJE.FILE` is
/// routinely a URL — an archive's viewer, a photograph on a family site — and
/// those are worth recording even though the bytes are somebody else's. Such a
/// record has no `storage_key` and never will; the browser fetches it directly
/// from the URL, which also means we never become a proxy for someone else's
/// bandwidth.
pub fn is_remote_url(file_path: &str) -> bool {
    let path = file_path.trim();
    path.starts_with("http://") || path.starts_with("https://")
}

/// Guess a MIME type from a file name, a URL, or a bare extension.
///
/// Content sniffing is not available here — a remote media exists precisely so
/// that we never fetch it, and a GEDCOM record names a file we do not have. The
/// extension is the only evidence there is. It decides one thing: whether a
/// viewer embeds the media or offers it as a download, so a wrong guess costs a
/// click, not a security property.
///
/// A bare extension is accepted because that is what GEDCOM's `OBJE.FILE.FORM`
/// actually carries — the 5.5.1 spec calls it the "multimedia format" and the
/// values in the wild are `jpeg`, `bmp`, `png`, not MIME types.
pub fn guess_mime(file_name: &str) -> Option<&'static str> {
    // Strip a query string and fragment first: Geneanet serves
    // `medium.jpg?t=1524948994`, which is a jpg.
    let path = file_name
        .split(['?', '#'])
        .next()
        .unwrap_or(file_name)
        .trim_end_matches('/');
    // `rsplit('.')` on a string with no dot yields the whole string, which is
    // what makes a bare "jpeg" resolve.
    let extension = path
        .rsplit(['.', '/', '\\'])
        .next()
        .filter(|e| !e.is_empty())?
        .to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "heic" | "heif" => "image/heic",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "zip" => "application/zip",
        _ => return None,
    })
}

/// Whether a string looks like a MIME type we can act on.
///
/// `application/octet-stream` answers `false`: it is the value a producer
/// writes when it has nothing to say, so treating it as an answer is how a
/// photograph ends up labelled "OCTET-STREAM" in a gallery while the very same
/// file renders fine in an `<img>` elsewhere.
fn is_informative_mime(mime: &str) -> bool {
    let mime = mime.trim();
    mime.contains('/') && !mime.eq_ignore_ascii_case("application/octet-stream")
}

/// The MIME type to believe for a media, given what its producer declared and
/// what its file is called.
///
/// Order of evidence: a real MIME type if one was declared; otherwise whatever
/// the declaration turns out to be an extension for (GEDCOM `FORM` says
/// `jpeg`); otherwise the file name or URL; and `application/octet-stream` only
/// when nothing says anything, which is then honest rather than a default
/// wearing an answer's clothes.
pub fn normalize_mime(declared: Option<&str>, file_name: &str) -> String {
    if let Some(declared) = declared.map(str::trim).filter(|d| !d.is_empty()) {
        if is_informative_mime(declared) {
            return declared.to_string();
        }
        // A bare `FORM jpeg` is information, just not in the shape claimed.
        if let Some(guessed) = guess_mime(declared) {
            return guessed.to_string();
        }
    }
    guess_mime(file_name)
        .unwrap_or("application/octet-stream")
        .to_string()
}

#[cfg(test)]
mod mime_tests {
    use super::*;

    #[test]
    fn a_url_is_recognised_as_remote_and_a_path_is_not() {
        assert!(is_remote_url("https://archives.example.org/scan/42.jpg"));
        assert!(is_remote_url("http://example.org/photo.png"));
        assert!(is_remote_url("  https://example.org/x.jpg  "));
        // What a GEDCOM more often carries: somebody else's local path.
        assert!(!is_remote_url("D:\\Photos\\grandpere.jpg"));
        assert!(!is_remote_url("media/photo.jpg"));
        assert!(!is_remote_url("ftp://example.org/x.jpg"));
        assert!(!is_remote_url(""));
    }

    #[test]
    fn a_mime_type_is_guessed_from_the_extension() {
        assert_eq!(guess_mime("scan.JPG"), Some("image/jpeg"));
        assert_eq!(guess_mime("acte.pdf"), Some("application/pdf"));
        assert_eq!(guess_mime("interview.mp4"), Some("video/mp4"));
        assert_eq!(guess_mime("recording.mp3"), Some("audio/mpeg"));
    }

    #[test]
    fn a_bare_extension_resolves_because_that_is_what_gedcom_form_carries() {
        assert_eq!(guess_mime("jpeg"), Some("image/jpeg"));
        assert_eq!(guess_mime("JPG"), Some("image/jpeg"));
        assert_eq!(guess_mime("bmp"), Some("image/bmp"));
    }

    #[test]
    fn a_query_string_does_not_hide_the_extension() {
        // Exactly the shape Geneanet writes into `OBJE.FILE`.
        assert_eq!(
            guess_mime("http://gw.geneanet.org/public/img/media/medium.jpg?t=1785419513"),
            Some("image/jpeg")
        );
        assert_eq!(
            guess_mime("http://gw.geneanet.org/public/img/media/medium.PNG?t=1524949083"),
            Some("image/png")
        );
        assert_eq!(
            guess_mime("https://example.org/a.png#top"),
            Some("image/png")
        );
    }

    #[test]
    fn a_dot_in_a_directory_does_not_become_the_extension() {
        // `rsplit` also on the separators, or `site.org/viewer` would resolve
        // its extension to "org/viewer".
        assert_eq!(guess_mime("https://site.org/viewer"), None);
        assert_eq!(guess_mime("archive.xyz"), None);
        assert_eq!(guess_mime(""), None);
    }

    #[test]
    fn a_declared_mime_type_is_believed() {
        assert_eq!(
            normalize_mime(Some("image/webp"), "photo.jpg"),
            "image/webp"
        );
    }

    #[test]
    fn octet_stream_is_treated_as_no_answer() {
        // Both real cases from the sample files: an exporter that wrote
        // `FORM application/octet-stream`, and one that wrote no FORM at all.
        assert_eq!(
            normalize_mime(
                Some("application/octet-stream"),
                "http://gw.geneanet.org/public/img/media/medium.jpg?t=1"
            ),
            "image/jpeg"
        );
        assert_eq!(
            normalize_mime(
                None,
                "http://gw.geneanet.org/public/img/media/medium.bmp?t=1"
            ),
            "image/bmp"
        );
    }

    #[test]
    fn a_gedcom_form_extension_is_read_as_one() {
        assert_eq!(normalize_mime(Some("jpeg"), "unknown"), "image/jpeg");
    }

    #[test]
    fn nothing_known_stays_honestly_unknown() {
        assert_eq!(
            normalize_mime(None, "https://example.org/viewer"),
            "application/octet-stream"
        );
        assert_eq!(normalize_mime(Some(""), ""), "application/octet-stream");
    }
}
