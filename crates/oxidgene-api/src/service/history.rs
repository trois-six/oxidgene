//! Change history: recording writes, versioning records, and restoring them.
//!
//! Every write to a tree records a [`Change`] on the same transaction as the
//! write, so the audit log can never disagree with the data: a write that
//! rolls back leaves no entry, and an entry never describes a write that did
//! not happen.
//!
//! The history stores the states writes replace, never the live one: a
//! record's current state is its live rows. A change names what the write
//! will touch — a person, a family, an event, a place, a source, the tree's
//! settings — and is recorded in two steps around the write:
//!
//! ```text
//! let pending = Change::update(tree_id, AuditEntity::Person, id)
//!     .person(id)
//!     .prepare(&txn)
//!     .await?;
//! // … the write …
//! pending.record(&txn).await?;
//! ```
//!
//! [`Change::prepare`] reads the named records as they are before the write —
//! families resolved to their spouses and events to their owners while the
//! links are still the old ones. [`PendingChange::record`] writes the audit
//! entry, reads the records again, and stores the state of each one the
//! write changed. A record the write left as it was stores nothing, so
//! services can name generously; a record the write created had no prior
//! state and stores nothing either; a record the write soft-deleted stores a
//! marker, its state being what the soft-deleted rows still hold.
//!
//! Writes that touch no versioned record — media, imports, exports — record
//! their entry alone with [`Change::record_unversioned`].
//!
//! See `docs/data-model.md` §5 and the audit and history routes of
//! `docs/api.md`.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::Utc;
use futures_util::future::BoxFuture;
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{
    AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditEntry, AuditSubject,
    PersonSnapshot, RecordLabel, RecordSnapshot, RecordType,
};
use oxidgene_core::projection::SearchEntry;
use oxidgene_core::types::Note;
use oxidgene_db::entities::{
    event, family_child, family_spouse, media, place, repository, source, tree,
};
use oxidgene_db::repo::{
    BuiltSnapshot, FamilyNameParticleUpdate, FamilyNameRename, HistoryRepo, NewRecordVersion,
    SnapshotRepo, TreeRepo, VersionHead, db_err, display_names,
};
use oxidgene_db::sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};

/// The versioned record types, in the order a change stores them.
const RECORD_TYPES: [RecordType; 5] = [
    RecordType::Person,
    RecordType::Place,
    RecordType::Source,
    RecordType::Repository,
    RecordType::Tree,
];

/// One write to a tree, as the audit log will record it.
#[derive(Debug, Clone)]
#[must_use = "a change is only recorded by `prepare` then `record`, or `record_unversioned`"]
pub struct Change {
    tree_id: Uuid,
    action: AuditAction,
    entity: AuditEntity,
    entity_id: Option<Uuid>,
    category: Option<AuditCategory>,
    subject: Option<(AuditSubject, Uuid)>,
    label: Option<String>,
    details: AuditDetails,
    persons: BTreeSet<Uuid>,
    families: BTreeSet<Uuid>,
    events: BTreeSet<Uuid>,
    places: BTreeSet<Uuid>,
    sources: BTreeSet<Uuid>,
    repositories: BTreeSet<Uuid>,
    tree: bool,
}

impl Change {
    /// A write of `action` on a row of kind `entity`.
    pub fn new(
        tree_id: Uuid,
        action: AuditAction,
        entity: AuditEntity,
        entity_id: impl Into<Option<Uuid>>,
    ) -> Self {
        Self {
            tree_id,
            action,
            entity,
            entity_id: entity_id.into(),
            category: None,
            subject: None,
            label: None,
            details: AuditDetails::default(),
            persons: BTreeSet::new(),
            families: BTreeSet::new(),
            events: BTreeSet::new(),
            places: BTreeSet::new(),
            sources: BTreeSet::new(),
            repositories: BTreeSet::new(),
            tree: false,
        }
    }

    pub fn create(tree_id: Uuid, entity: AuditEntity, id: Uuid) -> Self {
        Self::new(tree_id, AuditAction::Create, entity, id)
    }

    pub fn update(tree_id: Uuid, entity: AuditEntity, id: Uuid) -> Self {
        Self::new(tree_id, AuditAction::Update, entity, id)
    }

    pub fn delete(tree_id: Uuid, entity: AuditEntity, id: Uuid) -> Self {
        Self::new(tree_id, AuditAction::Delete, entity, id)
    }

    /// Version a person, and make them the subject if none is set yet.
    pub fn person(mut self, person_id: Uuid) -> Self {
        self.persons.insert(person_id);
        self.default_subject(AuditSubject::Person, person_id)
    }

    /// [`Self::person`], when there is one.
    pub fn person_if(self, person_id: Option<Uuid>) -> Self {
        match person_id {
            Some(id) => self.person(id),
            None => self,
        }
    }

