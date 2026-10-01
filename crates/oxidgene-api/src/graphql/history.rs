//! GraphQL types of the audit log and the record versions, mirroring the
//! REST payloads of `rest::history` field for field.

use async_graphql::{Enum, ID, SimpleObject};
use chrono::{DateTime, NaiveDate, Utc};
use oxidgene_core::history::{
    AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditEntry, AuditSubject,
    ChildLinkSnapshot, CitationSnapshot, EventSnapshot, NameSnapshot, NoteSnapshot, PersonSnapshot,
    PlaceSnapshot, RecordLabel, RecordSnapshot, RecordType, RecordVersion, SourceSnapshot,
    SpouseLinkSnapshot, TreeSnapshot, UnionSnapshot, VersionChange, WitnessSnapshot,
};

use super::types::{
    GqlCalendar, GqlChildType, GqlConfidence, GqlDateQualifier, GqlEventType, GqlNameType,
    GqlPrivacy, GqlSex, GqlSpouseRole, GqlTreeDefaultPrivacy,
};

/// Declares a GraphQL enum mirroring a history enum, with conversions both
/// ways.
macro_rules! mirror_enum {
    ($(#[$meta:meta])* $gql:ident => $core:ident { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
        pub enum $gql {
            $($variant,)+
        }

        impl From<$core> for $gql {
            fn from(value: $core) -> Self {
                match value {
                    $($core::$variant => Self::$variant,)+
                }
            }
        }

        impl From<$gql> for $core {
            fn from(value: $gql) -> Self {
                match value {
                    $($gql::$variant => Self::$variant,)+
                }
            }
        }
    };
}

mirror_enum!(
    /// Which part of a tree a write touched.
    GqlAuditCategory => AuditCategory { Data, Settings, Media, Import, Export, History }
);
mirror_enum!(
    /// What a write did.
    GqlAuditAction => AuditAction { Create, Update, Delete, Merge, Import, Export, Revert, Baseline }
);
mirror_enum!(
    /// The kind of row a write changed.
    GqlAuditEntity => AuditEntity {
        Tree, Person, PersonName, PersonDistinct, Family, FamilySpouse, FamilyChild, Event,
        EventWitness, Place, Source, Citation, Note, FamilyName, Media, MediaTag, MediaPage,
        MediaLink, Vignette, Portrait,
    }
);
mirror_enum!(
    /// The kind of record a write is about.
    GqlAuditSubject => AuditSubject { Tree, Person, Family, Place, Source, Media }
);
mirror_enum!(
    /// The kinds of record whose versions are kept.
    GqlRecordType => RecordType { Person, Place, Source, Tree }
);

fn id(uuid: uuid::Uuid) -> ID {
    ID(uuid.to_string())
}

/// Facts about a write beyond what it changed.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAuditDetails {
    pub format: Option<String>,
    pub file_name: Option<String>,
    pub count: Option<u64>,
    pub event_type: Option<GqlEventType>,
    pub version: Option<i32>,
    pub other_label: Option<String>,
    pub new_label: Option<String>,
}

impl From<AuditDetails> for GqlAuditDetails {
    fn from(d: AuditDetails) -> Self {
        Self {
            format: d.format,
            file_name: d.file_name,
            count: d.count,
            event_type: d.event_type.map(Into::into),
            version: d.version,
            other_label: d.other_label,
            new_label: d.new_label,
        }
    }
}

/// One write to a tree.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlAuditEntry {
    pub id: ID,
    pub tree_id: ID,
    pub occurred_at: DateTime<Utc>,
    pub category: GqlAuditCategory,
    pub action: GqlAuditAction,
    pub entity: GqlAuditEntity,
    pub entity_id: Option<ID>,
    pub subject: Option<GqlAuditSubject>,
    pub subject_id: Option<ID>,
    pub label: Option<String>,
    pub details: GqlAuditDetails,
    pub version_count: i64,
}

