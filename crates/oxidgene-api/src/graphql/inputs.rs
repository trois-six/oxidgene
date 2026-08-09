//! GraphQL input types for mutations.
//!
//! Nullable fields on `Update*` inputs are [`MaybeUndefined`], not `Option`.
//! A plain `Option<T>` cannot tell an omitted field from an explicit `null` —
//! both arrive as `None` — so those fields could never be *cleared*, only set:
//! the mutation was accepted and the old value silently kept. `MaybeUndefined`
//! keeps the three cases apart, and [`super::mutation::patch`] maps it onto the
//! repositories' `Option<Option<T>>` patch convention. This mirrors the
//! `double_option` deserializer on the REST side, so both surfaces behave
//! identically.

use async_graphql::{ID, InputObject, MaybeUndefined};

use super::types::{
    GqlCalendar, GqlChildType, GqlConfidence, GqlDateQualifier, GqlEventType, GqlNameType,
    GqlPrivacy, GqlSex, GqlSpouseRole,
};

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
    pub description: MaybeUndefined<String>,
    pub sosa_root_person_id: MaybeUndefined<String>,
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
    pub privacy: Option<GqlPrivacy>,
}

// ── PersonName Inputs ────────────────────────────────────────────────

/// Input for adding or updating a person name.
#[derive(Debug, InputObject)]
pub struct PersonNameInput {
    pub name_type: GqlNameType,
    pub given_names: Option<String>,
    /// The surname root, particle excluded.
    ///
    /// Stored verbatim: the server does not detect a particle hiding in it.
    /// Callers holding a full surname should split it with
    /// `oxidgene_core::types::split_surname_particle` first, as the UI does.
    pub surname: Option<String>,
    /// The surname particle, GEDCOM `SPFX` ("de la", "van der").
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    pub sort_order: Option<i32>,
}

/// Input for updating a person name (all fields optional except id).
#[derive(Debug, InputObject)]
pub struct UpdatePersonNameInput {
    pub name_type: Option<GqlNameType>,
    pub given_names: MaybeUndefined<String>,
    pub surname: MaybeUndefined<String>,
    pub surname_prefix: MaybeUndefined<String>,
    pub prefix: MaybeUndefined<String>,
    pub suffix: MaybeUndefined<String>,
    pub nickname: MaybeUndefined<String>,
    pub is_primary: Option<bool>,
    pub sort_order: Option<i32>,
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
    pub date_qualifier: Option<GqlDateQualifier>,
    pub date_value2: Option<String>,
    pub calendar: Option<GqlCalendar>,
    pub cause: Option<String>,
    pub place_id: Option<String>,
    pub person_id: Option<String>,
    pub family_id: Option<String>,
    pub description: Option<String>,
}

/// Input for updating an event.
#[derive(Debug, InputObject)]
pub struct UpdateEventInput {
    pub event_type: Option<GqlEventType>,
    pub date_value: MaybeUndefined<String>,
    /// Date for sorting, in YYYY-MM-DD format.
    pub date_sort: MaybeUndefined<String>,
    pub date_qualifier: MaybeUndefined<GqlDateQualifier>,
    pub date_value2: MaybeUndefined<String>,
    pub calendar: MaybeUndefined<GqlCalendar>,
    pub cause: MaybeUndefined<String>,
    pub place_id: MaybeUndefined<String>,
    pub description: MaybeUndefined<String>,
}

/// Input for adding a witness to an event.
#[derive(Debug, InputObject)]
pub struct AddEventWitnessInput {
    pub person_id: String,
    pub relation: Option<String>,
    #[graphql(default)]
    pub sort_order: i32,
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
    pub latitude: MaybeUndefined<f64>,
    pub longitude: MaybeUndefined<f64>,
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
    pub author: MaybeUndefined<String>,
    pub publisher: MaybeUndefined<String>,
    pub abbreviation: MaybeUndefined<String>,
    pub repository_name: MaybeUndefined<String>,
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
    /// Repoints the citation at another source.
    pub source_id: Option<ID>,
    pub page: MaybeUndefined<String>,
    pub confidence: Option<GqlConfidence>,
    pub text: MaybeUndefined<String>,
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
    pub title: MaybeUndefined<String>,
    pub description: MaybeUndefined<String>,
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

// ── Import Inputs ────────────────────────────────────────────────────

/// Input for the dictionary's bulk surname-particle edit.
///
/// `value` is a surname as listed by the dictionary, particle included;
/// `particle` is the new cut to apply to every occurrence of it, empty meaning
/// "this name has no particle". The particle must already be at the head of
/// `value` — this edit moves a boundary, it never adds a word.
#[derive(Debug, InputObject)]
pub struct SetFamilyNameParticleInput {
    pub value: String,
    pub particle: String,
}

/// Input for importing a GEDCOM string.
#[derive(Debug, InputObject)]
pub struct ImportGedcomInput {
    /// The raw GEDCOM string content.
    pub gedcom: String,
}

/// Input for importing a GeneWeb `.gw` file.
#[derive(Debug, InputObject)]
pub struct ImportGenewebInput {
    /// The raw file content, base64-encoded.
    ///
    /// `.gw` is ISO-8859-1 unless the file opts into UTF-8 with an `encoding:`
    /// directive, so its bytes cannot travel as a GraphQL `String` — those are
    /// UTF-8 by definition and a Latin-1 file would arrive mangled. The REST
    /// endpoint takes the same bytes raw, without this encoding step.
    pub content_base64: String,

    /// Name of the file being imported. GeneWeb records it on every family and
    /// it is echoed back in parse warnings; defaults to `import.gw`.
    pub filename: Option<String>,
}