    /// Version persons without making any of them the subject — the relatives
    /// a write reaches.
    pub fn persons(mut self, person_ids: impl IntoIterator<Item = Uuid>) -> Self {
        self.persons.extend(person_ids);
        self
    }

    /// Version the spouses of a family, and make it the subject if none is
    /// set yet.
    pub fn family(mut self, family_id: Uuid) -> Self {
        self.families.insert(family_id);
        self.default_subject(AuditSubject::Family, family_id)
    }

    /// Version whoever owns an event — its person, or its family's spouses —
    /// make the owner the subject if none is set yet, and note the event type.
    ///
    /// The owner is read before the write: an event the write creates has
    /// none yet, so its owner is named with [`Self::owner`] as well.
    pub fn event(mut self, event_id: Uuid) -> Self {
        self.events.insert(event_id);
        self
    }

    /// Version whatever a note, citation or event hangs off: a person, an
    /// event, a family, or a source.
    pub fn owner(
        mut self,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> Self {
        if let Some(id) = person_id {
            self = self.person(id);
        }
        if let Some(id) = event_id {
            self = self.event(id);
        }
        if let Some(id) = family_id {
            self = self.family(id);
        }
        if let Some(id) = source_id {
            self = self.source(id);
        }
        self
    }

    pub fn place(mut self, place_id: Uuid) -> Self {
        self.places.insert(place_id);
        self.default_subject(AuditSubject::Place, place_id)
    }

    pub fn source(mut self, source_id: Uuid) -> Self {
        self.sources.insert(source_id);
        self.default_subject(AuditSubject::Source, source_id)
    }

    pub fn repository(mut self, repository_id: Uuid) -> Self {
        self.repositories.insert(repository_id);
        self.default_subject(AuditSubject::Repository, repository_id)
    }

    /// Version the tree's settings, and make the tree the subject if none is
    /// set yet.
    pub fn tree_settings(mut self) -> Self {
        self.tree = true;
        self.about_tree()
    }

    /// The tree is the subject, nothing is versioned: imports and exports.
    pub fn about_tree(self) -> Self {
        let tree_id = self.tree_id;
        self.default_subject(AuditSubject::Tree, tree_id)
    }

    /// A media is the subject. Media are audited, never versioned.
    pub fn media(self, media_id: Uuid) -> Self {
        self.default_subject(AuditSubject::Media, media_id)
    }

    /// File the write under another category than its action and entity
    /// imply — a note about a media is media work, not genealogy.
    pub fn category(mut self, category: AuditCategory) -> Self {
        self.category = Some(category);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn details(mut self, details: AuditDetails) -> Self {
        self.details = details;
        self
    }

    fn default_subject(mut self, subject: AuditSubject, id: Uuid) -> Self {
        self.subject.get_or_insert((subject, id));
        self
    }

    /// Whether the change names any versioned record.
    fn versions_anything(&self) -> bool {
        self.tree
            || !self.persons.is_empty()
            || !self.families.is_empty()
            || !self.events.is_empty()
            || !self.places.is_empty()
            || !self.sources.is_empty()
            || !self.repositories.is_empty()
    }

    /// Read the named records as they are before the write.
    ///
    /// Call it on the write's own transaction, before the write; it writes
    /// nothing. Boxed: snapshotting is a deep chain of queries, and every
    /// GraphQL mutation awaiting it inline made the mutation root's future
    /// large enough to overflow a thread's stack in debug builds.
    pub fn prepare<'a, C>(self, db: &'a C) -> BoxFuture<'a, Result<PendingChange, OxidGeneError>>
    where
        C: ConnectionTrait,
    {
        Box::pin(self.prepare_inner(db))
    }

    async fn prepare_inner(
        mut self,
        db: &impl ConnectionTrait,
    ) -> Result<PendingChange, OxidGeneError> {
        self.resolve_events(db).await?;
        // Families resolve to their spouses, whose unions hold the family.
        let family_ids: Vec<Uuid> = self.families.iter().copied().collect();
        if !family_ids.is_empty() {
            let spouses: Vec<Uuid> = family_spouse::Entity::find()
                .select_only()
                .column(family_spouse::Column::PersonId)
                .filter(family_spouse::Column::FamilyId.is_in(family_ids))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?;
            self.persons.extend(spouses);
        }
        let mut before = Vec::with_capacity(RECORD_TYPES.len());
        for record_type in RECORD_TYPES {
            let ids = self.ids(record_type);
            let built = SnapshotRepo::build(db, self.tree_id, record_type, &ids).await?;
            let built = built.into_iter().map(|b| (b.record_id, b)).collect();
            before.push((record_type, ids, built));
        }
        Ok(PendingChange {
            change: self,
            before,
        })
    }

    /// Resolve the named events to their owners, as they are now. An event
    /// not found yet is kept, so that recording can still read its type.
    async fn resolve_events(&mut self, db: &impl ConnectionTrait) -> Result<(), OxidGeneError> {
        let mut unresolved = BTreeSet::new();
        for event_id in std::mem::take(&mut self.events) {
            let Some(row) = event::Entity::find_by_id(event_id)
                .one(db)
                .await
                .map_err(db_err)?
            else {
                unresolved.insert(event_id);
                continue;
            };
            self.details.event_type.get_or_insert(row.event_type.into());
            if let Some(person_id) = row.person_id {
                self.persons.insert(person_id);
                self.subject
                    .get_or_insert((AuditSubject::Person, person_id));
            } else if let Some(family_id) = row.family_id {
                self.families.insert(family_id);
                self.subject
                    .get_or_insert((AuditSubject::Family, family_id));
            }
        }
        self.events = unresolved;
        Ok(())
    }

    /// The IDs of the records of one type the change names.
    fn ids(&self, record_type: RecordType) -> Vec<Uuid> {
        match record_type {
            RecordType::Person => self.persons.iter().copied().collect(),
            RecordType::Place => self.places.iter().copied().collect(),
            RecordType::Source => self.sources.iter().copied().collect(),
            RecordType::Repository => self.repositories.iter().copied().collect(),
            RecordType::Tree if self.tree => vec![self.tree_id],
            RecordType::Tree => Vec::new(),
        }
    }

    /// Write the audit entry of a change that versions nothing: media work,
    /// imports, exports.
    ///
    /// A change naming a versioned record is refused: its prior states would
    /// be lost. Such a change goes through [`Self::prepare`].
    pub fn record_unversioned<'a, C>(
        self,
        db: &'a C,
    ) -> BoxFuture<'a, Result<AuditEntry, OxidGeneError>>
    where
        C: ConnectionTrait,
    {
        Box::pin(async move {
            if self.versions_anything() {
                return Err(OxidGeneError::Internal(
                    "a change naming versioned records must be prepared before its write"
                        .to_string(),
                ));
            }
            self.insert_entry(db).await
        })
    }