impl From<AuditEntry> for GqlAuditEntry {
    fn from(e: AuditEntry) -> Self {
        Self {
            id: id(e.id),
            tree_id: id(e.tree_id),
            occurred_at: e.occurred_at,
            category: e.category.into(),
            action: e.action.into(),
            entity: e.entity.into(),
            entity_id: e.entity_id.map(id),
            subject: e.subject.map(Into::into),
            subject_id: e.subject_id.map(id),
            label: e.label,
            details: e.details.into(),
            version_count: e.version_count,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlRecordLabel {
    pub id: ID,
    pub label: String,
}

impl From<RecordLabel> for GqlRecordLabel {
    fn from(l: RecordLabel) -> Self {
        Self {
            id: id(l.id),
            label: l.label,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNameSnapshot {
    pub id: ID,
    pub name_type: GqlNameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub sort_order: i32,
}

impl From<NameSnapshot> for GqlNameSnapshot {
    fn from(n: NameSnapshot) -> Self {
        Self {
            id: id(n.id),
            name_type: n.name_type.into(),
            given_names: n.given_names,
            surname: n.surname,
            surname_prefix: n.surname_prefix,
            prefix: n.prefix,
            suffix: n.suffix,
            nickname: n.nickname,
            is_primary: n.is_primary,
            sort_order: n.sort_order,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlNoteSnapshot {
    pub id: ID,
    pub text: String,
}

impl From<NoteSnapshot> for GqlNoteSnapshot {
    fn from(n: NoteSnapshot) -> Self {
        Self {
            id: id(n.id),
            text: n.text,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlCitationSnapshot {
    pub id: ID,
    pub source_id: ID,
    pub page: Option<String>,
    pub confidence: GqlConfidence,
    pub text: Option<String>,
}

impl From<CitationSnapshot> for GqlCitationSnapshot {
    fn from(c: CitationSnapshot) -> Self {
        Self {
            id: id(c.id),
            source_id: id(c.source_id),
            page: c.page,
            confidence: c.confidence.into(),
            text: c.text,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlWitnessSnapshot {
    pub id: ID,
    pub person_id: ID,
    pub relation: Option<String>,
    pub sort_order: i32,
}

impl From<WitnessSnapshot> for GqlWitnessSnapshot {
    fn from(w: WitnessSnapshot) -> Self {
        Self {
            id: id(w.id),
            person_id: id(w.person_id),
            relation: w.relation,
            sort_order: w.sort_order,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlEventSnapshot {
    pub id: ID,
    pub event_type: GqlEventType,
    pub date_value: Option<String>,
    pub date_sort: Option<NaiveDate>,
    pub date_qualifier: GqlDateQualifier,
    pub date_value2: Option<String>,
    pub calendar: GqlCalendar,
    pub cause: Option<String>,
    pub place_id: Option<ID>,
    pub description: Option<String>,
    pub witnesses: Vec<GqlWitnessSnapshot>,
    pub notes: Vec<GqlNoteSnapshot>,
    pub citations: Vec<GqlCitationSnapshot>,
}

impl From<EventSnapshot> for GqlEventSnapshot {
    fn from(e: EventSnapshot) -> Self {
        Self {
            id: id(e.id),
            event_type: e.event_type.into(),
            date_value: e.date_value,
            date_sort: e.date_sort,
            date_qualifier: e.date_qualifier.into(),
            date_value2: e.date_value2,
            calendar: e.calendar.into(),
            cause: e.cause,
            place_id: e.place_id.map(id),
            description: e.description,
            witnesses: convert(e.witnesses),
            notes: convert(e.notes),
            citations: convert(e.citations),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlChildLinkSnapshot {
    pub id: ID,
    pub family_id: ID,
    pub person_id: ID,
    pub child_type: GqlChildType,
    pub sort_order: i32,
}

impl From<ChildLinkSnapshot> for GqlChildLinkSnapshot {
    fn from(c: ChildLinkSnapshot) -> Self {
        Self {
            id: id(c.id),
            family_id: id(c.family_id),
            person_id: id(c.person_id),
            child_type: c.child_type.into(),
            sort_order: c.sort_order,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSpouseLinkSnapshot {
    pub id: ID,
    pub person_id: ID,
    pub role: GqlSpouseRole,
    pub sort_order: i32,
}

impl From<SpouseLinkSnapshot> for GqlSpouseLinkSnapshot {
    fn from(s: SpouseLinkSnapshot) -> Self {
        Self {
            id: id(s.id),
            person_id: id(s.person_id),
            role: s.role.into(),
            sort_order: s.sort_order,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlUnionSnapshot {
    pub family_id: ID,
    pub privacy: GqlPrivacy,
    pub spouses: Vec<GqlSpouseLinkSnapshot>,
    pub children: Vec<GqlChildLinkSnapshot>,
    pub events: Vec<GqlEventSnapshot>,
    pub notes: Vec<GqlNoteSnapshot>,
    pub citations: Vec<GqlCitationSnapshot>,
}

impl From<UnionSnapshot> for GqlUnionSnapshot {
    fn from(u: UnionSnapshot) -> Self {
        Self {
            family_id: id(u.family_id),
            privacy: u.privacy.into(),
            spouses: convert(u.spouses),
            children: convert(u.children),
            events: convert(u.events),
            notes: convert(u.notes),
            citations: convert(u.citations),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPersonSnapshot {
    pub sex: GqlSex,
    pub privacy: GqlPrivacy,
    pub names: Vec<GqlNameSnapshot>,
    pub events: Vec<GqlEventSnapshot>,
    pub notes: Vec<GqlNoteSnapshot>,
    pub citations: Vec<GqlCitationSnapshot>,
    pub parents: Vec<GqlChildLinkSnapshot>,
    pub unions: Vec<GqlUnionSnapshot>,
}

impl From<PersonSnapshot> for GqlPersonSnapshot {
    fn from(p: PersonSnapshot) -> Self {
        Self {
            sex: p.sex.into(),
            privacy: p.privacy.into(),
            names: convert(p.names),
            events: convert(p.events),
            notes: convert(p.notes),
            citations: convert(p.citations),
            parents: convert(p.parents),
            unions: convert(p.unions),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlPlaceSnapshot {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl From<PlaceSnapshot> for GqlPlaceSnapshot {
    fn from(p: PlaceSnapshot) -> Self {
        Self {
            name: p.name,
            latitude: p.latitude,
            longitude: p.longitude,
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlSourceSnapshot {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
    pub notes: Vec<GqlNoteSnapshot>,
}

impl From<SourceSnapshot> for GqlSourceSnapshot {
    fn from(s: SourceSnapshot) -> Self {
        Self {
            title: s.title,
            author: s.author,
            publisher: s.publisher,
            abbreviation: s.abbreviation,
            repository_name: s.repository_name,
            notes: convert(s.notes),
        }
    }
}

#[derive(Debug, Clone, SimpleObject)]
pub struct GqlTreeSnapshot {
    pub name: String,
    pub description: Option<String>,
    pub default_privacy: GqlTreeDefaultPrivacy,
    pub entry_suggestions: bool,
    pub sosa_root_person_id: Option<ID>,
    pub self_person_id: Option<ID>,
}

impl From<TreeSnapshot> for GqlTreeSnapshot {
    fn from(t: TreeSnapshot) -> Self {
        Self {
            name: t.name,
            description: t.description,
            default_privacy: t.default_privacy.into(),
            entry_suggestions: t.entry_suggestions,
            sosa_root_person_id: t.sosa_root_person_id.map(id),
            self_person_id: t.self_person_id.map(id),
        }
    }
}

/// A record's state: exactly one field is set, the one `recordType` names.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlRecordSnapshot {
    pub record_type: GqlRecordType,
    pub person: Option<GqlPersonSnapshot>,
    pub place: Option<GqlPlaceSnapshot>,
    pub source: Option<GqlSourceSnapshot>,
    pub tree: Option<GqlTreeSnapshot>,
}

impl From<RecordSnapshot> for GqlRecordSnapshot {
    fn from(s: RecordSnapshot) -> Self {
        let mut snapshot = Self {
            record_type: s.record_type().into(),
            person: None,
            place: None,
            source: None,
            tree: None,
        };
        match s {
            RecordSnapshot::Person(p) => snapshot.person = Some(p.into()),
            RecordSnapshot::Place(p) => snapshot.place = Some(p.into()),
            RecordSnapshot::Source(p) => snapshot.source = Some(p.into()),
            RecordSnapshot::Tree(p) => snapshot.tree = Some(p.into()),
        }
        snapshot
    }
}

/// One stored state of a versioned record.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlRecordVersion {
    pub id: ID,
    pub tree_id: ID,
    pub record_type: GqlRecordType,
    pub record_id: ID,
    pub version: i32,
    pub deleted: bool,
    pub created_at: DateTime<Utc>,
    pub entry: GqlAuditEntry,
    pub snapshot: GqlRecordSnapshot,
    pub labels: Vec<GqlRecordLabel>,
}

impl From<RecordVersion> for GqlRecordVersion {
    fn from(v: RecordVersion) -> Self {
        Self {
            id: id(v.id),
            tree_id: id(v.tree_id),
            record_type: v.record_type.into(),
            record_id: id(v.record_id),
            version: v.version,
            deleted: v.deleted,
            created_at: v.created_at,
            entry: v.entry.into(),
            snapshot: v.snapshot.into(),
            labels: convert(v.labels),
        }
    }
}

/// A version beside the one it replaced.
#[derive(Debug, Clone, SimpleObject)]
pub struct GqlVersionChange {
    pub version: GqlRecordVersion,
    pub previous: Option<GqlRecordVersion>,
}

impl From<VersionChange> for GqlVersionChange {
    fn from(c: VersionChange) -> Self {
        Self {
            version: c.version.into(),
            previous: c.previous.map(Into::into),
        }
    }
}

connection!(
    GqlAuditEntryEdge,
    GqlAuditEntryConnection,
    GqlAuditEntry,
    AuditEntry
);
connection!(
    GqlRecordVersionEdge,
    GqlRecordVersionConnection,
    GqlRecordVersion,
    RecordVersion
);
connection!(
    GqlVersionChangeEdge,
    GqlVersionChangeConnection,
    GqlVersionChange,
    VersionChange
);

fn convert<T, U: From<T>>(items: Vec<T>) -> Vec<U> {
    items.into_iter().map(Into::into).collect()
}
