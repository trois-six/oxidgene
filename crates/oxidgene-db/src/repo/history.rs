//! Repository for the audit log and the record versions.
//!
//! Storage only: which records a write versions, and what their snapshots
//! hold, is decided by the API's history service and [`super::SnapshotRepo`].
//!
//! `record_version` stores the states writes replaced, never the live one: a
//! record's current state is its live rows. A stored row is one of three
//! kinds:
//!
//! - a past state with its snapshot;
//! - a *deletion marker* — not deleted, no snapshot — the state a soft
//!   deletion replaced, which the soft-deleted rows still hold: it reads from
//!   them;
//! - a *deleted state* — deleted, no snapshot — the record as it was while
//!   deleted, which a restore replaced.
//!
//! Presented, a record's versions are its stored states, oldest first from
//! 1, and the live record as the last one; each carries the write that
//! produced it — the write that stored the version before it, or for the
//! first, the write that created the record or the import that brought it.
//!
//! The audit log reads newest first. A record's versions read from the latest
//! down, and the states one write replaced in the order it stored them.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use oxidgene_core::collections::sorted_unique;
use oxidgene_core::error::OxidGeneError;
use oxidgene_core::history::{
    AuditAction, AuditCategory, AuditDetails, AuditEntity, AuditEntry, RecordLabel, RecordSnapshot,
    RecordType, RecordVersion, VersionChange,
};
use oxidgene_core::types::{Connection, Edge, PageInfo};
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ConnectionTrait, FromQueryResult, Order, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Set,
};
use uuid::Uuid;

use super::batch::in_chunks;
use super::pagination::{PaginationParams, encode_cursor};
use super::{BuiltSnapshot, SnapshotRepo};
use crate::entities::{audit_entry, person, place, record_version, source, tree};
use crate::repo::db_err;

/// Filters of the audit log.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuditFilter {
    pub category: Option<AuditCategory>,
    /// Only the writes about this record.
    pub subject_id: Option<Uuid>,
}

/// A replaced state about to be stored.
#[derive(Debug, Clone)]
pub struct NewRecordVersion {
    pub record_id: Uuid,
    pub version: i32,
    /// The record was deleted in this state.
    pub deleted: bool,
    /// The state and the labels of what it names; `None` for a deleted state
    /// and for a deletion marker.
    pub content: Option<(RecordSnapshot, Vec<RecordLabel>)>,
}

/// A stored version without its snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromQueryResult)]
pub struct VersionHead {
    pub id: Uuid,
    pub record_id: Uuid,
    pub version: i32,
    pub deleted: bool,
    /// No snapshot is stored.
    pub empty: bool,
    pub audit_entry_id: Uuid,
}

impl VersionHead {
    /// The state a soft deletion replaced, read from the soft-deleted rows.
    pub fn is_marker(&self) -> bool {
        !self.deleted && self.empty
    }
}

/// Repository for `audit_entry` and `record_version`.
pub struct HistoryRepo;

