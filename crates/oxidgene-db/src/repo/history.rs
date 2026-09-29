//! Repository for the audit log and the record versions.
//!
//! Storage only: which records a write versions, and what their snapshots
//! hold, is decided by the API's history service and [`super::SnapshotRepo`].
//!
//! The audit log reads newest first. A record's versions read from the latest
//! down, and the versions one write produced in the order it wrote them.

use std::collections::HashMap;

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

use super::batch::{in_chunks, sorted_unique};
use super::pagination::{PaginationParams, encode_cursor};
use crate::entities::{audit_entry, record_version};
use crate::repo::db_err;

/// Filters of the audit log.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuditFilter {
    pub category: Option<AuditCategory>,
    /// Only the writes about this record.
    pub subject_id: Option<Uuid>,
}

/// A record state about to be stored.
#[derive(Debug, Clone)]
pub struct NewRecordVersion {
    pub record_id: Uuid,
    pub version: i32,
    pub deleted: bool,
    pub snapshot: RecordSnapshot,
    pub labels: Vec<RecordLabel>,
}

/// The latest stored state of a record.
#[derive(Debug, Clone)]
pub struct LatestVersion {
    pub version: i32,
    pub deleted: bool,
    pub snapshot: RecordSnapshot,
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
            .map_err(|e| OxidGeneError::Internal(e.to_string()))?;
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

