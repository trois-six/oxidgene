use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A media file (image, PDF, video, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
