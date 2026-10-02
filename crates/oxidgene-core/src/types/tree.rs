use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::TreeDefaultPrivacy;

/// A genealogical tree (project).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub sosa_root_person_id: Option<Uuid>,
    /// The person in this tree who represents the current user.
    pub self_person_id: Option<Uuid>,
    /// What `Privacy::Default` resolves to for every person, couple and
    /// document in this tree. Enforced by nothing yet — see the roadmap.
    #[serde(default)]
    pub default_privacy: TreeDefaultPrivacy,
    /// Whether entry fields suggest values as the user types.
    #[serde(default = "enabled")]
    pub entry_suggestions: bool,
    /// Who the tree's GEDCOM exports say they are from (`SUBM.NAME`); when
    /// unset, the "Who am I?" person's name.
    #[serde(default)]
    pub submitter_name: Option<String>,
    /// The submitter's email (`SUBM.EMAIL`).
    #[serde(default)]
    pub submitter_email: Option<String>,
    /// The submitter's postal address, over several lines (`SUBM.ADDR`).
    #[serde(default)]
    pub submitter_address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A tree setting that starts on, including in payloads written before it
/// existed.
pub(crate) fn enabled() -> bool {
    true
}
