use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// Pages in the document; `1` for photos and single-page files.
    pub page_count: i32,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Date the media was created or applies to (free-text, same shape as event dates).
    pub date_value: Option<String>,
    /// Normalized date for sorting and filtering.
    pub date_sort: Option<NaiveDate>,
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
