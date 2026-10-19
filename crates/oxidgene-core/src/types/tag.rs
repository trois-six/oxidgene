//! Tags: a tree's shared vocabulary of short labels, each with a colour, put
//! on records of every kind (docs/tags.md).

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Number of colour slots a tag can take, `0..TAG_COLOR_SLOTS`. The theme
/// declares one colour per slot (docs/ui-common.md §4.18).
pub const TAG_COLOR_SLOTS: i16 = 12;

/// Longest label a tag can have, in characters after trimming (docs/data-model.md, Tag).
pub const TAG_LABEL_MAX: usize = 80;

/// A tag as a record carries it: its identity, its spelling and its colour
/// slot. Reads of a taggable record return these, sorted by folded label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRef {
    pub id: Uuid,
    pub label: String,
    /// Index of the tag's colour, `0..TAG_COLOR_SLOTS`.
    pub color: i16,
}

/// The kinds of record a tag can be put on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Person,
    Family,
    Event,
    Place,
    Source,
    Citation,
    Repository,
    Media,
    Note,
}

impl RecordKind {
    /// Every kind, in the order of the `record_tag` columns.
    pub const ALL: [Self; 9] = [
        Self::Person,
        Self::Family,
        Self::Event,
        Self::Place,
        Self::Source,
        Self::Citation,
        Self::Repository,
        Self::Media,
        Self::Note,
    ];

    /// The stored and serialized form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Family => "family",
            Self::Event => "event",
            Self::Place => "place",
            Self::Source => "source",
            Self::Citation => "citation",
            Self::Repository => "repository",
            Self::Media => "media",
            Self::Note => "note",
        }
    }
}

impl FromStr for RecordKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unknown record kind: {value}"))
    }
}

impl std::fmt::Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_kind_strings_round_trip() {
        for kind in RecordKind::ALL {
            assert_eq!(kind.as_str().parse::<RecordKind>(), Ok(kind));
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.as_str())
            );
        }
        assert!("tag".parse::<RecordKind>().is_err());
    }
}
