//! Change history: the per-tree audit log and the versioned record snapshots.
//!
//! Every write to a tree leaves one [`AuditEntry`]. A write that changes a
//! versioned record — a person, a place, a source, or the tree's own settings —
//! also stores the state it replaced, so the record's successive states can be
//! compared field by field and an earlier one restored. The live record is
//! its current state: the history never stores a copy of it. A
//! [`RecordVersion`] is one of those states as the API presents it, the live
//! one included.
//!
//! Media are deliberately not versioned: their writes are audited, but a
//! photograph's bytes and its crops are not kept twice.
//!
//! A snapshot names the records it points at by ID only, so renaming a place
//! or a relative does not version everyone who refers to them. The display
//! labels of those IDs, as they read when the version was taken, travel beside
//! the snapshot in [`RecordVersion::labels`].

use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{
    Calendar, ChildType, Confidence, DateQualifier, EventType, NameType, Privacy, Sex,
    SourceMediaType, SpouseRole, TreeDefaultPrivacy,
};

/// Declares a string-backed history enum: snake_case on the wire and in the
/// database, with `as_str` and `FromStr` agreeing with serde.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal,)+ }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The stored and serialized form.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($text => Ok(Self::$variant),)+
                    other => Err(format!(concat!("unknown ", stringify!($name), ": {}"), other)),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum! {
    /// Which part of a tree a write touched. The audit log filters on it.
    pub enum AuditCategory {
        /// Genealogical data: persons, families, events, places, sources.
        Data => "data",
        /// The tree's own settings.
        Settings => "settings",
        /// Media, their pages, crops, tags, and links.
        Media => "media",
        /// A file or a Geneanet tree read into the tree.
        Import => "import",
        /// The tree written out to a file.
        Export => "export",
        /// A restored version.
        History => "history",
    }
}

string_enum! {
    /// What a write did.
    pub enum AuditAction {
        Create => "create",
        Update => "update",
        Delete => "delete",
        /// Two persons merged into one.
        Merge => "merge",
        Import => "import",
        Export => "export",
        /// A record put back as an earlier version had it.
        Revert => "revert",
    }
}

string_enum! {
    /// The kind of row a write changed.
    pub enum AuditEntity {
        Tree => "tree",
        Person => "person",
        PersonName => "person_name",
        /// Two same-named persons confirmed to be different people.
        PersonDistinct => "person_distinct",
        Family => "family",
        FamilySpouse => "family_spouse",
        FamilyChild => "family_child",
        Event => "event",
        EventWitness => "event_witness",
        Place => "place",
        Source => "source",
        /// A source's link to a repository holding it.
        SourceRepository => "source_repository",
        Repository => "repository",
        Citation => "citation",
        Note => "note",
        /// A surname's particle re-cut across every person bearing it.
        FamilyName => "family_name",
        Media => "media",
        MediaTag => "media_tag",
        MediaPage => "media_page",
        MediaLink => "media_link",
        Vignette => "vignette",
        Portrait => "portrait",
    }
}

string_enum! {
    /// The kind of record a write is about, for linking an audit entry to it.
    pub enum AuditSubject {
        Tree => "tree",
        Person => "person",
        Family => "family",
        Place => "place",
        Source => "source",
        Repository => "repository",
        Media => "media",
    }
}

string_enum! {
    /// The kinds of record whose successive states are kept.
    pub enum RecordType {
        /// A person with everything shown on their profile: names, events,
        /// notes, citations, parents, and unions.
        Person => "person",
        Place => "place",
        /// A source with its notes and the repositories holding it.
        Source => "source",
        /// A repository with its notes.
        Repository => "repository",
        /// The tree's settings.
        Tree => "tree",
    }
}

impl AuditCategory {
    /// The category a write falls under when nothing more specific applies.
    pub fn of(action: AuditAction, entity: AuditEntity) -> Self {
        match action {
            AuditAction::Import => return Self::Import,
            AuditAction::Export => return Self::Export,
            AuditAction::Revert => return Self::History,
            _ => {}
        }
        match entity {
            AuditEntity::Tree => Self::Settings,
            AuditEntity::Media
            | AuditEntity::MediaTag
            | AuditEntity::MediaPage
            | AuditEntity::MediaLink
            | AuditEntity::Vignette
            | AuditEntity::Portrait => Self::Media,
            _ => Self::Data,
        }
    }
}