    /// Write the audit entry, labelled as the subject reads now.
    async fn insert_entry(
        mut self,
        db: &impl ConnectionTrait,
    ) -> Result<AuditEntry, OxidGeneError> {
        let label = match self.label.take() {
            Some(label) => Some(label),
            None => self.subject_label(db).await?,
        };
        let entry = AuditEntry {
            id: Uuid::now_v7(),
            tree_id: self.tree_id,
            occurred_at: Utc::now(),
            category: self
                .category
                .unwrap_or_else(|| AuditCategory::of(self.action, self.entity)),
            action: self.action,
            entity: self.entity,
            entity_id: self.entity_id,
            subject: self.subject.map(|(kind, _)| kind),
            subject_id: self.subject.map(|(_, id)| id),
            label,
            details: self.details,
            version_count: 0,
        };
        HistoryRepo::insert_entry(db, &entry).await?;
        Ok(entry)
    }

    /// The subject's display name as it reads now.
    async fn subject_label(
        &self,
        db: &impl ConnectionTrait,
    ) -> Result<Option<String>, OxidGeneError> {
        let Some((kind, id)) = self.subject else {
            return Ok(None);
        };
        Ok(match kind {
            AuditSubject::Person => display_names(db, &[id]).await?.remove(&id),
            AuditSubject::Family => family_label(db, id).await?,
            AuditSubject::Place => place::Entity::find_by_id(id)
                .one(db)
                .await
                .map_err(db_err)?
                .map(|row| row.name),
            AuditSubject::Source => source::Entity::find_by_id(id)
                .one(db)
                .await
                .map_err(db_err)?
                .map(|row| row.title),
            AuditSubject::Repository => repository::Entity::find_by_id(id)
                .one(db)
                .await
                .map_err(db_err)?
                .map(|row| row.name),
            AuditSubject::Tree => tree::Entity::find_by_id(id)
                .one(db)
                .await
                .map_err(db_err)?
                .map(|row| row.name),
            AuditSubject::Media => media::Entity::find_by_id(id)
                .one(db)
                .await
                .map_err(db_err)?
                .map(|row| {
                    row.title
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or(row.file_name)
                }),
        })
    }
}

/// A change whose records were read before the write, waiting for the write
/// to be done to be recorded.
#[derive(Debug)]
#[must_use = "a prepared change is only recorded by calling `record`"]
pub struct PendingChange {
    change: Change,
    /// Per record type: the IDs named, and the state of those that existed.
    before: Vec<(RecordType, Vec<Uuid>, HashMap<Uuid, BuiltSnapshot>)>,
}

