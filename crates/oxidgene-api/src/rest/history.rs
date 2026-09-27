//! REST handlers for a tree's audit log and its records' versions.

use axum::Json;
use axum::extract::{Path, Query, State};
use oxidgene_core::history::{AuditEntry, RecordType, RecordVersion, VersionChange};
use oxidgene_core::types::Connection;
use oxidgene_db::repo::{AuditFilter, HistoryRepo, PaginationParams};
use uuid::Uuid;

use super::dto::{AuditQuery, PaginationQuery, RevertRecordRequest};
use super::error::ApiError;
use super::state::{AppState, begin_tx, commit_tx};
use crate::service::history;

/// GET /api/v1/trees/:tree_id/audit
///
/// The tree's audit log, newest first, optionally narrowed to one category or
/// to the writes about one record.
pub async fn list_audit(
    State(state): State<AppState>,
    Path(tree_id): Path<Uuid>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Connection<AuditEntry>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    let filter = AuditFilter {
        category: query.category,
        subject_id: query.subject_id,
    };
    Ok(Json(
        HistoryRepo::list_entries(&state.db, tree_id, filter, &params).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/audit/:entry_id
pub async fn get_audit_entry(
    State(state): State<AppState>,
    Path((tree_id, entry_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<AuditEntry>, ApiError> {
    Ok(Json(
        HistoryRepo::get_entry(&state.db, tree_id, entry_id).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/audit/:entry_id/changes
///
/// The versions one write produced, each beside the version it replaced.
pub async fn list_audit_changes(
    State(state): State<AppState>,
    Path((tree_id, entry_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<VersionChange>>, ApiError> {
    HistoryRepo::get_entry(&state.db, tree_id, entry_id).await?;
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        HistoryRepo::list_entry_changes(&state.db, tree_id, entry_id, &params).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/history/:record_type/:record_id
///
/// A record's versions, latest first.
pub async fn list_versions(
    State(state): State<AppState>,
    Path((tree_id, record_type, record_id)): Path<(Uuid, RecordType, Uuid)>,
    Query(query): Query<PaginationQuery>,
) -> Result<Json<Connection<RecordVersion>>, ApiError> {
    let params = PaginationParams {
        first: query.first.unwrap_or(25),
        after: query.after,
    };
    Ok(Json(
        HistoryRepo::list_versions(&state.db, tree_id, record_type, record_id, &params).await?,
    ))
}

/// GET /api/v1/trees/:tree_id/history/:record_type/:record_id/:version
pub async fn get_version(
    State(state): State<AppState>,
    Path((tree_id, record_type, record_id, version)): Path<(Uuid, RecordType, Uuid, i32)>,
) -> Result<Json<RecordVersion>, ApiError> {
    Ok(Json(
        HistoryRepo::get_version(&state.db, tree_id, record_type, record_id, version).await?,
    ))
}

/// POST /api/v1/trees/:tree_id/history/:record_type/:record_id/revert
///
/// Put the record back as `version` had it. The restore is itself a write:
/// it returns its own audit entry, and the state it restores becomes the
/// record's newest version.
pub async fn revert_record(
    State(state): State<AppState>,
    Path((tree_id, record_type, record_id)): Path<(Uuid, RecordType, Uuid)>,
    Json(body): Json<RevertRecordRequest>,
) -> Result<Json<AuditEntry>, ApiError> {
    let txn = begin_tx(&state.db).await?;
    let entry = history::revert(
        &txn,
        &state.profiles,
        tree_id,
        record_type,
        record_id,
        body.version,
    )
    .await?;
    commit_tx(txn).await?;
    Ok(Json(entry))
}
