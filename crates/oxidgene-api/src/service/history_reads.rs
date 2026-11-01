//! The audit log and the record versions as the viewer sees them
//! (docs/privacy.md §6.4, §7), REST's and GraphQL's alike.
//!
//! The owner's redaction is unrestricted: each read is then the
//! repository's own, statement for statement. For a restricted viewer
//! holding `history_read` — an Editor, who does not see the owner's
//! excluded persons —
//!
//! - an entry about a person the redaction hides — its subject, or a person
//!   it stored a state of — about a family it withholds, or about a place,
//!   a source, a repository or a medium only hidden records reach (§3.2), is
//!   left out of the log, and read directly, with its changes, is `404`;
//! - the versions of such a person, place, source or repository are `404`,
//!   and so is restoring one;
//! - every version left is redacted: a hidden relative, witness or spouse is
//!   shown under the identifier the redaction gives them, or not at all when
//!   absent, without a label, which the frontend renders as an unknown
//!   record; a withheld family's events, notes and citations go, and the
//!   labels only they named with them; confidential notes go for a viewer
//!   without `read_confidential`; a version made by an entry left out of the
//!   log does not name it.

use std::collections::HashSet;

use oxidgene_core::OxidGeneError;
use oxidgene_core::history::{
    AuditEntry, AuditSubject, EventSnapshot, PersonSnapshot, RecordSnapshot, RecordType,
    RecordVersion, UnionSnapshot, VersionChange,
};
use oxidgene_core::redaction::Redaction;
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{
    AuditFilter, DictionaryRepo, HiddenRecords, HistoryRepo, PaginationParams,
};
use sea_orm::DatabaseConnection;
use uuid::Uuid;

/// What a restricted viewer's log leaves out.
struct Hidden<'a> {
    redaction: &'a Redaction,
    /// The entries that stored a state of a hidden person.
    versioning: HashSet<Uuid>,
    /// The places, sources, repositories and media hidden with them.
    records: HiddenRecords,
}

impl<'a> Hidden<'a> {
    async fn load(
        db: &DatabaseConnection,
        redaction: &'a Redaction,
        tree_id: Uuid,
    ) -> Result<Self, OxidGeneError> {
        let persons: Vec<Uuid> = redaction.hidden_persons().collect();
        Ok(Self {
            redaction,
            versioning: HistoryRepo::entries_versioning_persons(db, tree_id, &persons).await?,
            records: DictionaryRepo::hidden_records(db, tree_id, redaction).await?,
        })
    }

    /// Whether the entry `id` about `subject` is left out.
    fn entry(&self, id: Uuid, subject: Option<AuditSubject>, subject_id: Option<Uuid>) -> bool {
        self.versioning.contains(&id)
            || about_hidden(self.redaction, subject, subject_id)
            || self.records.contains_subject(subject, subject_id)
    }
}

/// Whether a subject is a person the redaction hides or a family it
/// withholds.
fn about_hidden(
    redaction: &Redaction,
    subject: Option<AuditSubject>,
    subject_id: Option<Uuid>,
) -> bool {
    match (subject, subject_id) {
        (Some(AuditSubject::Person), Some(id)) => !redaction.shows(id),
        (Some(AuditSubject::Family), Some(id)) => redaction.family_withheld(id),
        _ => false,
    }
}

fn not_found(entry_id: Uuid) -> OxidGeneError {
    OxidGeneError::NotFound {
        entity: "AuditEntry",
        id: entry_id,
    }
}

/// The tree's audit log as the viewer sees it, newest first.
pub async fn list_entries(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    filter: AuditFilter,
    params: &PaginationParams,
) -> Result<Connection<AuditEntry>, OxidGeneError> {
    if redaction.is_unrestricted() {
        return HistoryRepo::list_entries(db, tree_id, filter, params).await;
    }
    let hidden = Hidden::load(db, redaction, tree_id).await?;
    let kept: Vec<Uuid> = HistoryRepo::entry_subjects(db, tree_id, filter)
        .await?
        .into_iter()
        .filter(|(id, subject, subject_id)| !hidden.entry(*id, *subject, *subject_id))
        .map(|(id, ..)| id)
        .collect();
    HistoryRepo::list_entries_among(db, tree_id, &kept, params).await
}