    /// Store the versions one write produced.
    pub async fn insert_versions(
        db: &impl ConnectionTrait,
        entry: &AuditEntry,
        record_type: RecordType,
        versions: Vec<NewRecordVersion>,
    ) -> Result<(), OxidGeneError> {
        // Bounded batches: every row binds ten values.
        const BATCH: usize = 500;
        let mut rows = Vec::with_capacity(versions.len().min(BATCH));
        for version in versions {
            rows.push(record_version::ActiveModel {
                id: Set(Uuid::now_v7()),
                tree_id: Set(entry.tree_id),
                audit_entry_id: Set(entry.id),
                record_type: Set(record_type.as_str().to_string()),
                record_id: Set(version.record_id),
                version: Set(version.version),
                deleted: Set(version.deleted),
                created_at: Set(entry.occurred_at),
                snapshot: Set(serde_json::to_string(&version.snapshot)
                    .map_err(|e| OxidGeneError::Internal(e.to_string()))?),
                labels: Set(serde_json::to_string(&version.labels)
                    .map_err(|e| OxidGeneError::Internal(e.to_string()))?),
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

    /// The latest stored state of each of `record_ids` that has one.
    pub async fn latest_versions(
        db: &impl ConnectionTrait,
        record_type: RecordType,
        record_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, LatestVersion>, OxidGeneError> {
        let rows = in_chunks(record_ids, |chunk| async move {
            record_version::Entity::find()
                .filter(record_version::Column::RecordType.eq(record_type.as_str()))
                .filter(record_version::Column::RecordId.is_in(chunk))
                .filter(Expr::cust(
                    "record_version.version = (SELECT MAX(latest.version) \
                     FROM record_version latest \
                     WHERE latest.record_type = record_version.record_type \
                     AND latest.record_id = record_version.record_id)",
                ))
                .all(db)
                .await
                .map_err(db_err)
        })
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.record_id,
                    LatestVersion {
                        version: row.version,
                        deleted: row.deleted,
                        snapshot: parse_snapshot(&row.snapshot)?,
                    },
                ))
            })
            .collect()
    }

    /// The persons of a tree whose record changed most recently, newest first.
    ///
    /// A person counts as modified when a write stored a new version of them
    /// — which a change to their names, events, notes, citations or unions
    /// does, and which renaming a relative or touching a media does not. The
    /// versions an import or a baseline stores are left out: they record
    /// every person at once, so the latest of them would name arbitrary
    /// persons rather than the ones somebody worked on. Persons deleted since
    /// are left out too.
    ///
    /// Driven from the tree's audit entries, so a large import's versions are
    /// never read: they hang off an entry the action filter has already
    /// discarded. The live-person lookup is also what keeps only person
    /// versions — IDs are unique across tables. Filtering on `record_type`
    /// instead would hand SQLite the `(record_type, record_id)` index, and
    /// with it every person version of every tree, imports included.
    pub async fn recently_modified_persons(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        limit: u64,
    ) -> Result<Vec<Uuid>, OxidGeneError> {
        record_version::Entity::find()
            .select_only()
            .column(record_version::Column::RecordId)
            .inner_join(audit_entry::Entity)
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .filter(
                audit_entry::Column::Action
                    .is_not_in([AuditAction::Import.as_str(), AuditAction::Baseline.as_str()]),
            )
            .filter(Expr::cust(
                "EXISTS (SELECT 1 FROM person \
                 WHERE person.id = record_version.record_id \
                 AND person.deleted_at IS NULL)",
            ))
            .group_by(record_version::Column::RecordId)
            // A version's `created_at` is its entry's `occurred_at`. Persons
            // one write versioned together tie on it; the newest record wins.
            .order_by(record_version::Column::CreatedAt.max(), Order::Desc)
            .order_by(record_version::Column::RecordId, Order::Desc)
            .limit(limit)
            .into_tuple()
            .all(db)
            .await
            .map_err(db_err)
    }

    /// Whether anything was ever recorded for the tree.
    pub async fn has_entries(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<bool, OxidGeneError> {
        let found = audit_entry::Entity::find()
            .filter(audit_entry::Column::TreeId.eq(tree_id))
            .select_only()
            .column(audit_entry::Column::Id)
            .into_tuple::<Uuid>()
            .one(db)
            .await
            .map_err(db_err)?;
        Ok(found.is_some())
    }

    /// When each of a tree's completed imports happened, with its details
    /// (format, file name, persons brought), oldest first. A duplication is
    /// the new tree's import.
    pub async fn imports(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(chrono::DateTime<chrono::Utc>, AuditDetails)>, OxidGeneError> {
        let rows: Vec<(chrono::DateTime<chrono::Utc>, Option<String>)> =
            audit_entry::Entity::find()
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
            .map(|(occurred_at, details)| {
                let details = match details.as_deref() {
                    Some(json) => serde_json::from_str(json)
                        .map_err(|e| OxidGeneError::Internal(e.to_string()))?,
                    None => AuditDetails::default(),
                };
                Ok((occurred_at, details))
            })
            .collect()
    }

    /// The spells a tree's persons spent deleted before a restore brought
    /// them back, as `(deleted, restored)` times.
    ///
    /// A restore clears `person.deleted_at`, so the person table alone would
    /// read as if a restored person had never been deleted. Only a person
    /// restore can bring a deleted person back, so the read starts from the
    /// tree's `revert` entries on persons — few — and reads those persons'
    /// version flags and times, never their snapshots: each deleted version
    /// followed by a live one is such a spell.
    pub async fn person_restores(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
    ) -> Result<Vec<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>, OxidGeneError>
    {
        let restored = sorted_unique(
            audit_entry::Entity::find()
                .select_only()
                .column(audit_entry::Column::EntityId)
                .filter(audit_entry::Column::TreeId.eq(tree_id))
                .filter(audit_entry::Column::Category.eq(AuditCategory::History.as_str()))
                .filter(audit_entry::Column::Action.eq(AuditAction::Revert.as_str()))
                .filter(audit_entry::Column::Entity.eq(AuditEntity::Person.as_str()))
                .filter(audit_entry::Column::EntityId.is_not_null())
                .into_tuple()
                .all(db)
                .await
                .map_err(db_err)?,
        );
        let mut rows: Vec<(Uuid, i32, bool, chrono::DateTime<chrono::Utc>)> =
            in_chunks(&restored, |chunk| async move {
                record_version::Entity::find()
                    .select_only()
                    .column(record_version::Column::RecordId)
                    .column(record_version::Column::Version)
                    .column(record_version::Column::Deleted)
                    .column(record_version::Column::CreatedAt)
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
            .filter(|pair| pair[0].0 == pair[1].0 && pair[0].2 && !pair[1].2)
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

    /// A record's versions, latest first.
    pub async fn list_versions(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<RecordVersion>, OxidGeneError> {
        let limit = params.clamped_first();
        let base = record_version::Entity::find()
            .filter(record_version::Column::TreeId.eq(tree_id))
            .filter(record_version::Column::RecordType.eq(record_type.as_str()))
            .filter(record_version::Column::RecordId.eq(record_id));
        let total_count = PaginatorTrait::count(base.clone(), db)
            .await
            .map_err(db_err)?;
        let mut query = base.clone();
        if let Some(after) = params.decode_cursor()? {
            // Versions are ordered by number, not by ID: find where the
            // cursor's version sits.
            let cursor = base
                .clone()
                .filter(record_version::Column::Id.eq(after))
                .one(db)
                .await
                .map_err(db_err)?
                .ok_or_else(|| OxidGeneError::Validation(format!("Invalid cursor: {after}")))?;
            query = query.filter(record_version::Column::Version.lt(cursor.version));
        }
        let mut rows = query
            .order_by(record_version::Column::Version, Order::Desc)
            .limit(limit + 1)
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(limit as usize);
        let versions = with_entries(db, rows).await?;
        Ok(connection(
            versions.into_iter().map(|v| (v.id, v)).collect(),
            has_next_page,
            total_count,
        ))
    }

    /// One version of a record.
    pub async fn get_version(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        version: i32,
    ) -> Result<RecordVersion, OxidGeneError> {
        let row = record_version::Entity::find()
            .filter(record_version::Column::TreeId.eq(tree_id))
            .filter(record_version::Column::RecordType.eq(record_type.as_str()))
            .filter(record_version::Column::RecordId.eq(record_id))
            .filter(record_version::Column::Version.eq(version))
            .one(db)
            .await
            .map_err(db_err)?
            .ok_or(OxidGeneError::NotFound {
                entity: "RecordVersion",
                id: record_id,
            })?;
        let mut versions = with_entries(db, vec![row]).await?;
        Ok(versions.remove(0))
    }

    /// The versions one write produced, each with the one it replaced.
    pub async fn list_entry_changes(
        db: &impl ConnectionTrait,
        tree_id: Uuid,
        entry_id: Uuid,
        params: &PaginationParams,
    ) -> Result<Connection<VersionChange>, OxidGeneError> {
        let limit = params.clamped_first();
        let base = record_version::Entity::find()
            .filter(record_version::Column::TreeId.eq(tree_id))
            .filter(record_version::Column::AuditEntryId.eq(entry_id));
        let total_count = PaginatorTrait::count(base.clone(), db)
            .await
            .map_err(db_err)?;
        let mut query = base;
        if let Some(after) = params.decode_cursor()? {
            query = query.filter(record_version::Column::Id.gt(after));
        }
        let mut rows = query
            .order_by(record_version::Column::Id, Order::Asc)
            .limit(limit + 1)
            .all(db)
            .await
            .map_err(db_err)?;
        let has_next_page = rows.len() as u64 > limit;
        rows.truncate(limit as usize);

        // The versions these replaced, one query per record type.
        let mut wanted: HashMap<String, Vec<Uuid>> = HashMap::new();
        for row in rows.iter().filter(|row| row.version > 1) {
            wanted
                .entry(row.record_type.clone())
                .or_default()
                .push(row.record_id);
        }
        let mut previous_rows = Vec::new();
        for (record_type, ids) in wanted {
            let found = in_chunks(&ids, |chunk| {
                let record_type = record_type.clone();
                async move {
                    record_version::Entity::find()
                        .filter(record_version::Column::RecordType.eq(record_type))
                        .filter(record_version::Column::RecordId.is_in(chunk))
                        .all(db)
                        .await
                        .map_err(db_err)
                }
            })
            .await?;
            previous_rows.extend(found.into_iter().filter(|candidate| {
                rows.iter().any(|row| {
                    row.record_type == candidate.record_type
                        && row.record_id == candidate.record_id
                        && row.version == candidate.version + 1
                })
            }));
        }
        let previous = with_entries(db, previous_rows).await?;
        let versions = with_entries(db, rows).await?;
        let changes = versions
            .into_iter()
            .map(|version| {
                let before = previous
                    .iter()
                    .find(|p| {
                        p.record_type == version.record_type
                            && p.record_id == version.record_id
                            && p.version + 1 == version.version
                    })
                    .cloned();
                (
                    version.id,
                    VersionChange {
                        version,
                        previous: before,
                    },
                )
            })
            .collect();
        Ok(connection(changes, has_next_page, total_count))
    }
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

/// Convert entry rows, counting the versions each produced.
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
    rows.into_iter()
        .map(|row| {
            let count = counts.get(&row.id).copied().unwrap_or(0);
            entry_from_row(row, count)
        })
        .collect()
}

/// Convert version rows, attaching the entry that produced each.
async fn with_entries(
    db: &impl ConnectionTrait,
    rows: Vec<record_version::Model>,
) -> Result<Vec<RecordVersion>, OxidGeneError> {
    let entry_ids = sorted_unique(rows.iter().map(|row| row.audit_entry_id).collect());
    let entry_rows = in_chunks(&entry_ids, |chunk| async move {
        audit_entry::Entity::find()
            .filter(audit_entry::Column::Id.is_in(chunk))
            .all(db)
            .await
            .map_err(db_err)
    })
    .await?;
    let entries: HashMap<Uuid, AuditEntry> = with_counts(db, entry_rows)
        .await?
        .into_iter()
        .map(|entry| (entry.id, entry))
        .collect();
    rows.into_iter()
        .map(|row| {
            let entry =
                entries
                    .get(&row.audit_entry_id)
                    .cloned()
                    .ok_or(OxidGeneError::NotFound {
                        entity: "AuditEntry",
                        id: row.audit_entry_id,
                    })?;
            Ok(RecordVersion {
                id: row.id,
                tree_id: row.tree_id,
                record_type: parse(&row.record_type)?,
                record_id: row.record_id,
                version: row.version,
                deleted: row.deleted,
                created_at: row.created_at,
                entry,
                snapshot: parse_snapshot(&row.snapshot)?,
                labels: serde_json::from_str(&row.labels)
                    .map_err(|e| OxidGeneError::Internal(e.to_string()))?,
            })
        })
        .collect()
}

fn entry_from_row(
    row: audit_entry::Model,
    version_count: i64,
) -> Result<AuditEntry, OxidGeneError> {
    let details: AuditDetails = match row.details.as_deref() {
        Some(json) => {
            serde_json::from_str(json).map_err(|e| OxidGeneError::Internal(e.to_string()))?
        }
        None => AuditDetails::default(),
    };
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
        details,
        version_count,
    })
}

fn parse<T: std::str::FromStr<Err = String>>(value: &str) -> Result<T, OxidGeneError> {
    value.parse().map_err(OxidGeneError::Internal)
}

fn parse_snapshot(json: &str) -> Result<RecordSnapshot, OxidGeneError> {
    serde_json::from_str(json).map_err(|e| OxidGeneError::Internal(e.to_string()))
}
