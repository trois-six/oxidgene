use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::Confidence;

/// A citation linking a source to a person, event, or family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub id: Uuid,
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    /// Page or location within the source.
    pub page: Option<String>,
    pub confidence: Confidence,
    /// Extracted text from the source.
    pub text: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