impl PendingChange {
    /// Facts only the write could tell — how many names a repair rewrote.
    pub fn details(mut self, details: AuditDetails) -> Self {
        self.change.details = details;
        self
    }

    /// Write the audit entry, and store the state each named record had
    /// before the write when the write changed it.
    ///
    /// Call it on the write's own transaction, after the write. Boxed, as
    /// [`Change::prepare`] is.
    pub fn record<'a, C>(self, db: &'a C) -> BoxFuture<'a, Result<AuditEntry, OxidGeneError>>
    where
        C: ConnectionTrait,
    {
        Box::pin(self.record_inner(db))
    }

    async fn record_inner(self, db: &impl ConnectionTrait) -> Result<AuditEntry, OxidGeneError> {
        let Self { mut change, before } = self;
        // An event the write created: its type, and its owner as subject.
        change.resolve_events(db).await?;
        change.events.clear();
        let mut entry = change.insert_entry(db).await?;
        for (record_type, ids, before) in before {
            entry.version_count += store_replaced(db, &entry, record_type, &ids, &before).await?;
        }
        Ok(entry)
    }
}

/// What a write did to one record, judged from its state before and after.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// Nothing to store: unchanged, or created by the write.
    Nothing,
    /// Changed: store the state it had.
    Replaced(RecordSnapshot, Vec<RecordLabel>),
    /// Soft-deleted with its state intact: store a marker reading it from
    /// the soft-deleted rows.
    SoftDeleted,
    /// Brought back from deletion: store the deleted state, after giving the
    /// deletion's marker, if any, the state the rows held until now.
    Undeleted(Option<(RecordSnapshot, Vec<RecordLabel>)>),
    /// Changed while deleted: the marker, if any, must keep the state the
    /// rows held until now.
    ChangedWhileDeleted(RecordSnapshot, Vec<RecordLabel>),
}

/// Judge one record. `before` and `after` are its rows' states, absent when
/// it had none; `has_history` whether a state of it was ever stored.
fn outcome(
    before: Option<&BuiltSnapshot>,
    after: Option<&BuiltSnapshot>,
    has_history: bool,
) -> Outcome {
    let content = |b: &BuiltSnapshot| (b.snapshot.clone(), b.labels.clone());
    let Some(before) = before else {
        // Absent before: created by the write, or re-created after a removal.
        return match after {
            Some(after) if has_history && !after.deleted => Outcome::Undeleted(None),
            _ => Outcome::Nothing,
        };
    };
    let unchanged = after.is_some_and(|after| after.snapshot == before.snapshot);
    match (before.deleted, after.map(|after| after.deleted)) {
        (false, Some(false)) if unchanged => Outcome::Nothing,
        (false, Some(true)) if unchanged => Outcome::SoftDeleted,
        (false, _) => Outcome::Replaced(before.snapshot.clone(), before.labels.clone()),
        (true, Some(false)) => Outcome::Undeleted(Some(content(before))),
        (true, _) if unchanged => Outcome::Nothing,
        (true, _) => Outcome::ChangedWhileDeleted(before.snapshot.clone(), before.labels.clone()),
    }
}

/// Store the states the write replaced among `ids`, given their states
/// `before` it, and return how many were stored.
async fn store_replaced(
    db: &impl ConnectionTrait,
    entry: &AuditEntry,
    record_type: RecordType,
    ids: &[Uuid],
    before: &HashMap<Uuid, BuiltSnapshot>,
) -> Result<i64, OxidGeneError> {
    if ids.is_empty() {
        return Ok(0);
    }
    let heads = HistoryRepo::latest_heads(db, record_type, ids).await?;
    // A record that had no rows and no history was created by the write:
    // there is nothing to read again.
    let reread: Vec<Uuid> = ids
        .iter()
        .copied()
        .filter(|id| before.contains_key(id) || heads.contains_key(id))
        .collect();
    let after: HashMap<Uuid, BuiltSnapshot> =
        SnapshotRepo::build(db, entry.tree_id, record_type, &reread)
            .await?
            .into_iter()
            .map(|b| (b.record_id, b))
            .collect();
    let outcomes: Vec<(Uuid, Outcome)> = reread
        .iter()
        .map(|id| {
            let outcome = outcome(before.get(id), after.get(id), heads.contains_key(id));
            (*id, outcome)
        })
        .filter(|(_, outcome)| *outcome != Outcome::Nothing)
        .collect();

    // A state equal to the latest stored one is not stored twice.
    let compared: Vec<Uuid> = outcomes
        .iter()
        .filter(|(_, outcome)| matches!(outcome, Outcome::Replaced(..)))
        .filter_map(|(id, _)| heads.get(id))
        .filter(|head| !head.deleted && !head.empty)
        .map(|head| head.id)
        .collect();
    let stored = HistoryRepo::stored_snapshots(db, &compared).await?;

    let mut versions = Vec::new();
    for (record_id, outcome) in outcomes {
        let head = heads.get(&record_id);
        let next = head.map_or(1, |head| head.version + 1);
        let new = |deleted, content| NewRecordVersion {
            record_id,
            version: next,
            deleted,
            content,
        };
        match outcome {
            Outcome::Nothing => {}
            Outcome::Replaced(snapshot, labels) => {
                let repeated = head.and_then(|h| stored.get(&h.id)) == Some(&snapshot);
                if !repeated {
                    versions.push(new(false, Some((snapshot, labels))));
                }
            }
            Outcome::SoftDeleted => versions.push(new(false, None)),
            Outcome::Undeleted(content) => {
                fill_marker(db, head, content).await?;
                if !head.is_some_and(|head| head.deleted) {
                    versions.push(new(true, None));
                }
            }
            Outcome::ChangedWhileDeleted(snapshot, labels) => {
                fill_marker(db, head, Some((snapshot, labels))).await?;
            }
        }
    }
    let count = versions.len() as i64;
    if count > 0 {
        HistoryRepo::insert_versions(db, entry, record_type, versions).await?;
    }
    Ok(count)
}

