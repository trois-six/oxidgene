use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::TagRef;

/// A bibliographic source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    /// The organisation responsible for the source's data (GEDCOM
    /// `SOUR.DATA.AGNC`).
    #[serde(default)]
    pub agency: Option<String>,
    /// The tags the source carries, sorted by folded label (docs/data-model.md, RecordTag).
    #[serde(default)]
    pub tags: Vec<TagRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
