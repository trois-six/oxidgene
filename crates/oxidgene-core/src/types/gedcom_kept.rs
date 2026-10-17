use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a GEDCOM file held for one entity that OxidGene's model has no
/// place for, kept to be written back on export (docs/api.md, "Kept GEDCOM").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GedcomKept {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub entity_kind: KeptEntityKind,
    /// The person, family, source, repository or medium; the tree's id for
    /// its submitter; for a record OxidGene has no entity for, an id of its
    /// own, which pointers to that record name.
    pub entity_id: Uuid,
    /// The kept nodes, as `oxidgene_gedcom::kept::to_json` writes them.
    pub nodes: String,
    pub created_at: DateTime<Utc>,
}

/// The kind of entity kept GEDCOM belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeptEntityKind {
    Person,
    Family,
    Source,
    Repository,
    Media,
    Submitter,
    Record,
}

impl KeptEntityKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Family => "family",
            Self::Source => "source",
            Self::Repository => "repository",
            Self::Media => "media",
            Self::Submitter => "submitter",
            Self::Record => "record",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "person" => Some(Self::Person),
            "family" => Some(Self::Family),
            "source" => Some(Self::Source),
            "repository" => Some(Self::Repository),
            "media" => Some(Self::Media),
            "submitter" => Some(Self::Submitter),
            "record" => Some(Self::Record),
            _ => None,
        }
    }
}
