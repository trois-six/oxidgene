use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::TagRef;

/// A textual note attached to a person, event, family, source, media, or
/// repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub text: String,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    /// The media this note is about — "the left-hand column is water-damaged".
    /// Distinct from `Media::description`, which is the caption under a tile.
    pub media_id: Option<Uuid>,
    /// The repository this note is about (GEDCOM `REPO.NOTE`).
    #[serde(default)]
    pub repository_id: Option<Uuid>,
    /// The shared note record (5.5.1 `NOTE`, 7.x `SNOTE`) this note was read
    /// from, the same for every note read from it; `None` for a note of its
    /// own. An export writes the notes of one group that say the same thing
    /// as one shared record again.
    #[serde(default)]
    pub shared_group: Option<Uuid>,
    /// The tags the note carries, sorted by folded label (docs/data-model.md, RecordTag).
    #[serde(default)]
    pub tags: Vec<TagRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