/// Give a deletion marker the state its soft-deleted rows held, before the
/// write changed them. Anything else is left alone.
async fn fill_marker(
    db: &impl ConnectionTrait,
    head: Option<&VersionHead>,
    content: Option<(RecordSnapshot, Vec<RecordLabel>)>,
) -> Result<(), OxidGeneError> {
    match (head, content) {
        (Some(head), Some((snapshot, labels))) if head.is_marker() => {
            HistoryRepo::fill_marker(db, head.id, &snapshot, &labels).await
        }
        _ => Ok(()),
    }
}

/// What a note hangs off.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoteTarget {
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    pub media_id: Option<Uuid>,
    pub repository_id: Option<Uuid>,
}

impl From<&Note> for NoteTarget {
    fn from(note: &Note) -> Self {
        Self {
            person_id: note.person_id,
            event_id: note.event_id,
            family_id: note.family_id,
            source_id: note.source_id,
            media_id: note.media_id,
            repository_id: note.repository_id,
        }
    }
}

/// The change a note write makes: it versions what the note hangs off, and a
/// note about a media — a page's transcript — is media work.
pub fn note_change(
    tree_id: Uuid,
    action: AuditAction,
    note_id: Uuid,
    target: NoteTarget,
) -> Change {
    let mut change = Change::new(tree_id, action, AuditEntity::Note, note_id).owner(
        target.person_id,
        target.event_id,
        target.family_id,
        target.source_id,
    );
    if let Some(repository_id) = target.repository_id {
        change = change.repository(repository_id);
    }
    match target.media_id {
        Some(media_id) => change.media(media_id).category(AuditCategory::Media),
        None => change,
    }
}

/// The change a surname re-cut makes: every person bearing the name, filed
/// under the name itself. The count is the write's, given once it is done.
pub fn family_name_change(tree_id: Uuid, value: &str, person_ids: &[Uuid]) -> Change {
    Change::new(tree_id, AuditAction::Update, AuditEntity::FamilyName, None)
        .persons(person_ids.iter().copied())
        .label(value)
}

/// What a surname re-cut tells its audit entry.
pub fn family_name_details(update: &FamilyNameParticleUpdate) -> AuditDetails {
    AuditDetails {
        count: Some(update.persons_updated as u64),
        ..AuditDetails::default()
    }
}

/// What a family-name rename tells its audit entry: how many persons, and the
/// new name beside the old one, which labels the entry.
pub fn family_name_rename_details(rename: &FamilyNameRename) -> AuditDetails {
    AuditDetails {
        count: Some(rename.persons_updated as u64),
        new_label: Some(rename.new_value.clone()),
        ..AuditDetails::default()
    }
}

/// A family's label: its spouses' names.
async fn family_label(
    db: &impl ConnectionTrait,
    family_id: Uuid,
) -> Result<Option<String>, OxidGeneError> {
    let spouses: Vec<Uuid> = family_spouse::Entity::find()
        .select_only()
        .column(family_spouse::Column::PersonId)
        .filter(family_spouse::Column::FamilyId.eq(family_id))
        .into_tuple()
        .all(db)
        .await
        .map_err(db_err)?;
    let names = display_names(db, &spouses).await?;
    let label: Vec<&str> = spouses
        .iter()
        .filter_map(|id| names.get(id).map(String::as_str))
        .collect();
    Ok((!label.is_empty()).then(|| label.join(" & ")))
}

/// The import and export format recorded for a tree duplication.
pub const DUPLICATE_FORMAT: &str = "duplicate";

