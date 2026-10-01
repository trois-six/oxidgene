//! Client calls for a tree's audit log and its records' versions.

use oxidgene_core::history::{AuditCategory, AuditEntry, RecordType, RecordVersion, VersionChange};
use oxidgene_core::types::Connection;
use serde::Serialize;
use uuid::Uuid;

use super::{ApiClient, ApiError};

/// The largest page the history endpoints return.
pub const HISTORY_PAGE_SIZE: u64 = 100;

#[derive(Serialize)]
struct RevertBody {
    version: i32,
}

impl ApiClient {
    /// A page of the tree's audit log, newest first.
    pub async fn list_audit(
        &self,
        tree_id: Uuid,
        category: Option<AuditCategory>,
        after: Option<&str>,
    ) -> Result<Connection<AuditEntry>, ApiError> {
        let mut params = vec![("first", HISTORY_PAGE_SIZE.to_string())];
        if let Some(category) = category {
            params.push(("category", category.as_str().to_string()));
        }
        if let Some(after) = after {
            params.push(("after", after.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/audit"), &params)
            .await
    }

    /// A page of the versions one write produced, each beside the one it
    /// replaced.
    pub async fn list_audit_changes(
        &self,
        tree_id: Uuid,
        entry_id: Uuid,
        after: Option<&str>,
    ) -> Result<Connection<VersionChange>, ApiError> {
        let mut params = vec![("first", HISTORY_PAGE_SIZE.to_string())];
        if let Some(after) = after {
            params.push(("after", after.to_string()));
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/audit/{entry_id}/changes"),
            &params,
        )
        .await
    }

    /// A page of a record's versions, latest first.
    pub async fn list_versions(
        &self,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        after: Option<&str>,
    ) -> Result<Connection<RecordVersion>, ApiError> {
        let mut params = vec![("first", HISTORY_PAGE_SIZE.to_string())];
        if let Some(after) = after {
            params.push(("after", after.to_string()));
        }
        self.get_with_query(
            &format!(
                "/api/v1/trees/{tree_id}/history/{}/{record_id}",
                record_type.as_str()
            ),
            &params,
        )
        .await
    }

    /// One version of a record.
    pub async fn get_version(
        &self,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        version: i32,
    ) -> Result<RecordVersion, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/history/{}/{record_id}/{version}",
            record_type.as_str()
        ))
        .await
    }

    /// Put a record back as `version` had it.
    pub async fn revert_record(
        &self,
        tree_id: Uuid,
        record_type: RecordType,
        record_id: Uuid,
        version: i32,
    ) -> Result<AuditEntry, ApiError> {
        self.post(
            &format!(
                "/api/v1/trees/{tree_id}/history/{}/{record_id}/revert",
                record_type.as_str()
            ),
            &RevertBody { version },
        )
        .await
    }
}