/// Facts about a write beyond what it changed, each optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditDetails {
    /// File format of an import or export (`gedcom`, `gedzip`, `geneweb`,
    /// `geneanet`, `duplicate`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Name of the file imported, or of the other tree of a duplication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Number of persons an import brought or an export wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// Type of the event a write concerned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
    /// Version a revert restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    /// Display name of the other person of a merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_label: Option<String>,
    /// The name a family-name rename gave; the entry's label keeps the old
    /// one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_label: Option<String>,
}

impl AuditDetails {
    /// True when no detail is set.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// One write to a tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub category: AuditCategory,
    pub action: AuditAction,
    pub entity: AuditEntity,
    /// The row written, when the write had a single one.
    pub entity_id: Option<Uuid>,
    /// The record the write is about, for linking to it.
    pub subject: Option<AuditSubject>,
    pub subject_id: Option<Uuid>,
    /// The subject's display name when the write happened: it outlives a
    /// later rename or deletion.
    pub label: Option<String>,
    #[serde(default)]
    pub details: AuditDetails,
    /// Number of record states the write replaced, and so stored.
    #[serde(default)]
    pub version_count: i64,
}

/// The display label of a record a snapshot refers to by ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordLabel {
    pub id: Uuid,
    pub label: String,
}

/// One state of a versioned record: a past state the history stored, or the
/// live record as its current state.
///
/// Versions are numbered from 1, oldest first; the current one is numbered
/// after the last stored one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordVersion {
    /// The stored state's ID; the record's own ID for the current state,
    /// which is not stored.
    pub id: Uuid,
    pub tree_id: Uuid,
    pub record_type: RecordType,
    pub record_id: Uuid,
    pub version: i32,
    /// This is the live record, not a stored past state.
    pub current: bool,
    /// The record did not exist in this state: it was deleted.
    pub deleted: bool,
    /// The write that produced this state. For a record's first version, the
    /// write that created it, or the import that brought it; `None` when
    /// neither was recorded.
    pub entry: Option<AuditEntry>,
    /// The record's state; `None` when it was deleted.
    pub snapshot: Option<RecordSnapshot>,
    /// Labels of the places, sources, persons, and families the snapshot
    /// names, as they read when the state was stored — as they read now for
    /// the current state.
    #[serde(default)]
    pub labels: Vec<RecordLabel>,
}

impl RecordVersion {
    /// The label recorded for `id`, if any.
    pub fn label(&self, id: Uuid) -> Option<&str> {
        self.labels
            .iter()
            .find(|label| label.id == id)
            .map(|label| label.label.as_str())
    }
}

/// What one write did to a record: the state it replaced, beside the state
/// it produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionChange {
    /// The state the write produced — the current one when nothing changed
    /// the record since.
    pub version: RecordVersion,
    /// The state the write replaced.
    pub previous: RecordVersion,
}

/// The state of a versioned record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordSnapshot {
    Person(PersonSnapshot),
    Place(PlaceSnapshot),
    Source(SourceSnapshot),
    Repository(RepositorySnapshot),
    Tree(TreeSnapshot),
}

impl RecordSnapshot {
    /// The kind of record this is a state of.
    pub fn record_type(&self) -> RecordType {
        match self {
            Self::Person(_) => RecordType::Person,
            Self::Place(_) => RecordType::Place,
            Self::Source(_) => RecordType::Source,
            Self::Repository(_) => RecordType::Repository,
            Self::Tree(_) => RecordType::Tree,
        }
    }
}