/// Record a completed import.
///
/// Imports write in several transactions of their own, so this one follows
/// them rather than joining them. It stores no version: what the import
/// brought is live, and a record nobody changed since has no past state.
/// `file_name` is the imported file's, or the other tree's name for a
/// duplication.
pub async fn record_import(
    db: &DatabaseConnection,
    tree_id: Uuid,
    format: &str,
    file_name: Option<String>,
    persons: usize,
) -> Result<AuditEntry, OxidGeneError> {
    let txn = db.begin().await.map_err(db_err)?;
    let entry = Change::new(tree_id, AuditAction::Import, AuditEntity::Tree, tree_id)
        .about_tree()
        .details(AuditDetails {
            format: Some(format.to_string()),
            file_name,
            count: Some(persons as u64),
            ..AuditDetails::default()
        })
        .record_unversioned(&txn)
        .await?;
    txn.commit().await.map_err(db_err)?;
    Ok(entry)
}

/// Record a completed export. An export writes nothing to the tree, but what
/// left it, and when, belongs in its audit log.
pub async fn record_export(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    format: &str,
    file_name: Option<String>,
) -> Result<AuditEntry, OxidGeneError> {
    Change::new(tree_id, AuditAction::Export, AuditEntity::Tree, tree_id)
        .about_tree()
        .details(AuditDetails {
            format: Some(format.to_string()),
            file_name,
            ..AuditDetails::default()
        })
        .record_unversioned(db)
        .await
}

/// How many recently modified persons a read returns when not told.
pub const RECENT_PERSONS_DEFAULT_LIMIT: usize = 5;

/// The most recently modified persons a single read may return.
pub const RECENT_PERSONS_MAX_LIMIT: usize = 50;

/// The persons of a tree modified most recently, newest first, as search rows.
///
/// "Modified" is what the audit log says: a person is modified when a write
/// about them was recorded. Imports are about the tree and are left out, and
/// so are persons deleted since; see [`HistoryRepo::recently_modified_persons`].
/// `limit` is capped at [`RECENT_PERSONS_MAX_LIMIT`].
pub async fn recently_modified_persons(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    limit: usize,
) -> Result<Vec<SearchEntry>, OxidGeneError> {
    TreeRepo::get(db, tree_id).await?;
    let person_ids = HistoryRepo::recently_modified_persons(
        db,
        tree_id,
        limit.min(RECENT_PERSONS_MAX_LIMIT) as u64,
    )
    .await?;
    profiles.search_entries(tree_id, &person_ids).await
}

/// The most trees one request may ask the recent persons of.
pub const MAX_RECENT_PERSON_TREES: usize = 64;

/// One tree's most recently modified persons, in a batch.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TreeRecentPersons {
    pub tree_id: Uuid,
    pub persons: Vec<SearchEntry>,
}

