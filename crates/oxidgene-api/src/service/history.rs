//! Change history: recording writes, versioning records, and restoring them.
//!
//! Every handler that writes to a tree ends with a [`Change`], recorded on the
//! same transaction as the write, so the audit log can never disagree with the
//! data: a write that rolls back leaves no entry, and an entry never describes
//! a write that did not happen.
//!
//! A change names what it touched — a person, a family, an event, a place, a
//! source, the tree's settings — and recording it snapshots every versioned
//! record behind those names. A snapshot equal to the record's latest version
//! is dropped, so handlers can name generously: naming a relative whose own
//! record did not change costs a comparison, never a spurious version.
//!
//! See `docs/data-model.md` §5 and the audit and history routes of
//! `docs/api.md`.

use std::collections::{BTreeSet, HashSet};

use chrono::Utc;
use futures_util::future::BoxFuture;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{
    AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditEntry, AuditSubject,
    PersonSnapshot, RecordLabel, RecordSnapshot, RecordType,
};
use oxidgene_core::projection::SearchEntry;
use oxidgene_core::types::Note;
use oxidgene_db::entities::{event, family_spouse, media, place, source, tree};
use oxidgene_db::repo::{
    FamilyNameParticleUpdate, FamilyNameRename, HistoryRepo, NewRecordVersion, SnapshotRepo,
    SnapshotScope, TreeRepo, db_err, display_names,
};
use oxidgene_db::sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect,
    TransactionTrait,
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::profile::{ProfileService, invalidation};