/// A person as their profile shows them, less their media.
///
/// Every collection is kept in a fixed order so that two snapshots of the same
/// state compare equal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonSnapshot {
    pub sex: Sex,
    pub privacy: Privacy,
    #[serde(default)]
    pub names: Vec<NameSnapshot>,
    /// The person's own events.
    #[serde(default)]
    pub events: Vec<EventSnapshot>,
    #[serde(default)]
    pub notes: Vec<NoteSnapshot>,
    #[serde(default)]
    pub citations: Vec<CitationSnapshot>,
    /// The families the person is a child of.
    #[serde(default)]
    pub parents: Vec<ChildLinkSnapshot>,
    /// The families the person is a spouse in, with what they hold.
    #[serde(default)]
    pub unions: Vec<UnionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameSnapshot {
    pub id: Uuid,
    pub name_type: NameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSnapshot {
    pub id: Uuid,
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<NaiveDate>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub cause: Option<String>,
    #[serde(default)]
    pub age: Option<String>,
    #[serde(default)]
    pub agency: Option<String>,
    pub place_id: Option<Uuid>,
    pub description: Option<String>,
    #[serde(default)]
    pub witnesses: Vec<WitnessSnapshot>,
    /// For a family event, the age it gives for each spouse.
    #[serde(default)]
    pub spouse_ages: Vec<SpouseAgeSnapshot>,
    #[serde(default)]
    pub notes: Vec<NoteSnapshot>,
    #[serde(default)]
    pub citations: Vec<CitationSnapshot>,
}

/// The age a family event gives for one spouse, keyed by the spouse's
/// membership of the family; `person_id` names who it was when recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpouseAgeSnapshot {
    pub id: Uuid,
    pub family_spouse_id: Uuid,
    pub person_id: Uuid,
    pub age: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WitnessSnapshot {
    pub id: Uuid,
    pub person_id: Uuid,
    pub relation: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteSnapshot {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CitationSnapshot {
    pub id: Uuid,
    pub source_id: Uuid,
    pub page: Option<String>,
    #[serde(default)]
    pub confidence: Option<Confidence>,
    pub text: Option<String>,
}

/// A child's membership of a family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChildLinkSnapshot {
    pub id: Uuid,
    pub family_id: Uuid,
    pub person_id: Uuid,
    pub child_type: ChildType,
    pub sort_order: i32,
}

/// A spouse's membership of a family.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpouseLinkSnapshot {
    pub id: Uuid,
    pub person_id: Uuid,
    pub role: SpouseRole,
    pub sort_order: i32,
}

/// A family the person is a spouse in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnionSnapshot {
    pub family_id: Uuid,
    pub privacy: Privacy,
    /// Every spouse of the family, the person included.
    #[serde(default)]
    pub spouses: Vec<SpouseLinkSnapshot>,
    #[serde(default)]
    pub children: Vec<ChildLinkSnapshot>,
    #[serde(default)]
    pub events: Vec<EventSnapshot>,
    #[serde(default)]
    pub notes: Vec<NoteSnapshot>,
    #[serde(default)]
    pub citations: Vec<CitationSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaceSnapshot {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    #[serde(default)]
    pub agency: Option<String>,
    #[serde(default)]
    pub notes: Vec<NoteSnapshot>,
    /// The repositories holding the source, in order.
    #[serde(default)]
    pub repositories: Vec<SourceRepositorySnapshot>,
}

/// A source's link to a repository, which the version's labels name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRepositorySnapshot {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub call_number: Option<String>,
    pub media_type: Option<SourceMediaType>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositorySnapshot {
    pub name: String,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    #[serde(default)]
    pub notes: Vec<NoteSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TreeSnapshot {
    pub name: String,
    pub description: Option<String>,
    pub default_privacy: TreeDefaultPrivacy,
    #[serde(default = "crate::types::enabled")]
    pub entry_suggestions: bool,
    pub sosa_root_person_id: Option<Uuid>,
    pub self_person_id: Option<Uuid>,
}

/// A snapshot item that keeps its identity across versions.
pub trait Keyed {
    fn key(&self) -> Uuid;
}

macro_rules! keyed_by {
    ($($ty:ty => $field:ident),+ $(,)?) => {
        $(impl Keyed for $ty {
            fn key(&self) -> Uuid {
                self.$field
            }
        })+
    };
}

keyed_by!(
    NameSnapshot => id,
    EventSnapshot => id,
    WitnessSnapshot => id,
    NoteSnapshot => id,
    CitationSnapshot => id,
    ChildLinkSnapshot => id,
    SpouseLinkSnapshot => id,
    UnionSnapshot => family_id,
);

/// Pair the items of two versions of a collection by identity, for a
/// side-by-side comparison.
///
/// Items of `after` come in their own order, each beside its counterpart in
/// `before` if it has one; items only `before` had are placed after the last
/// matched item that preceded them, so a removal shows where it used to be.
pub fn align<'a, T: Keyed>(before: &'a [T], after: &'a [T]) -> Vec<(Option<&'a T>, Option<&'a T>)> {
    let mut rows: Vec<(Option<&T>, Option<&T>)> = after
        .iter()
        .map(|item| {
            let previous = before.iter().find(|old| old.key() == item.key());
            (previous, Some(item))
        })
        .collect();
    for (index, old) in before.iter().enumerate() {
        if after.iter().any(|item| item.key() == old.key()) {
            continue;
        }
        // Insert after the row of the nearest earlier item that survived.
        let anchor = before[..index]
            .iter()
            .rev()
            .find_map(|earlier| {
                rows.iter()
                    .position(|(prev, _)| prev.is_some_and(|p| p.key() == earlier.key()))
            })
            .map_or(0, |position| position + 1);
        // Keep several removals in their original order.
        let mut at = anchor;
        while at < rows.len() && rows[at].1.is_none() {
            at += 1;
        }
        rows.insert(at, (Some(old), None));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(n: u128, text: &str) -> NoteSnapshot {
        NoteSnapshot {
            id: Uuid::from_u128(n),
            text: text.to_string(),
        }
    }

    fn keys(rows: &[(Option<&NoteSnapshot>, Option<&NoteSnapshot>)]) -> Vec<(u128, u128)> {
        rows.iter()
            .map(|(before, after)| {
                (
                    before.map_or(0, |n| n.id.as_u128()),
                    after.map_or(0, |n| n.id.as_u128()),
                )
            })
            .collect()
    }

    #[test]
    fn align_pairs_by_identity_and_keeps_removals_in_place() {
        let before = [note(1, "a"), note(2, "b"), note(3, "c")];
        let after = [note(1, "a"), note(3, "c2"), note(4, "d")];
        assert_eq!(
            keys(&align(&before, &after)),
            vec![(1, 1), (2, 0), (3, 3), (0, 4)]
        );
    }

    #[test]
    fn align_places_a_leading_removal_first() {
        let before = [note(1, "a"), note(2, "b")];
        let after = [note(2, "b")];
        assert_eq!(keys(&align(&before, &after)), vec![(1, 0), (2, 2)]);
    }

    #[test]
    fn align_keeps_consecutive_removals_in_order() {
        let before = [note(1, "a"), note(2, "b"), note(3, "c")];
        let after: [NoteSnapshot; 0] = [];
        assert_eq!(keys(&align(&before, &after)), vec![(1, 0), (2, 0), (3, 0)]);
    }

    #[test]
    fn string_enums_round_trip_through_their_stored_form() {
        for entity in AuditEntity::ALL {
            assert_eq!(entity.as_str().parse::<AuditEntity>(), Ok(*entity));
        }
        for action in AuditAction::ALL {
            assert_eq!(action.as_str().parse::<AuditAction>(), Ok(*action));
        }
        assert!("nonsense".parse::<RecordType>().is_err());
    }

    #[test]
    fn categories_follow_the_action_before_the_entity() {
        assert_eq!(
            AuditCategory::of(AuditAction::Update, AuditEntity::Tree),
            AuditCategory::Settings
        );
        assert_eq!(
            AuditCategory::of(AuditAction::Create, AuditEntity::Vignette),
            AuditCategory::Media
        );
        assert_eq!(
            AuditCategory::of(AuditAction::Revert, AuditEntity::Person),
            AuditCategory::History
        );
        assert_eq!(
            AuditCategory::of(AuditAction::Import, AuditEntity::Tree),
            AuditCategory::Import
        );
        assert_eq!(
            AuditCategory::of(AuditAction::Delete, AuditEntity::Event),
            AuditCategory::Data
        );
    }

    #[test]
    fn a_snapshot_serializes_with_its_record_type() {
        let snapshot = RecordSnapshot::Place(PlaceSnapshot {
            name: "Springfield".to_string(),
            latitude: None,
            longitude: None,
        });
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.starts_with(r#"{"type":"place""#));
        let back: RecordSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, snapshot);
        assert_eq!(back.record_type(), RecordType::Place);
    }
}
