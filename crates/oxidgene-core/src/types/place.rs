use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::TagRef;

/// A geographic place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// The tags the place carries, sorted by folded label (docs/data-model.md, RecordTag).
    #[serde(default)]
    pub tags: Vec<TagRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