/// One write to a tree, as the audit log will record it.
#[derive(Debug, Clone)]
#[must_use = "a change is only recorded by calling `record`"]
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
    tree: bool,
    whole_tree: bool,
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
            tree: false,
            whole_tree: false,
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
    pub fn event(mut self, event_id: Uuid) -> Self {
        self.events.insert(event_id);
        self
    }

    /// Version whatever a note or citation hangs off: a person, an event, a
    /// family, or a source.
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

    /// Version the tree's settings, and make the tree the subject if none is
    /// set yet.
    pub fn tree_settings(mut self) -> Self {
        self.tree = true;
        let tree_id = self.tree_id;
        self.default_subject(AuditSubject::Tree, tree_id)
    }

    /// A media is the subject. Media are audited, never versioned.
    pub fn media(self, media_id: Uuid) -> Self {
        self.default_subject(AuditSubject::Media, media_id)
    }

    /// Version every person, place, and source of the tree, and its settings:
    /// imports, and the baseline of data recorded before history existed.
    pub fn whole_tree(mut self) -> Self {
        self.whole_tree = true;
        self.tree_settings()
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

    /// Write the audit entry and the versions the change produced.
    ///
    /// Call it on the write's own transaction, after the write.
    ///
    /// Boxed: snapshotting is a deep chain of queries, and every GraphQL
    /// mutation awaiting it inline made the mutation root's future large
    /// enough to overflow a thread's stack in debug builds.
    pub fn record<'a, C>(self, db: &'a C) -> BoxFuture<'a, Result<AuditEntry, OxidGeneError>>
    where
        C: ConnectionTrait,
    {
        Box::pin(self.record_inner(db))
    }

    async fn record_inner(
        mut self,
        db: &impl ConnectionTrait,
    ) -> Result<AuditEntry, OxidGeneError> {
        // Events resolve to their owner.
        for event_id in std::mem::take(&mut self.events) {
            let Some(row) = event::Entity::find_by_id(event_id)
                .one(db)
                .await
                .map_err(db_err)?
            else {
                continue;
            };
            self.details.event_type.get_or_insert(row.event_type.into());
            if let Some(person_id) = row.person_id {
                self = self.person(person_id);
            } else if let Some(family_id) = row.family_id {
                self = self.family(family_id);
            }
        }
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

        let label = match self.label.take() {
            Some(label) => Some(label),
            None => self.subject_label(db).await?,
        };
        let mut entry = AuditEntry {
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
            details: self.details.clone(),
            version_count: 0,
        };
        HistoryRepo::insert_entry(db, &entry).await?;

        let ids = |set: &BTreeSet<Uuid>| set.iter().copied().collect::<Vec<_>>();
        let persons = ids(&self.persons);
        let places = ids(&self.places);
        let sources = ids(&self.sources);
        let tree = [self.tree_id];
        let scopes: [(RecordType, SnapshotScope<'_>); 4] = if self.whole_tree {
            [
                (RecordType::Person, SnapshotScope::Tree),
                (RecordType::Place, SnapshotScope::Tree),
                (RecordType::Source, SnapshotScope::Tree),
                (RecordType::Tree, SnapshotScope::Ids(&tree)),
            ]
        } else {
            [
                (RecordType::Person, SnapshotScope::Ids(&persons)),
                (RecordType::Place, SnapshotScope::Ids(&places)),
                (RecordType::Source, SnapshotScope::Ids(&sources)),
                (
                    RecordType::Tree,
                    SnapshotScope::Ids(if self.tree { &tree } else { &[] }),
                ),
            ]
        };
        for (record_type, scope) in scopes {
            entry.version_count += capture(db, &entry, record_type, scope).await? as i64;
        }
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

/// The change a note write makes: it versions what the note hangs off, and a
/// note about a media — a page's transcript — is media work.
pub fn note_change(tree_id: Uuid, action: AuditAction, note: &Note) -> Change {
    let change = Change::new(tree_id, action, AuditEntity::Note, note.id).owner(
        note.person_id,
        note.event_id,
        note.family_id,
        note.source_id,
    );
    match note.media_id {
        Some(media_id) => change.media(media_id).category(AuditCategory::Media),
        None => change,
    }
}

/// The change a surname re-cut makes: every person bearing the name, filed
/// under the name itself.
pub fn family_name_change(tree_id: Uuid, update: &FamilyNameParticleUpdate) -> Change {
    Change::new(tree_id, AuditAction::Update, AuditEntity::FamilyName, None)
        .persons(update.person_ids.iter().copied())
        .label(update.value.clone())
        .details(AuditDetails {
            count: Some(update.persons_updated as u64),
            ..AuditDetails::default()
        })
}

/// The change a family-name rename makes: every person renamed, filed under
/// the old name, with the new one beside it.
pub fn family_name_rename_change(tree_id: Uuid, rename: &FamilyNameRename) -> Change {
    Change::new(tree_id, AuditAction::Update, AuditEntity::FamilyName, None)
        .persons(rename.person_ids.iter().copied())
        .label(rename.value.clone())
        .details(AuditDetails {
            count: Some(rename.persons_updated as u64),
            new_label: Some(rename.new_value.clone()),
            ..AuditDetails::default()
        })
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

/// Store a new version of every record in `scope` whose state differs from
/// its latest version, and return how many were stored.
///
/// A named record that no longer exists at all gets a deleted version
/// repeating its last state, so its history still ends with its removal.
async fn capture(
    db: &impl ConnectionTrait,
    entry: &AuditEntry,
    record_type: RecordType,
    scope: SnapshotScope<'_>,
) -> Result<usize, OxidGeneError> {
    if matches!(scope, SnapshotScope::Ids(ids) if ids.is_empty()) {
        return Ok(0);
    }
    let built = SnapshotRepo::build(db, entry.tree_id, record_type, scope).await?;
    let mut wanted: Vec<Uuid> = built.iter().map(|b| b.record_id).collect();
    let missing: Vec<Uuid> = match scope {
        SnapshotScope::Ids(ids) => ids
            .iter()
            .copied()
            .filter(|id| !wanted.contains(id))
            .collect(),
        SnapshotScope::Tree => Vec::new(),
    };
    wanted.extend(&missing);
    let mut latest = HistoryRepo::latest_versions(db, record_type, &wanted).await?;

    let mut versions = Vec::new();
    for snapshot in built {
        let next = match latest.remove(&snapshot.record_id) {
            Some(last)
                if last.deleted == snapshot.deleted && last.snapshot == snapshot.snapshot =>
            {
                continue;
            }
            Some(last) => last.version + 1,
            None => 1,
        };
        versions.push(NewRecordVersion {
            record_id: snapshot.record_id,
            version: next,
            deleted: snapshot.deleted,
            snapshot: snapshot.snapshot,
            labels: snapshot.labels,
        });
    }
    for record_id in missing {
        if let Some(last) = latest.remove(&record_id).filter(|last| !last.deleted) {
            versions.push(NewRecordVersion {
                record_id,
                version: last.version + 1,
                deleted: true,
                snapshot: last.snapshot,
                labels: Vec::new(),
            });
        }
    }
    let count = versions.len();
    if count > 0 {
        HistoryRepo::insert_versions(db, entry, record_type, versions).await?;
    }
    Ok(count)
}

/// The import and export format recorded for a tree duplication.
pub const DUPLICATE_FORMAT: &str = "duplicate";

/// Record a completed import, versioning everything the tree now holds.
///
/// Imports write in several transactions of their own, so this one follows
/// them rather than joining them. `file_name` is the imported file's, or the
/// other tree's name for a duplication.
///
/// It is the last bulk write of every import and duplication — a version of
/// every record the tree now holds — so the planner statistics are refreshed
/// here, after it, rather than after the projections: refreshed earlier, they
/// would describe `record_version` and `audit_entry` without these rows, and
/// the history reads (the home page's recently modified persons among them)
/// would plan full scans on tables recorded as nearly empty.
pub async fn record_import(
    db: &DatabaseConnection,
    tree_id: Uuid,
    format: &str,
    file_name: Option<String>,
    persons: usize,
) -> Result<AuditEntry, OxidGeneError> {
    let txn = db.begin().await.map_err(db_err)?;
    let entry = Change::new(tree_id, AuditAction::Import, AuditEntity::Tree, tree_id)
        .whole_tree()
        .details(AuditDetails {
            format: Some(format.to_string()),
            file_name,
            count: Some(persons as u64),
            ..AuditDetails::default()
        })
        .record(&txn)
        .await?;
    txn.commit().await.map_err(db_err)?;
    oxidgene_db::repo::refresh_statistics(db).await;
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
        .tree_settings()
        .details(AuditDetails {
            format: Some(format.to_string()),
            file_name,
            ..AuditDetails::default()
        })
        .record(db)
        .await
}

/// Record the state of trees that hold data but no history yet.
///
/// History starts with this build; data written before it has no version to
/// compare a first edit against. Run once at startup, before requests are
/// served: each such tree gets one baseline entry versioning everything it
/// holds. A tree that already has an entry is left alone, so this is cheap on
/// every later start.
pub async fn record_baselines(db: &DatabaseConnection) -> Result<usize, OxidGeneError> {
    let trees: Vec<Uuid> = tree::Entity::find()
        .select_only()
        .column(tree::Column::Id)
        .filter(tree::Column::DeletedAt.is_null())
        .into_tuple()
        .all(db)
        .await
        .map_err(db_err)?;
    let mut recorded = 0;
    for tree_id in trees {
        if HistoryRepo::has_entries(db, tree_id).await? {
            continue;
        }
        let txn = db.begin().await.map_err(db_err)?;
        let entry = Change::new(tree_id, AuditAction::Baseline, AuditEntity::Tree, tree_id)
            .whole_tree()
            .record(&txn)
            .await?;
        txn.commit().await.map_err(db_err)?;
        info!(
            versions = entry.version_count,
            "Recorded the history baseline of a tree"
        );
        recorded += 1;
    }
    Ok(recorded)
}

/// [`record_baselines`], logging instead of failing: a tree without a
/// baseline still records every change made from now on.
///
/// Runs at startup, outside any request: its span is the root its database
/// calls hang from, rather than each call being a trace of its own.
#[tracing::instrument(name = "history.baselines", skip_all)]
pub async fn record_baselines_at_startup(db: &DatabaseConnection) {
    match record_baselines(db).await {
        // A baseline versions a whole tree, like an import.
        Ok(recorded) if recorded > 0 => oxidgene_db::repo::refresh_statistics(db).await,
        Ok(_) => {}
        Err(_) => warn!(
            error = "history_baseline",
            "Failed to record the history baseline"
        ),
    }
}

/// How many recently modified persons a read returns when not told.
pub const RECENT_PERSONS_DEFAULT_LIMIT: usize = 5;

/// The most recently modified persons a single read may return.
pub const RECENT_PERSONS_MAX_LIMIT: usize = 50;

/// The persons of a tree modified most recently, newest first, as search rows.
///
/// "Modified" is what the history says: a person is modified when a write
/// stored a new version of them. Imports and baselines are left out, and so
/// are persons deleted since; see [`HistoryRepo::recently_modified_persons`].
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

/// Put a record back as one of its versions had it.
///
/// Runs on the caller's transaction, refreshes the projections it reaches,
/// and records the restore as a change of its own — itself a new version, so
/// a restore can be undone like any other write.
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
    if target.deleted {
        return Err(OxidGeneError::Validation(
            "a deleted state cannot be restored; restore the version before it".to_string(),
        ));
    }
    let details = AuditDetails {
        version: Some(version),
        ..AuditDetails::default()
    };
    let change = Change::new(
        tree_id,
        AuditAction::Revert,
        entity_of(record_type),
        record_id,
    )
    .details(details);
    match &target.snapshot {
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
        RecordSnapshot::Place(snapshot) => {
            SnapshotRepo::restore_place(db, tree_id, record_id, snapshot).await?;
            let affected = invalidation::affected_persons_for_place(db, record_id).await?;
            profiles
                .invalidate_for_mutation(db, tree_id, &affected)
                .await?;
            change.place(record_id).record(db).await
        }
        RecordSnapshot::Source(snapshot) => {
            SnapshotRepo::restore_source(db, tree_id, record_id, snapshot).await?;
            change.source(record_id).record(db).await
        }
        RecordSnapshot::Tree(snapshot) => {
            if record_id != tree_id {
                return Err(OxidGeneError::NotFound {
                    entity: "Tree",
                    id: record_id,
                });
            }
            SnapshotRepo::restore_tree(db, tree_id, snapshot).await?;
            change.tree_settings().record(db).await
        }
    }
}

/// Puts a person back as `snapshot` had them, refreshes everyone linked to
/// them before or after the restore, and records `change` for them.
async fn revert_person(
    db: &impl ConnectionTrait,
    profiles: &ProfileService,
    tree_id: Uuid,
    person_id: Uuid,
    snapshot: &PersonSnapshot,
    labels: &[RecordLabel],
    change: Change,
) -> Result<AuditEntry, OxidGeneError> {
    let before = invalidation::affected_persons(db, person_id).await?;
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
    change.person(person_id).persons(affected).record(db).await
}

/// The audit entity a record type's restore is filed under.
fn entity_of(record_type: RecordType) -> AuditEntity {
    match record_type {
        RecordType::Person => AuditEntity::Person,
        RecordType::Place => AuditEntity::Place,
        RecordType::Source => AuditEntity::Source,
        RecordType::Tree => AuditEntity::Tree,
    }
}