impl HistoryRepo {
    /// Store an audit entry. Its `version_count` is not stored: it is counted
    /// from the versions on every read.
    pub async fn insert_entry(
        db: &impl ConnectionTrait,
        entry: &AuditEntry,
    ) -> Result<(), OxidGeneError> {
        let details = (!entry.details.is_empty())
            .then(|| serde_json::to_string(&entry.details))
            .transpose()
            .map_err(internal)?;
        audit_entry::ActiveModel {
            id: Set(entry.id),
            tree_id: Set(entry.tree_id),
            occurred_at: Set(entry.occurred_at),
            category: Set(entry.category.as_str().to_string()),
            action: Set(entry.action.as_str().to_string()),
            entity: Set(entry.entity.as_str().to_string()),
            entity_id: Set(entry.entity_id),
            subject: Set(entry.subject.map(|s| s.as_str().to_string())),
            subject_id: Set(entry.subject_id),
            label: Set(entry.label.clone()),
            details: Set(details),
        }
        .insert(db)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    /// Store the states one write replaced.
    pub async fn insert_versions(
        db: &impl ConnectionTrait,
        entry: &AuditEntry,
        record_type: RecordType,
        versions: Vec<NewRecordVersion>,
    ) -> Result<(), OxidGeneError> {
        // Bounded batches: every row binds nine values.
        const BATCH: usize = 500;
        let mut rows = Vec::with_capacity(versions.len().min(BATCH));
        for version in versions {
            let (snapshot, labels) = encode_content(version.content.as_ref())?;
            rows.push(record_version::ActiveModel {
                id: Set(Uuid::now_v7()),
                tree_id: Set(entry.tree_id),
                audit_entry_id: Set(entry.id),
                record_type: Set(record_type.as_str().to_string()),
                record_id: Set(version.record_id),
                version: Set(version.version),
                deleted: Set(version.deleted),
                snapshot: Set(snapshot),
                labels: Set(labels),
            });
            if rows.len() == BATCH {
                record_version::Entity::insert_many(std::mem::take(&mut rows))
                    .exec(db)
                    .await
                    .map_err(db_err)?;
            }
        }
        if !rows.is_empty() {
            record_version::Entity::insert_many(rows)
                .exec(db)
                .await
                .map_err(db_err)?;
        }
        Ok(())
    }

    /// Give a deletion marker the state it stands for, before a write changes
    /// the soft-deleted rows it reads from.
    pub async fn fill_marker(
        db: &impl ConnectionTrait,
        version_id: Uuid,
        snapshot: &RecordSnapshot,
        labels: &[RecordLabel],
    ) -> Result<(), OxidGeneError> {
        let (snapshot, labels) = encode_content(Some(&(snapshot.clone(), labels.to_vec())))?;
        record_version::Entity::update_many()
            .col_expr(record_version::Column::Snapshot, Expr::value(snapshot))
            .col_expr(record_version::Column::Labels, Expr::value(labels))
            .filter(record_version::Column::Id.eq(version_id))
            .exec(db)
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// The latest stored version of each of `record_ids` that has one.
    pub async fn latest_heads(
        db: &impl ConnectionTrait,
        record_type: RecordType,
        record_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, VersionHead>, OxidGeneError> {
        let rows = in_chunks(record_ids, |chunk| async move {
            heads_query()
                .filter(record_version::Column::RecordType.eq(record_type.as_str()))
                .filter(record_version::Column::RecordId.is_in(chunk))
                .filter(Expr::cust(
                    "record_version.version = (SELECT MAX(latest.version) \
                     FROM record_version latest \
                     WHERE latest.record_type = record_version.record_type \
                     AND latest.record_id = record_version.record_id)",
                ))
                .into_model::<VersionHead>()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        Ok(rows.into_iter().map(|row| (row.record_id, row)).collect())
    }

    /// The snapshots stored under these version IDs; versions storing none
    /// are absent.
    pub async fn stored_snapshots(
        db: &impl ConnectionTrait,
        version_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, RecordSnapshot>, OxidGeneError> {
        Ok(contents(db, version_ids)
            .await?
            .into_iter()
            .map(|(id, (snapshot, _))| (id, snapshot))
            .collect())
    }

    /// The newest snapshot stored for a record, if any — its last state with
    /// content, whatever was stored after it.
    pub async fn last_stored_snapshot(
        db: &impl ConnectionTrait,
        record_type: RecordType,
        record_id: Uuid,
    ) -> Result<Option<RecordSnapshot>, OxidGeneError> {
        let snapshot: Option<Option<String>> = record_version::Entity::find()
            .select_only()
            .column(record_version::Column::Snapshot)
            .filter(record_version::Column::RecordType.eq(record_type.as_str()))
            .filter(record_version::Column::RecordId.eq(record_id))
            .filter(record_version::Column::Snapshot.is_not_null())
            .order_by(record_version::Column::Version, Order::Desc)
            .into_tuple()
            .one(db)
            .await
            .map_err(db_err)?;
        snapshot
            .flatten()
            .as_deref()
            .map(parse_snapshot)
            .transpose()
    }

    /// The persons of a tree a write was about most recently, newest first.
    ///
    /// A person counts as modified when a recorded write names them as its
    /// subject: a change to their record, their names, events, notes,
    /// citations or unions, a merge into them, a restore. Imports and exports
    /// are about the tree, and a family-name repair about a name, so neither
    /// counts. Persons deleted since are left out.
    ///
    /// Read from the audit log's `(tree_id, subject_id, id)` index alone, and
    /// one live-person lookup per subject — which is also what keeps only
    /// persons, IDs being unique across tables: the history's stored states
    /// are never touched.
    pub async fn recently_modified_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        limit: u64,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        audit_entry::Entity::find()
            .select_only()
            .column(audit_entry::Column::SubjectId)
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .filter(audit_entry::Column::SubjectId.is_not_null())
            .filter(Expr::cust(
                "EXISTS (SELECT 1 FROM person \
                 WHERE person.id = audit_entry.subject_id \
                 AND person.deleted_at IS NULL)",
            ))
            .group_by(audit_entry::Column::SubjectId)
            // Entry IDs are UUID v7: the greatest is the latest write.
            .order_by(audit_entry::Column::Id.max(), Order::Desc)
            .limit(limit)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// When each of a tree's completed imports happened, with its details
    /// (format, file name, persons brought), oldest first. A duplication is
    /// the new tree's import.
    pub async fn imports(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(DateTime<Utc>, AuditDetails)>, OxidGeneError> {
        let rows: Vec<(DateTime<Utc>, Option<String>)> = audit_entry::Entity::find()
            .select_only()
            .column(audit_entry::Column::OccurredAt)
            .column(audit_entry::Column::Details)
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .filter(audit_entry::Column::Category.eq(AuditCategory::Import.as_str()))
            .filter(audit_entry::Column::Action.eq(AuditAction::Import.as_str()))
            .order_by(audit_entry::Column::Id, Order::Asc)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?;
        rows.into_iter()
            .map(|(occurred_at, details)| Ok((occurred_at, parse_details(details.as_deref())?)))
            .collect()
    }

    /// The spells a tree's persons spent deleted before a restore brought
    /// them back, as `(deleted, restored)` times.
    ///
    /// A restore clears `person.deleted_at`, so the person table alone would
    /// read as if a restored person had never been deleted. Only a person
    /// restore can bring a deleted person back, so the read starts from the
    /// tree's `revert` entries on persons — few — and reads those persons'
    /// version flags, never their snapshots: a deleted state is stored by the
    /// restore that ended it, and the version before it by the deletion.
    pub async fn person_restores(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(DateTime<Utc>, DateTime<Utc>)>, OxidGeneError> {
        let restored = sorted_unique(
            audit_entry::Entity::find()
                .select_only()
                .column(audit_entry::Column::EntityId)
                .filter(audit_entry::Column::TreeId.eq(tree_id))
                .filter(audit_entry::Column::Category.eq(AuditCategory::History.as_str()))
                .filter(audit_entry::Column::Action.eq(AuditAction::Revert.as_str()))
                .filter(audit_entry::Column::Entity.eq(AuditEntity::Person.as_str()))
                .filter(audit_entry::Column::EntityId.is_not_null())
                .into_tuple::<Uuid>()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        let mut rows: Vec<(Uuid, i32, bool, DateTime<Utc>)> =
            in_chunks(&restored, |chunk| async move {
                record_version::Entity::find()
                    .select_only()
                    .column(record_version::Column::RecordId)
                    .column(record_version::Column::Version)
                    .column(record_version::Column::Deleted)
                    .column(audit_entry::Column::OccurredAt)
                    .inner_join(audit_entry::Entity)
                    .filter(record_version::Column::RecordType.eq(RecordType::Person.as_str()))
                    .filter(record_version::Column::RecordId.is_in(chunk))
                    .into_tuple()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?;
        rows.sort_by_key(|(record_id, version, _, _)| (*record_id, *version));
        Ok(rows
            .windows(2)
            .filter(|pair| pair[0].0 == pair[1].0 && pair[1].2)
            .map(|pair| (pair[0].3, pair[1].3))
            .collect())
    }

    /// One audit entry of a tree.
    pub async fn get_entry(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entry_id: Uuid,
    ) -> Result<AuditEntry, OxidGeneError> {
        let row = audit_entry::Entity::find_by_id(entry_id)
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "AuditEntry",
                id: entry_id,
            })?;
        let mut entries = with_counts(db, vec![row]).await?;
        Ok(entries.remove(0))
    }

    /// A tree's audit log, newest first.
    pub async fn list_entries(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        filter: AuditFilter,
        params: &PaginationParams,
    ) -> Result<Connection<AuditEntry>, OxidGeneError> {
        let limit = params.clamped_first();
        let mut query = audit_entry::Entity::find().filter(audit_entry::Column::TreeId.eq(tree_id));
        if let Some(category) = filter.category {
            query = query.filter(audit_entry::Column::Category.eq(category.as_str()));
        }
        if let Some(subject_id) = filter.subject_id {
            query = query.filter(audit_entry::Column::SubjectId.eq(subject_id));
        }
        let total_count = PaginatorTrait::count(query.clone(), db)
            .await
            .map_err(db_err)?;
        if let Some(after) = params.decode_cursor()? {
            query = query.filter(audit_entry::Column::Id.lt(after));
        }
        let mut rows = query
            .order_by(audit_entry::Column::Id, Order::Desc)
            .limit(limit + 1)
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(limit as usize);
        let entries = with_counts(db, rows).await?;
        Ok(connection(
            entries.into_iter().map(|e| (e.id, e)).collect(),
            has_next_page,
            total_count,
        ))
    }

    /// A record's versions, latest — its live state — first.
    ///
    /// A record that never existed in the tree, or not any more and without
    /// any stored state, has none.
    pub async fn list_versions(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<RecordVersion>, OxidGeneError> {
        let limit = params.clamped_first() as usize;
        let key = (record_type, record_id);
        let heads = Heads::load(db, tree_id, &[key]).await?;
        let stored = heads.stored(key);
        let exists =
            !stored.is_empty() || record_exists(db, tree_id, record_type, record_id).await?;
        if !exists {
            return Ok(connection(Vec::new(), false, 0));
        }
        let current = heads.current_number(key);
        // Version numbers, latest first, past the cursor.
        let below = match params.decode_cursor()? {
            None => current + 1,
            Some(after) if after == record_id => current,
            Some(after) => stored
                .iter()
                .find(|head| head.id == after)
                .map(|head| head.version)
                .ok_or_else(|| OxidGeneError::Validation(format!("Invalid cursor: {after}")))?,
        };
        let numbers: Vec<i32> = (1..below).rev().take(limit + 1).collect();
        let has_next_page = numbers.len() > limit;
        let wanted: Vec<(RecordType, Uuid, i32)> = numbers
            .iter()
            .take(limit)
            .map(|n| (record_type, record_id, *n))
            .collect();
        let mut presented = present(db, tree_id, &heads, &wanted).await?;
        let versions: Vec<(Uuid, RecordVersion)> = wanted
            .iter()
            .filter_map(|key| presented.remove(key))
            .map(|version| (version.id, version))
            .collect();
        Ok(connection(versions, has_next_page, current as u64))
    }

    /// One version of a record, the live one included.
    pub async fn get_version(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        version: i32,
    ) -> Result<RecordVersion, OxidGeneError> {
        let key = (record_type, record_id);
        let heads = Heads::load(db, tree_id, &[key]).await?;
        let wanted = (record_type, record_id, version);
        let not_found = OxidGeneError::NotFound {
            entity: "RecordVersion",
            id: record_id,
        };
        if version < 1 || version > heads.current_number(key) {
            return Err(not_found);
        }
        present(db, tree_id, &heads, &[wanted])
            .await?
            .remove(&wanted)
            .ok_or(not_found)
    }

    /// The states one write replaced, in the order it stored them, each
    /// beside the state the write produced.
    pub async fn list_entry_changes(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entry_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<VersionChange>, OxidGeneError> {
        let limit = params.clamped_first();
        let base = heads_query()
            .column(record_version::Column::RecordType)
            .filter(record_version::Column::TreeId.eq(tree_id))
            .filter(record_version::Column::AuditEntryId.eq(entry_id));
        let total_count = PaginatorTrait::count(
            record_version::Entity::find()
                .filter(record_version::Column::TreeId.eq(tree_id))
                .filter(record_version::Column::AuditEntryId.eq(entry_id)),
            db,
        )
        .await
        .map_err(db_err)?;
        let mut query = base;
        if let Some(after) = params.decode_cursor()? {
            query = query.filter(record_version::Column::Id.gt(after));
        }
        let mut rows: Vec<TypedHead> = query
            .order_by(record_version::Column::Id, Order::Asc)
            .limit(limit + 1)
            .into_model::<TypedHead>()
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(limit as usize);

        let replaced: Vec<(RecordType, Uuid, i32)> = rows
            .iter()
            .map(|row| Ok((parse(&row.record_type)?, row.record_id, row.version)))
            .collect::<Result<_, OxidGeneError>>()?;
        let keys = sorted_unique(replaced.iter().map(|(t, id, _)| (*t, *id)));
        let heads = Heads::load(db, tree_id, &keys).await?;
        let wanted: Vec<(RecordType, Uuid, i32)> = replaced
            .iter()
            .flat_map(|&(t, id, n)| [(t, id, n), (t, id, n + 1)])
            .collect();
        let mut presented = present(db, tree_id, &heads, &wanted).await?;
        let mut changes = Vec::with_capacity(replaced.len());
        for (row, (t, id, n)) in rows.iter().zip(replaced) {
            let (Some(previous), Some(version)) = (
                presented.remove(&(t, id, n)),
                presented.remove(&(t, id, n + 1)),
            ) else {
                continue;
            };
            changes.push((row.id, VersionChange { version, previous }));
        }
        Ok(connection(changes, has_next_page, total_count))
    }
}

/// A version head with its record type, for reads spanning several types.
#[derive(Debug, FromQueryResult)]
struct TypedHead {
    id: Uuid,
    record_id: Uuid,
    version: i32,
    record_type: String,
}

/// The columns of a [`VersionHead`].
fn heads_query() -> sea_orm::Select<record_version::Entity> {
    record_version::Entity::find()
        .select_only()
        .column(record_version::Column::Id)
        .column(record_version::Column::RecordId)
        .column(record_version::Column::Version)
        .column(record_version::Column::Deleted)
        .column_as(record_version::Column::Snapshot.is_null(), "empty")
        .column(record_version::Column::AuditEntryId)
}

/// Every stored version of some records, without their snapshots, oldest
/// first per record.
struct Heads {
    by_record: HashMap<(RecordType, Uuid), Vec<VersionHead>>,
}

impl Heads {
    async fn load(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        records: &[(RecordType, Uuid)],
    ) -> Result<Self, OxidGeneError> {
        let mut by_record: HashMap<(RecordType, Uuid), Vec<VersionHead>> = HashMap::new();
        for (record_type, ids) in by_type(records.iter().copied()) {
            let rows = in_chunks(&ids, |chunk| async move {
                heads_query()
                    .filter(record_version::Column::TreeId.eq(tree_id))
                    .filter(record_version::Column::RecordType.eq(record_type.as_str()))
                    .filter(record_version::Column::RecordId.is_in(chunk))
                    .into_model::<VersionHead>()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?;
            for row in rows {
                by_record
                    .entry((record_type, row.record_id))
                    .or_default()
                    .push(row);
            }
        }
        for heads in by_record.values_mut() {
            heads.sort_by_key(|head| head.version);
        }
        Ok(Self { by_record })
    }

    fn stored(&self, key: (RecordType, Uuid)) -> &[VersionHead] {
        self.by_record.get(&key).map_or(&[], Vec::as_slice)
    }

    /// The number of the record's live state: one after its last stored one.
    fn current_number(&self, key: (RecordType, Uuid)) -> i32 {
        self.stored(key).last().map_or(0, |head| head.version) + 1
    }

    fn head(&self, key: (RecordType, Uuid), version: i32) -> Option<&VersionHead> {
        self.stored(key).iter().find(|head| head.version == version)
    }
}

/// How one presented version is assembled.
#[derive(Clone, Copy)]
enum Source {
    /// From the live rows: the current state, or a deletion marker.
    Live { current: bool },
    /// From a stored snapshot.
    Stored,
    /// A deleted state: no content.
    Deleted,
}

/// A version to present: where its state comes from, its stored head if
/// any, and the audit entry of the write that produced it, when stored.
struct Planned {
    key: (RecordType, Uuid, i32),
    source: Source,
    head: Option<VersionHead>,
    producer: Option<Uuid>,
}

impl Planned {
    fn record(&self) -> (RecordType, Uuid) {
        (self.key.0, self.key.1)
    }
}

/// What each wanted version reads from; numbers past the current one are
/// left out.
fn plan(heads: &Heads, wanted: &[(RecordType, Uuid, i32)]) -> Vec<Planned> {
    let mut plan = Vec::with_capacity(wanted.len());
    for &(record_type, record_id, number) in wanted {
        let key = (record_type, record_id);
        let (source, head) = match heads.head(key, number) {
            _ if number == heads.current_number(key) => (Source::Live { current: true }, None),
            Some(head) if head.deleted => (Source::Deleted, Some(*head)),
            Some(head) if head.empty => (Source::Live { current: false }, Some(*head)),
            Some(head) => (Source::Stored, Some(*head)),
            None => continue,
        };
        plan.push(Planned {
            key: (record_type, record_id, number),
            source,
            head,
            producer: heads.head(key, number - 1).map(|h| h.audit_entry_id),
        });
    }
    plan
}

/// A presented state: current, deleted, and its content.
type State = (bool, bool, Option<(RecordSnapshot, Vec<RecordLabel>)>);

/// The state of a planned version, from the live rows or the stored
/// contents; `None` for the live state of a record that never existed.
fn state_of(
    planned: &Planned,
    heads: &Heads,
    live: &HashMap<(RecordType, Uuid), BuiltSnapshot>,
    stored: &mut HashMap<Uuid, (RecordSnapshot, Vec<RecordLabel>)>,
) -> Option<State> {
    let record = planned.record();
    Some(match planned.source {
        Source::Live { current } => match live.get(&record) {
            Some(built) if !(current && built.deleted) => (
                current,
                false,
                Some((built.snapshot.clone(), built.labels.clone())),
            ),
            // A live state is deleted when its rows are, or gone.
            Some(_) => (true, true, None),
            None if current && heads.stored(record).is_empty() => return None,
            None => (current, true, None),
        },
        Source::Stored => (
            false,
            false,
            planned.head.and_then(|h| stored.remove(&h.id)),
        ),
        Source::Deleted => (false, true, None),
    })
}

/// The versions `wanted` — `(type, record, number)` — of records whose
/// `heads` are loaded, with their states and the writes that produced them.
/// A number beyond the live state's, or the live state of a record that
/// never existed, is left out.
async fn present(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    heads: &Heads,
    wanted: &[(RecordType, Uuid, i32)],
) -> Result<HashMap<(RecordType, Uuid, i32), RecordVersion>, OxidGeneError> {
    let plan = plan(heads, wanted);
    let stored_ids: Vec<Uuid> = plan
        .iter()
        .filter(|p| matches!(p.source, Source::Stored))
        .filter_map(|p| p.head.map(|h| h.id))
        .collect();
    let mut stored = contents(db, &stored_ids).await?;
    let live = live_states(
        db,
        tree_id,
        plan.iter()
            .filter(|p| matches!(p.source, Source::Live { .. }))
            .map(Planned::record),
    )
    .await?;
    let entries = entries_by_id(db, &sorted_unique(plan.iter().filter_map(|p| p.producer))).await?;
    let origins = origins(
        db,
        tree_id,
        plan.iter().filter(|p| p.key.2 == 1).map(Planned::record),
    )
    .await?;

    let mut presented = HashMap::with_capacity(plan.len());
    for planned in &plan {
        let Some((current, deleted, content)) = state_of(planned, heads, &live, &mut stored) else {
            continue;
        };
        let (record_type, record_id, number) = planned.key;
        let entry = match planned.producer {
            Some(id) => entries.get(&id).cloned(),
            None => origins.get(&planned.record()).cloned(),
        };
        let (snapshot, labels) = match content {
            Some((snapshot, labels)) => (Some(snapshot), labels),
            None => (None, Vec::new()),
        };
        presented.insert(
            planned.key,
            RecordVersion {
                id: planned
                    .head
                    .filter(|_| !current)
                    .map_or(record_id, |h| h.id),
                tree_id,
                record_type,
                record_id,
                version: number,
                current,
                deleted,
                entry,
                snapshot,
                labels,
            },
        );
    }
    Ok(presented)
}

/// Group record keys by type, each type's IDs once.
fn by_type(
    records: impl IntoIterator<Item = (RecordType, Uuid)>,
) -> HashMap<RecordType, Vec<Uuid>> {
    let mut grouped: HashMap<RecordType, Vec<Uuid>> = HashMap::new();
    for (record_type, id) in records {
        grouped.entry(record_type).or_default().push(id);
    }
    for ids in grouped.values_mut() {
        *ids = sorted_unique(ids.drain(..));
    }
    grouped
}

/// The live state of each of these records that still has rows.
async fn live_states(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    records: impl IntoIterator<Item = (RecordType, Uuid)>,
) -> Result<HashMap<(RecordType, Uuid), BuiltSnapshot>, OxidGeneError> {
    let mut live = HashMap::new();
    for (record_type, ids) in by_type(records) {
        for built in SnapshotRepo::build(db, tree_id, record_type, &ids).await? {
            live.insert((record_type, built.record_id), built);
        }
    }
    Ok(live)
}

/// Whether a record has rows in the tree, deleted or not.
async fn record_exists(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    record_type: RecordType,
    record_id: Uuid,
) -> Result<bool, OxidGeneError> {
    Ok(!created_at(db, tree_id, record_type, &[record_id])
        .await?
        .is_empty())
}

/// When each of these records was created, for those that have rows.
async fn created_at(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    record_type: RecordType,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, DateTime<Utc>>, OxidGeneError> {
    let rows: Vec<(Uuid, DateTime<Utc>)> = match record_type {
        RecordType::Person => {
            in_chunks(ids, |chunk| async move {
                person::Entity::find()
                    .select_only()
                    .column(person::Column::Id)
                    .column(person::Column::CreatedAt)
                    .filter(person::Column::TreeId.eq(tree_id))
                    .filter(person::Column::Id.is_in(chunk))
                    .into_tuple()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?
        }
        RecordType::Place => {
            in_chunks(ids, |chunk| async move {
                place::Entity::find()
                    .select_only()
                    .column(place::Column::Id)
                    .column(place::Column::CreatedAt)
                    .filter(place::Column::TreeId.eq(tree_id))
                    .filter(place::Column::Id.is_in(chunk))
                    .into_tuple()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?
        }
        RecordType::Source => {
            in_chunks(ids, |chunk| async move {
                source::Entity::find()
                    .select_only()
                    .column(source::Column::Id)
                    .column(source::Column::CreatedAt)
                    .filter(source::Column::TreeId.eq(tree_id))
                    .filter(source::Column::Id.is_in(chunk))
                    .into_tuple()
                    .all(db)
                    .await
                    .map_err(db_err)
            })
            .await?
        }
        RecordType::Tree => tree::Entity::find()
            .select_only()
            .column(tree::Column::Id)
            .column(tree::Column::CreatedAt)
            .filter(tree::Column::Id.eq(tree_id))
            .filter(tree::Column::Id.is_in(ids.iter().copied()))
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)?,
    };
    Ok(rows.into_iter().collect())
}

/// The write each of these records' first state came from: the write that
/// created it, or else the first import completed once it existed. Records
/// neither of which was recorded are absent.
async fn origins(
    db: &impl ConnectionTrait,
    tree_id: Uuid,
    records: impl IntoIterator<Item = (RecordType, Uuid)>,
) -> Result<HashMap<(RecordType, Uuid), AuditEntry>, OxidGeneError> {
    let records: Vec<(RecordType, Uuid)> = sorted_unique(records);
    if records.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = sorted_unique(records.iter().map(|(_, id)| *id));
    let created: Vec<audit_entry::Model> = in_chunks(&ids, |chunk| async move {
        audit_entry::Entity::find()
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .filter(audit_entry::Column::Action.eq(AuditAction::Create.as_str()))
            .filter(audit_entry::Column::EntityId.is_in(chunk))
            .order_by(audit_entry::Column::Id, Order::Asc)
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    let mut by_record: HashMap<Uuid, audit_entry::Model> = HashMap::new();
    for row in created {
        if let Some(id) = row.entity_id {
            by_record.entry(id).or_insert(row);
        }
    }

    // The others came from an import: the first one completed after the
    // record was created.
    let imported: Vec<(RecordType, Uuid)> = records
        .iter()
        .copied()
        .filter(|(_, id)| !by_record.contains_key(id))
        .collect();
    if !imported.is_empty() {
        let imports: Vec<audit_entry::Model> = audit_entry::Entity::find()
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .filter(audit_entry::Column::Action.eq(AuditAction::Import.as_str()))
            .order_by(audit_entry::Column::Id, Order::Asc)
            .all(db)
            .await
            .map_err(db_err)?;
        for (record_type, ids) in by_type(imported) {
            for (id, created) in created_at(db, tree_id, record_type, &ids).await? {
                if let Some(import) = imports.iter().find(|row| row.occurred_at >= created) {
                    by_record.insert(id, import.clone());
                }
            }
        }
    }

    // `with_counts` reads each entry once, however many records it brought.
    let entries: HashMap<Uuid, AuditEntry> = with_counts(db, by_record.values().cloned().collect())
        .await?
        .into_iter()
        .map(|entry| (entry.id, entry))
        .collect();
    Ok(records
        .into_iter()
        .filter_map(|(record_type, id)| {
            let entry = entries.get(&by_record.get(&id)?.id)?.clone();
            Some(((record_type, id), entry))
        })
        .collect())
}

/// The snapshots and labels stored under these version IDs.
async fn contents(
    db: &impl ConnectionTrait,
    version_ids: &[Uuid],
) -> Result<HashMap<Uuid, (RecordSnapshot, Vec<RecordLabel>)>, OxidGeneError> {
    let rows: Vec<(Uuid, Option<String>, Option<String>)> =
        in_chunks(version_ids, |chunk| async move {
            record_version::Entity::find()
                .select_only()
                .column(record_version::Column::Id)
                .column(record_version::Column::Snapshot)
                .column(record_version::Column::Labels)
                .filter(record_version::Column::Id.is_in(chunk))
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
    let mut contents = HashMap::with_capacity(rows.len());
    for (id, snapshot, labels) in rows {
        let Some(snapshot) = snapshot else { continue };
        let labels = match labels.as_deref() {
            Some(json) => serde_json::from_str(json).map_err(internal)?,
            None => Vec::new(),
        };
        contents.insert(id, (parse_snapshot(&snapshot)?, labels));
    }
    Ok(contents)
}

/// These audit entries, with their version counts.
async fn entries_by_id(
    db: &impl ConnectionTrait,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, AuditEntry>, OxidGeneError> {
    let rows = in_chunks(ids, |chunk| async move {
        audit_entry::Entity::find()
            .filter(audit_entry::Column::Id.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    Ok(with_counts(db, rows)
        .await?
        .into_iter()
        .map(|entry| (entry.id, entry))
        .collect())
}

/// A version's content as stored: snapshot and labels JSON, or nulls.
fn encode_content(
    content: Option<&(RecordSnapshot, Vec<RecordLabel>)>,
) -> Result<(Option<String>, Option<String>), OxidGeneError> {
    let Some((snapshot, labels)) = content else {
        return Ok((None, None));
    };
    Ok((
        Some(serde_json::to_string(snapshot).map_err(internal)?),
        Some(serde_json::to_string(labels).map_err(internal)?),
    ))
}

/// Wrap a page of items in a connection.
fn connection<T: Clone>(
    items: Vec<(Uuid, T)>,
    has_next_page: bool,
    total_count: u64,
) -> Connection<T> {
    let edges: Vec<Edge<T>> = items
        .into_iter()
        .map(|(id, node)| Edge {
            cursor: encode_cursor(&id),
            node,
        })
        .collect();
    let end_cursor = edges.last().map(|edge| edge.cursor.clone());
    Connection {
        edges,
        page_info: PageInfo {
            has_next_page,
            end_cursor,
        },
        total_count: total_count as i64,
    }
}

#[derive(Debug, FromQueryResult)]
struct VersionCount {
    audit_entry_id: Uuid,
    count: i64,
}

/// Convert entry rows, counting the versions each stored.
async fn with_counts(
    db: &impl ConnectionTrait,
    rows: Vec<audit_entry::Model>,
) -> Result<Vec<AuditEntry>, OxidGeneError> {
    let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let counts: HashMap<Uuid, i64> = in_chunks(&ids, |chunk| async move {
        record_version::Entity::find()
            .select_only()
            .column(record_version::Column::AuditEntryId)
            .column_as(record_version::Column::Id.count(), "count")
            .filter(record_version::Column::AuditEntryId.is_in(chunk))
            .group_by(record_version::Column::AuditEntryId)
            .into_model::<VersionCount>()
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?
    .into_iter()
    .map(|row| (row.audit_entry_id, row.count))
    .collect();
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| seen.insert(row.id))
        .map(|row| {
            let count = counts.get(&row.id).copied().unwrap_or(0);
            entry_from_row(row, count)
        })
        .collect()
}

fn entry_from_row(
    row: audit_entry::Model,
    version_count: i64,
) -> Result<AuditEntry, OxidGeneError> {
    Ok(AuditEntry {
        id: row.id,
        tree_id: row.tree_id,
        occurred_at: row.occurred_at,
        category: parse(&row.category)?,
        action: parse(&row.action)?,
        entity: parse(&row.entity)?,
        entity_id: row.entity_id,
        subject: row.subject.as_deref().map(parse).transpose()?,
        subject_id: row.subject_id,
        label: row.label,
        details: parse_details(row.details.as_deref())?,
        version_count,
    })
}

fn parse_details(json: Option<&str>) -> Result<AuditDetails, OxidGeneError> {
    match json {
        Some(json) => serde_json::from_str(json).map_err(internal),
        None => Ok(AuditDetails::default()),
    }
}

fn parse<T: std::str::FromStr<Err = String>>(value: &str) -> Result<T, OxidGeneError> {
    value.parse().map_err(OxidGeneError::Internal)
}

fn parse_snapshot(json: &str) -> Result<RecordSnapshot, OxidGeneError> {
    serde_json::from_str(json).map_err(internal)
}

fn internal(error: serde_json::Error) -> OxidGeneError {
    OxidGeneError::Internal(error.to_string())
}
