//! GraphQL input types for mutations.

use async_graphql::InputObject;

use super::types::{GqlChildType, GqlConfidence, GqlEventType, GqlNameType, GqlSex, GqlSpouseRole};

// ── Tree Inputs ──────────────────────────────────────────────────────

/// Input for creating a new tree.
#[derive(Debug, InputObject)]
pub struct CreateTreeInput {
    pub name: String,
    pub description: Option<String>,
}

/// Input for updating an existing tree.
#[derive(Debug, InputObject)]
pub struct UpdateTreeInput {
    pub name: Option<String>,
    pub description: Option<String>,
}

// ── Person Inputs ────────────────────────────────────────────────────

/// Input for creating a new person.
#[derive(Debug, InputObject)]
pub struct CreatePersonInput {
    pub sex: GqlSex,
}

/// Input for updating a person.
#[derive(Debug, InputObject)]
pub struct UpdatePersonInput {
    pub sex: Option<GqlSex>,
}

// ── PersonName Inputs ────────────────────────────────────────────────

/// Input for adding or updating a person name.
#[derive(Debug, InputObject)]
pub struct PersonNameInput {
    pub name_type: GqlNameType,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
}

/// Input for updating a person name (all fields optional except id).
#[derive(Debug, InputObject)]
pub struct UpdatePersonNameInput {
    pub name_type: Option<GqlNameType>,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: Option<bool>,
}

// ── Family Inputs ────────────────────────────────────────────────────

// Family has no extra fields beyond tree_id, so create doesn't need an input.
// Update just touches updated_at, so no input needed either.

// ── FamilySpouse / FamilyChild Inputs ────────────────────────────────

/// Input for adding a spouse to a family.
#[derive(Debug, InputObject)]
pub struct AddSpouseInput {
    pub person_id: String,
    pub role: GqlSpouseRole,
    #[graphql(default)]
    pub sort_order: i32,
}

/// Input for adding a child to a family.
#[derive(Debug, InputObject)]
pub struct AddChildInput {
    pub person_id: String,
    pub child_type: GqlChildType,
    #[graphql(default)]
    pub sort_order: i32,
}

// ── Event Inputs ─────────────────────────────────────────────────────

/// Input for creating an event.
#[derive(Debug, InputObject)]
pub struct CreateEventInput {
    pub event_type: GqlEventType,
    pub date_value: Option<String>,
    /// Date for sorting, in YYYY-MM-DD format.
    pub date_sort: Option<String>,
    pub place_id: Option<String>,
    pub person_id: Option<String>,
    pub family_id: Option<String>,
    pub description: Option<String>,
}

/// Input for updating an event.
#[derive(Debug, InputObject)]
pub struct UpdateEventInput {
    pub event_type: Option<GqlEventType>,
    pub date_value: Option<String>,
    /// Date for sorting, in YYYY-MM-DD format.
    pub date_sort: Option<String>,
    pub place_id: Option<String>,
    pub description: Option<String>,
}

// ── Place Inputs ─────────────────────────────────────────────────────

/// Input for creating a place.
#[derive(Debug, InputObject)]
pub struct CreatePlaceInput {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

/// Input for updating a place.
#[derive(Debug, InputObject)]
pub struct UpdatePlaceInput {
    pub name: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

// ── Source Inputs ────────────────────────────────────────────────────

/// Input for creating a source.
#[derive(Debug, InputObject)]
pub struct CreateSourceInput {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
}

/// Input for updating a source.
#[derive(Debug, InputObject)]
pub struct UpdateSourceInput {
    pub title: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
}

// ── Citation Inputs ──────────────────────────────────────────────────

/// Input for creating a citation.
#[derive(Debug, InputObject)]
pub struct CreateCitationInput {
    pub source_id: String,
    pub person_id: Option<String>,
    pub event_id: Option<String>,
    pub family_id: Option<String>,
    pub page: Option<String>,
    pub confidence: GqlConfidence,
    pub text: Option<String>,
}

/// Input for updating a citation.
#[derive(Debug, InputObject)]
pub struct UpdateCitationInput {
    pub page: Option<String>,
    pub confidence: Option<GqlConfidence>,
    pub text: Option<String>,
}

// ── Media Inputs ─────────────────────────────────────────────────────

/// Input for uploading media metadata (no actual file upload in MVP).
#[derive(Debug, InputObject)]
pub struct UploadMediaInput {
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Input for updating media metadata.
#[derive(Debug, InputObject)]
pub struct UpdateMediaInput {
    pub title: Option<String>,
    pub description: Option<String>,
}

// ── MediaLink Inputs ─────────────────────────────────────────────────

/// Input for creating a media link.
#[derive(Debug, InputObject)]
pub struct CreateMediaLinkInput {
    pub media_id: String,
    pub person_id: Option<String>,
    pub event_id: Option<String>,
    pub source_id: Option<String>,
    pub family_id: Option<String>,
    #[graphql(default)]
    pub sort_order: i32,
}

// ── Note Inputs ──────────────────────────────────────────────────────

/// Input for creating a note.
#[derive(Debug, InputObject)]
pub struct CreateNoteInput {
    pub text: String,
    pub person_id: Option<String>,
    pub event_id: Option<String>,
    pub family_id: Option<String>,
    pub source_id: Option<String>,
}

/// Input for updating a note.
#[derive(Debug, InputObject)]
pub struct UpdateNoteInput {
    pub text: Option<String>,
}
