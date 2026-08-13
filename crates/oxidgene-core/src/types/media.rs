use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{Calendar, DateQualifier};

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