/// [`recently_modified_persons`] for several trees in one operation, for a
/// page that shows each tree's latest persons — the home page's cards. Order
/// is kept; a tree that is missing or deleted is left out rather than
/// failing the batch.
pub async fn recently_modified_persons_of_trees(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_ids: &[Uuid],
    limit: usize,
) -> Result<Vec<TreeRecentPersons>, OxidGeneError> {
    if tree_ids.len() > MAX_RECENT_PERSON_TREES {
        return Err(OxidGeneError::Validation(format!(
            "at most {MAX_RECENT_PERSON_TREES} trees can be read at once"
        )));
    }
    let mut out = Vec::with_capacity(tree_ids.len());
    for &tree_id in tree_ids {
        match recently_modified_persons(db, profiles, tree_id, limit).await {
            Ok(persons) => out.push(TreeRecentPersons { tree_id, persons }),
            Err(OxidGeneError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(out)
}

/// Put a record back as one of its versions had it.
///
/// Runs on the caller's transaction, refreshes the projections it reaches,
/// and records the restore as a change of its own: the state it replaces is
/// stored, so a restore can be undone like any other write. Restoring the
/// state a deletion replaced undeletes the record.
pub fn revert<'a, C>(
    db: &'a C,
    profiles: &'a ProfileService,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
    version: i32,
) -> BoxFuture<'a, Result<AuditEntry, OxidGeneError>>
where
    C: ConnectionTrait,
{
    Box::pin(revert_inner(
        db,
        profiles,
        tree_id,
        record_type,
        record_id,
        version,
    ))
}

async fn revert_inner(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
    version: i32,
) -> Result<AuditEntry, OxidGeneError> {
    let target = HistoryRepo::get_version(db, tree_id, record_type, record_id, version).await?;
    let Some(snapshot) = target.snapshot.filter(|_| !target.deleted) else {
        return Err(OxidGeneError::Validation(
            "a deleted state cannot be restored; restore the version before it".to_string(),
        ));
    };
    let change = Change::new(
        tree_id,
        AuditAction::Revert,
        entity_of(record_type),
        record_id,
    )
    .details(AuditDetails {
        version: Some(version),
        ..AuditDetails::default()
    });
    match &snapshot {
        RecordSnapshot::Person(snapshot) => {
            revert_person(
                db,
                profiles,
                tree_id,
                record_id,
                snapshot,
                &target.labels,
                change,
            )
            .await
        }
        other => revert_record(db, profiles, tree_id, record_id, other, change).await,
    }
}

/// Puts a place, a source, a repository or the tree's settings back as
/// `snapshot` had it, and records `change` for it.
async fn revert_record(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    record_id: Uuid,
    snapshot: &RecordSnapshot,
    change: Change,
) -> Result<AuditEntry, OxidGeneError> {
    match snapshot {
        RecordSnapshot::Person(_) => Err(OxidGeneError::Internal(
            "a person is restored by revert_person".to_string(),
        )),
        RecordSnapshot::Place(snapshot) => {
            revert_place(db, profiles, tree_id, record_id, snapshot, change).await
        }
        RecordSnapshot::Source(snapshot) => {
            let pending = change.source(record_id).prepare(db).await?;
            SnapshotRepo::restore_source(db, tree_id, record_id, snapshot).await?;
            pending.record(db).await
        }
        RecordSnapshot::Repository(snapshot) => {
            let pending = change.repository(record_id).prepare(db).await?;
            SnapshotRepo::restore_repository(db, tree_id, record_id, snapshot).await?;
            pending.record(db).await
        }
        RecordSnapshot::Tree(snapshot) => {
            if record_id != tree_id {
                return Err(OxidGeneError::NotFound {
                    entity: "Tree",
                    id: record_id,
                });
            }
            let pending = change.tree_settings().prepare(db).await?;
            SnapshotRepo::restore_tree(db, tree_id, snapshot).await?;
            pending.record(db).await
        }
    }
}

/// Puts a place back as `snapshot` had it, refreshes the persons whose
/// events name it, and records `change` for it.
async fn revert_place(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    place_id: Uuid,
    snapshot: &oxidgene_core::history::PlaceSnapshot,
    change: Change,
) -> Result<AuditEntry, OxidGeneError> {
    let pending = change.place(place_id).prepare(db).await?;
    SnapshotRepo::restore_place(db, tree_id, place_id, snapshot).await?;
    let affected = invalidation::affected_persons_for_place(db, place_id).await?;
    profiles
        .invalidate_for_mutation(db, tree_id, &affected)
        .await?;
    pending.record(db).await
}

/// Puts a person back as `snapshot` had them, refreshes everyone linked to
/// them before or after the restore, and records `change` for them.
///
/// Everyone the restore can change is named before it: the person's
/// relatives now, the relatives the snapshot links them to, the members of
/// every family it names — a family it undeletes changes all of them — and
/// the places and sources it may bring back.
async fn revert_person(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    snapshot: &PersonSnapshot,
    labels: &[RecordLabel],
    change: Change,
) -> Result<AuditEntry, OxidGeneError> {
    let reached = Reached::of(snapshot);
    let before = invalidation::affected_persons(db, person_id).await?;
    let members = family_members(db, &reached.families).await?;
    let mut change = change
        .person(person_id)
        .persons(before.iter().copied())
        .persons(reached.persons)
        .persons(members);
    change.places.extend(reached.places);
    change.sources.extend(reached.sources);
    let pending = change.prepare(db).await?;

    let touched = SnapshotRepo::restore_person(db, tree_id, person_id, snapshot, labels).await?;
    let after = invalidation::affected_persons(db, person_id).await?;
    let affected: Vec<Uuid> = before
        .into_iter()
        .chain(after)
        .chain(touched)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    profiles
        .invalidate_for_mutation(db, tree_id, &affected)
        .await?;
    pending.record(db).await
}

/// The records a person snapshot names, which restoring it can write.
struct Reached {
    persons: Vec<Uuid>,
    families: Vec<Uuid>,
    places: Vec<Uuid>,
    sources: Vec<Uuid>,
}

impl Reached {
    fn of(snapshot: &PersonSnapshot) -> Self {
        let events = snapshot
            .events
            .iter()
            .chain(snapshot.unions.iter().flat_map(|u| u.events.iter()));
        let citations = snapshot
            .citations
            .iter()
            .chain(snapshot.unions.iter().flat_map(|u| u.citations.iter()))
            .chain(events.clone().flat_map(|e| e.citations.iter()));
        Self {
            persons: sorted_unique(snapshot.unions.iter().flat_map(|u| {
                u.spouses
                    .iter()
                    .map(|s| s.person_id)
                    .chain(u.children.iter().map(|c| c.person_id))
            })),
            families: sorted_unique(
                snapshot
                    .parents
                    .iter()
                    .map(|p| p.family_id)
                    .chain(snapshot.unions.iter().map(|u| u.family_id)),
            ),
            places: sorted_unique(events.filter_map(|e| e.place_id)),
            sources: sorted_unique(citations.map(|c| c.source_id)),
        }
    }
}

/// Every spouse and child of these families, as the links stand now.
async fn family_members(
    db: &impl ConnectionTrait,
    family_ids: &[Uuid],
) -> Result<Vec<Uuid>, OxidGeneError> {
    if family_ids.is_empty() {
        return Ok(Vec::new());
    }
    let spouses: Vec<Uuid> = family_spouse::Entity::find()
        .select_only()
        .column(family_spouse::Column::PersonId)
        .filter(family_spouse::Column::FamilyId.is_in(family_ids.iter().copied()))
        .into_tuple()
        .all(db)
        .await
        .map_err(db_err)?;
    let children: Vec<Uuid> = family_child::Entity::find()
        .select_only()
        .column(family_child::Column::PersonId)
        .filter(family_child::Column::FamilyId.is_in(family_ids.iter().copied()))
        .into_tuple()
        .all(db)
        .await
        .map_err(db_err)?;
    Ok(sorted_unique(spouses.into_iter().chain(children)))
}

/// The audit entity a record type's restore is filed under.
fn entity_of(record_type: RecordType) -> AuditEntity {
    match record_type {
        RecordType::Person => AuditEntity::Person,
        RecordType::Place => AuditEntity::Place,
        RecordType::Source => AuditEntity::Source,
        RecordType::Repository => AuditEntity::Repository,
        RecordType::Tree => AuditEntity::Tree,
    }
}

#[cfg(test)]
mod tests {
    use oxidgene_core::history::PlaceSnapshot;

    use super::*;

    fn built(name: &str, deleted: bool) -> BuiltSnapshot {
        BuiltSnapshot {
            record_id: Uuid::from_u128(1),
            deleted,
            snapshot: RecordSnapshot::Place(PlaceSnapshot {
                name: name.to_string(),
                latitude: None,
                longitude: None,
            }),
            labels: Vec::new(),
        }
    }

    fn replaced(name: &str) -> Outcome {
        let b = built(name, false);
        Outcome::Replaced(b.snapshot, b.labels)
    }

    #[test]
    fn a_record_the_write_created_or_left_alone_stores_nothing() {
        let alpha = built("Alpha", false);
        assert_eq!(outcome(None, Some(&alpha), false), Outcome::Nothing);
        assert_eq!(outcome(Some(&alpha), Some(&alpha), true), Outcome::Nothing);
    }

    #[test]
    fn a_changed_or_removed_record_stores_its_prior_state() {
        let alpha = built("Alpha", false);
        let beta = built("Beta", false);
        assert_eq!(outcome(Some(&alpha), Some(&beta), false), replaced("Alpha"));
        assert_eq!(outcome(Some(&alpha), None, false), replaced("Alpha"));
        // Deleted and changed at once — a merged duplicate: the rows no
        // longer hold the prior state, which is stored.
        let beta_deleted = built("Beta", true);
        assert_eq!(
            outcome(Some(&alpha), Some(&beta_deleted), true),
            replaced("Alpha")
        );
    }

    #[test]
    fn a_soft_deletion_stores_a_marker() {
        let alpha = built("Alpha", false);
        let deleted = built("Alpha", true);
        assert_eq!(
            outcome(Some(&alpha), Some(&deleted), false),
            Outcome::SoftDeleted
        );
    }

    #[test]
    fn bringing_a_record_back_stores_its_deleted_state() {
        let deleted = built("Alpha", true);
        let alpha = built("Alpha", false);
        let b = built("Alpha", true);
        assert_eq!(
            outcome(Some(&deleted), Some(&alpha), true),
            Outcome::Undeleted(Some((b.snapshot, b.labels)))
        );
        // Re-created after a hard removal.
        assert_eq!(outcome(None, Some(&alpha), true), Outcome::Undeleted(None));
    }

    #[test]
    fn a_deleted_record_that_changes_keeps_its_marker_true() {
        let deleted = built("Alpha", true);
        let changed = built("Beta", true);
        assert!(matches!(
            outcome(Some(&deleted), Some(&changed), true),
            Outcome::ChangedWhileDeleted(..)
        ));
        assert!(matches!(
            outcome(Some(&deleted), None, true),
            Outcome::ChangedWhileDeleted(..)
        ));
        assert_eq!(
            outcome(Some(&deleted), Some(&deleted), true),
            Outcome::Nothing
        );
    }
}
