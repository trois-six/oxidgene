use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::EventType;

/// A genealogical event (birth, death, marriage, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub event_type: EventType,
    /// GEDCOM date phrase (free text, e.g. "ABT 1842", "BET 1800 AND 1810").
    pub date_value: Option<String>,
    /// Normalized date for sorting and filtering.
    pub date_sort: Option<NaiveDate>,
    pub place_id: Option<Uuid>,
    /// Set for individual events.
    pub person_id: Option<Uuid>,
    /// Set for family events.
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
