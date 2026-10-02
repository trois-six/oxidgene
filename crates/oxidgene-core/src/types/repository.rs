use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::SourceMediaType;

/// A place holding sources — an archive, a library, a registry office
/// (GEDCOM `REPO`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub name: String,
    /// The postal address, over several lines.
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub website: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// That a source is held at a repository, under one call number (GEDCOM
/// `SOUR.REPO` with its `CALN` and `MEDI`). A source held under several call
/// numbers at the same repository has one link per call number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceRepository {
    pub id: Uuid,
    pub source_id: Uuid,
    pub repository_id: Uuid,
    pub call_number: Option<String>,
    /// The medium the source is kept on there.
    pub media_type: Option<SourceMediaType>,
    pub sort_order: i32,
}
