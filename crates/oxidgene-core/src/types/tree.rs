use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::TreeDefaultPrivacy;

/// A genealogical tree (project).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tree {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub sosa_root_person_id: Option<Uuid>,
    /// What `Privacy::Default` resolves to for every person, couple and
    /// document in this tree. Enforced by nothing yet — see the roadmap.
    #[serde(default)]
    pub default_privacy: TreeDefaultPrivacy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