/// One audit entry, `404` when the viewer's log leaves it out.
pub async fn get_entry(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    entry_id: Uuid,
) -> Result<AuditEntry, OxidGeneError> {
    let entry = HistoryRepo::get_entry(db, tree_id, entry_id).await?;
    if redaction.is_unrestricted() {
        return Ok(entry);
    }
    let versioned = HistoryRepo::persons_versioned_by(db, tree_id, entry_id).await?;
    if about_hidden(redaction, entry.subject, entry.subject_id)
        || versioned.iter().any(|person| !redaction.shows(*person))
        || DictionaryRepo::hidden_records(db, tree_id, redaction)
            .await?
            .contains_subject(entry.subject, entry.subject_id)
    {
        return Err(not_found(entry_id));
    }
    Ok(entry)
}

/// The states one entry replaced, each beside the state it produced.
pub async fn list_entry_changes(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    entry_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<VersionChange>, OxidGeneError> {
    get_entry(db, redaction, tree_id, entry_id).await?;
    let mut changes = HistoryRepo::list_entry_changes(db, tree_id, entry_id, params).await?;
    if !redaction.is_unrestricted() {
        let hidden = Hidden::load(db, redaction, tree_id).await?;
        for edge in &mut changes.edges {
            redact_version(&hidden, &mut edge.node.version);
            redact_version(&hidden, &mut edge.node.previous);
        }
    }
    Ok(changes)
}

/// `404` for the versions of a person the viewer does not see in full, and
/// of a place, a source or a repository only hidden records reach.
fn require_record(
    hidden: &Hidden<'_>,
    record_type: RecordType,
    record_id: Uuid,
) -> Result<(), OxidGeneError> {
    let person = record_type == RecordType::Person && !hidden.redaction.shows(record_id);
    if person || hidden.records.contains(record_id) {
        return Err(OxidGeneError::NotFound {
            entity: "RecordVersion",
            id: record_id,
        });
    }
    Ok(())
}

/// A record's versions as the viewer sees them, latest first.
pub async fn list_versions(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
    params: &PaginationParams,
) -> Result<Connection<RecordVersion>, OxidGeneError> {
    if redaction.is_unrestricted() {
        return HistoryRepo::list_versions(db, tree_id, record_type, record_id, params).await;
    }
    let hidden = Hidden::load(db, redaction, tree_id).await?;
    require_record(&hidden, record_type, record_id)?;
    let mut versions =
        HistoryRepo::list_versions(db, tree_id, record_type, record_id, params).await?;
    for edge in &mut versions.edges {
        redact_version(&hidden, &mut edge.node);
    }
    Ok(versions)
}

/// One version of a record as the viewer sees it.
pub async fn get_version(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
    version: i32,
) -> Result<RecordVersion, OxidGeneError> {
    if redaction.is_unrestricted() {
        return HistoryRepo::get_version(db, tree_id, record_type, record_id, version).await;
    }
    let hidden = Hidden::load(db, redaction, tree_id).await?;
    require_record(&hidden, record_type, record_id)?;
    let mut found = HistoryRepo::get_version(db, tree_id, record_type, record_id, version).await?;
    redact_version(&hidden, &mut found);
    Ok(found)
}

/// Check that the viewer may restore a version of the record: not one of a
/// record they do not see. Nothing to read for the owner.
pub async fn require_restorable(
    db: &DatabaseConnection,
    redaction: &Redaction,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
) -> Result<(), OxidGeneError> {
    if redaction.is_unrestricted() {
        return Ok(());
    }
    let hidden = Hidden::load(db, redaction, tree_id).await?;
    require_record(&hidden, record_type, record_id)
}

// ── Redacting a version ───────────────────────────────────────────────────

/// `version` as the viewer sees it.
fn redact_version(hidden: &Hidden<'_>, version: &mut RecordVersion) {
    let redaction = hidden.redaction;
    if version
        .entry
        .as_ref()
        .is_some_and(|entry| hidden.entry(entry.id, entry.subject, entry.subject_id))
    {
        version.entry = None;
    }
    match version.snapshot.as_mut() {
        Some(RecordSnapshot::Person(person)) => redact_person(redaction, person),
        Some(RecordSnapshot::Source(source)) if redaction.withholds_confidential() => {
            source.notes.retain(|note| !note.confidential);
        }
        Some(RecordSnapshot::Repository(repository)) if redaction.withholds_confidential() => {
            repository.notes.retain(|note| !note.confidential);
        }
        Some(RecordSnapshot::Tree(tree)) => {
            tree.sosa_root_person_id = tree
                .sosa_root_person_id
                .and_then(|id| redaction.public_id(id));
            tree.self_person_id = tree.self_person_id.and_then(|id| redaction.public_id(id));
        }
        _ => {}
    }
    // A label names a record the version still names, and never a person
    // the viewer does not see in full.
    let named = named_records(version.snapshot.as_ref());
    version.labels.retain(|label| {
        named.contains(&label.id) && redaction.shows(label.id) && !hidden.records.contains(label.id)
    });
}

/// A person's state as the viewer sees it: the relatives, witnesses and
/// spouses they may not follow under the identifier the redaction gives
/// them, or gone; a withheld family's content gone; confidential notes gone
/// for a viewer without `read_confidential`.
fn redact_person(redaction: &Redaction, person: &mut PersonSnapshot) {
    let confidential = redaction.withholds_confidential();
    if confidential {
        person.notes.retain(|note| !note.confidential);
    }
    for event in &mut person.events {
        redact_event(redaction, event);
    }
    for union in &mut person.unions {
        redact_union(redaction, union);
    }
}

fn redact_union(redaction: &Redaction, union: &mut UnionSnapshot) {
    if redaction.family_withheld(union.family_id) {
        union.events.clear();
        union.notes.clear();
        union.citations.clear();
    } else if redaction.withholds_confidential() {
        union.notes.retain(|note| !note.confidential);
    }
    union
        .spouses
        .retain_mut(|spouse| match redaction.public_id(spouse.person_id) {
            Some(id) => {
                spouse.person_id = id;
                true
            }
            None => false,
        });
    union
        .children
        .retain_mut(|child| match redaction.public_id(child.person_id) {
            Some(id) => {
                child.person_id = id;
                true
            }
            None => false,
        });
    for event in &mut union.events {
        redact_event(redaction, event);
    }
}

fn redact_event(redaction: &Redaction, event: &mut EventSnapshot) {
    if redaction.withholds_confidential() {
        event.notes.retain(|note| !note.confidential);
    }
    event
        .witnesses
        .retain_mut(|witness| match redaction.public_id(witness.person_id) {
            Some(id) => {
                witness.person_id = id;
                true
            }
            None => false,
        });
    event
        .spouse_ages
        .retain_mut(|age| match redaction.public_id(age.person_id) {
            Some(id) => {
                age.person_id = id;
                true
            }
            None => false,
        });
}

/// Every place, source, repository and person a snapshot names: what its
/// labels may name.
fn named_records(snapshot: Option<&RecordSnapshot>) -> HashSet<Uuid> {
    let mut named = HashSet::new();
    match snapshot {
        Some(RecordSnapshot::Person(person)) => {
            let mut events: Vec<&EventSnapshot> = person.events.iter().collect();
            let mut citations: Vec<_> = person.citations.iter().collect();
            for union in &person.unions {
                events.extend(&union.events);
                citations.extend(&union.citations);
                named.extend(union.spouses.iter().map(|s| s.person_id));
                named.extend(union.children.iter().map(|c| c.person_id));
            }
            for event in events {
                citations.extend(&event.citations);
                named.extend(event.place_id);
                named.extend(event.witnesses.iter().map(|w| w.person_id));
                named.extend(event.spouse_ages.iter().map(|a| a.person_id));
            }
            named.extend(citations.iter().map(|c| c.source_id));
        }
        Some(RecordSnapshot::Source(source)) => {
            named.extend(source.repositories.iter().map(|r| r.repository_id));
        }
        Some(RecordSnapshot::Tree(tree)) => {
            named.extend(tree.sosa_root_person_id);
            named.extend(tree.self_person_id);
        }
        _ => {}
    }
    named
}
