//! Request/response DTOs for REST endpoints.

use oxidgene_core::{ChildType, Confidence, EventType, NameType, Sex, SpouseRole};
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

// ── Event DTOs ───────────────────────────────────────────────────────

/// Query parameters for listing events (includes filters + pagination).
#[derive(Debug, Deserialize)]
pub struct EventListQuery {
    pub first: Option<u64>,
    pub after: Option<String>,
    pub event_type: Option<EventType>,
    pub person_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
}

/// Request body for creating an event.
#[derive(Debug, Deserialize)]
pub struct CreateEventRequest {
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<chrono::NaiveDate>,
    pub place_id: Option<uuid::Uuid>,
    pub person_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub description: Option<String>,
}

/// Request body for updating an event.
#[derive(Debug, Deserialize)]
pub struct UpdateEventRequest {
    pub event_type: Option<EventType>,
    pub date_value: Option<Option<String>>,
    pub date_sort: Option<Option<chrono::NaiveDate>>,
    pub place_id: Option<Option<uuid::Uuid>>,
    pub description: Option<Option<String>>,
}

// ── Place DTOs ───────────────────────────────────────────────────────

/// Query parameters for listing places (search + pagination).
#[derive(Debug, Deserialize)]
pub struct PlaceListQuery {
    pub first: Option<u64>,
    pub after: Option<String>,
    pub search: Option<String>,
}

/// Request body for creating a place.
#[derive(Debug, Deserialize)]
pub struct CreatePlaceRequest {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Request body for updating a place.
#[derive(Debug, Deserialize)]
pub struct UpdatePlaceRequest {
    pub name: Option<String>,
    pub latitude: Option<Option<f64>>,
    pub longitude: Option<Option<f64>>,
}

// ── Source DTOs ──────────────────────────────────────────────────────

/// Request body for creating a source.
#[derive(Debug, Deserialize)]
pub struct CreateSourceRequest {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
}

/// Request body for updating a source.
#[derive(Debug, Deserialize)]
pub struct UpdateSourceRequest {
    pub title: Option<String>,
    pub author: Option<Option<String>>,
    pub publisher: Option<Option<String>>,
    pub abbreviation: Option<Option<String>>,
    pub repository_name: Option<Option<String>>,
}

// ── Citation DTOs ───────────────────────────────────────────────────

/// Request body for creating a citation.
#[derive(Debug, Deserialize)]
pub struct CreateCitationRequest {
    pub source_id: uuid::Uuid,
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub page: Option<String>,
    pub confidence: Confidence,
    pub text: Option<String>,
}

/// Request body for updating a citation.
#[derive(Debug, Deserialize)]
pub struct UpdateCitationRequest {
    pub page: Option<Option<String>>,
    pub confidence: Option<Confidence>,
    pub text: Option<Option<String>>,
}

// ── Media DTOs ──────────────────────────────────────────────────────

/// Request body for creating a media record (metadata only).
#[derive(Debug, Deserialize)]
pub struct CreateMediaRequest {
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Request body for updating media metadata.
#[derive(Debug, Deserialize)]
pub struct UpdateMediaRequest {
    pub title: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

// ── MediaLink DTOs ──────────────────────────────────────────────────

/// Request body for creating a media link.
#[derive(Debug, Deserialize)]
pub struct CreateMediaLinkRequest {
    pub media_id: uuid::Uuid,
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Note DTOs ───────────────────────────────────────────────────────

/// Query parameters for listing notes by entity.
#[derive(Debug, Deserialize)]
pub struct NoteListQuery {
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
}

/// Request body for creating a note.
#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub text: String,
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
}

/// Request body for updating a note.
#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub text: Option<String>,
}

// ── GEDCOM DTOs ──────────────────────────────────────────────────────

/// Request body for importing a GEDCOM string.
#[derive(Debug, Deserialize)]
pub struct ImportGedcomRequest {
    pub gedcom: String,
}

/// Response body for GEDCOM import.
#[derive(Debug, Serialize)]
pub struct ImportGedcomResponse {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub media_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

/// Response body for GEDCOM export.
#[derive(Debug, Serialize)]
pub struct ExportGedcomResponse {
    pub gedcom: String,
    pub warnings: Vec<String>,
}
