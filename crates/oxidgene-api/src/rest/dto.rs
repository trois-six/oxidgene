//! Request/response DTOs for REST endpoints.

use oxidgene_core::{ChildType, NameType, Sex, SpouseRole};
use serde::{Deserialize, Serialize};

// ── Pagination query params ──────────────────────────────────────────

/// Query parameters for cursor-based pagination.
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    /// Number of items to return (default: 25, max: 100).
    pub first: Option<u64>,
    /// Cursor to start after (UUID string).
    pub after: Option<String>,
}

// ── Tree DTOs ────────────────────────────────────────────────────────

/// Request body for creating a tree.
#[derive(Debug, Deserialize)]
pub struct CreateTreeRequest {
    pub name: String,
    pub description: Option<String>,
}

/// Request body for updating a tree.
#[derive(Debug, Deserialize)]
pub struct UpdateTreeRequest {
    pub name: Option<String>,
    /// `null` clears the description; absent field leaves it unchanged.
    pub description: Option<Option<String>>,
}

// ── Person DTOs ──────────────────────────────────────────────────────

/// Request body for creating a person.
#[derive(Debug, Deserialize)]
pub struct CreatePersonRequest {
    pub sex: Sex,
}

/// Request body for updating a person.
#[derive(Debug, Deserialize)]
pub struct UpdatePersonRequest {
    pub sex: Option<Sex>,
}

// ── PersonName DTOs ──────────────────────────────────────────────────

/// Request body for creating a person name.
#[derive(Debug, Deserialize)]
pub struct CreatePersonNameRequest {
    pub name_type: NameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
}

/// Request body for updating a person name.
#[derive(Debug, Deserialize)]
pub struct UpdatePersonNameRequest {
    pub name_type: Option<NameType>,
    pub given_names: Option<Option<String>>,
    pub surname: Option<Option<String>>,
    pub prefix: Option<Option<String>>,
    pub suffix: Option<Option<String>>,
    pub nickname: Option<Option<String>>,
    pub is_primary: Option<bool>,
}

// ── Family DTOs ──────────────────────────────────────────────────────

// Family has no extra fields to create/update beyond tree_id (from path),
// so we don't need a CreateFamilyRequest. Update just touches updated_at.

// ── FamilySpouse DTOs ────────────────────────────────────────────────

/// Request body for adding a spouse to a family.
#[derive(Debug, Deserialize)]
pub struct AddSpouseRequest {
    pub person_id: uuid::Uuid,
    pub role: SpouseRole,
    #[serde(default)]
    pub sort_order: i32,
}

// ── FamilyChild DTOs ─────────────────────────────────────────────────

/// Request body for adding a child to a family.
#[derive(Debug, Deserialize)]
pub struct AddChildRequest {
    pub person_id: uuid::Uuid,
    pub child_type: ChildType,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Ancestry query params ────────────────────────────────────────────

/// Query parameters for ancestor/descendant queries.
#[derive(Debug, Deserialize)]
pub struct AncestryQuery {
    /// Maximum depth to traverse.
    pub max_depth: Option<i32>,
}

// ── Generic ID response ──────────────────────────────────────────────

/// Minimal response for delete operations.
#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}
