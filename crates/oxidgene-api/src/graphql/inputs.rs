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

use async_graphql::{Error, ID, InputObject, MaybeUndefined, Result};
use std::collections::HashMap;

use super::types::{
    GqlCalendar, GqlChildType, GqlConfidence, GqlDateQualifier, GqlDocumentCategory, GqlEventType,
    GqlNameType, GqlPrivacy, GqlSex, GqlSourceMediaType, GqlSpouseRole, GqlTreeDefaultPrivacy,
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
    /// What `Default` privacy resolves to for everything in this tree.
    pub default_privacy: Option<GqlTreeDefaultPrivacy>,
    pub name: Option<String>,
    pub description: MaybeUndefined<String>,
    pub sosa_root_person_id: MaybeUndefined<String>,
    pub self_person_id: MaybeUndefined<String>,
}

// ── Geneanet import wizard inputs ───────────────────────────────────

/// One deposit's byte size, collected by the desktop login window.
#[derive(Debug, InputObject)]
pub struct GeneanetDepositSizeInput {
    pub deposit_id: i64,
    pub size: i64,
}

/// A string-keyed path entry used for locally staged media.
#[derive(Debug, InputObject)]
pub struct GeneanetMediaPathInput {
    pub url: String,
    pub path: String,
}

/// Shared inputs for Geneanet preview and fetch planning.
#[derive(Debug, InputObject)]
pub struct GeneanetPreviewInput {
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    #[graphql(default)]
    pub deposit_sizes: Vec<GeneanetDepositSizeInput>,
    #[graphql(default)]
    pub archive_paths: Vec<String>,
}

/// Session content to encode as a downloadable Geneanet archive.
#[derive(Debug, InputObject)]
pub struct GeneanetSessionEncodeInput {
    pub collection: String,
    #[graphql(default)]
    pub deposit_sizes: Vec<GeneanetDepositSizeInput>,
    pub account: Option<String>,
    #[graphql(default)]
    pub media: Vec<GeneanetMediaPathInput>,
}

/// Inputs needed to import a Geneanet tree and its already fetched media.
#[derive(Debug, InputObject)]
pub struct GeneanetImportInput {
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    #[graphql(default)]
    pub deposit_sizes: Vec<GeneanetDepositSizeInput>,
    #[graphql(default)]
    pub archive_paths: Vec<String>,
    #[graphql(default)]
    pub fetched: Vec<GeneanetMediaPathInput>,
    pub progress_id: Option<String>,
}

pub(crate) fn geneanet_deposit_sizes(
    entries: &[GeneanetDepositSizeInput],
) -> Result<HashMap<i64, u64>> {
    entries
        .iter()
        .map(|entry| {
            u64::try_from(entry.size)
                .map(|size| (entry.deposit_id, size))
                .map_err(|_| Error::new("Geneanet deposit sizes cannot be negative"))
        })
        .collect()
}

pub(crate) fn geneanet_media_paths(entries: &[GeneanetMediaPathInput]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|entry| (entry.url.clone(), entry.path.clone()))
        .collect()
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

/// Input for updating a family.
#[derive(Debug, InputObject)]
pub struct UpdateFamilyInput {
    pub privacy: Option<GqlPrivacy>,
}

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

/// Input for recording a media file we do not hold the bytes of.
///
/// The metadata-only path, mirroring `POST /trees/{id}/media`. To send actual
/// bytes, use `uploadMediaFile`.
#[derive(Debug, InputObject)]
pub struct UploadMediaInput {
    pub file_name: String,
    pub mime_type: String,
    pub file_path: String,
    pub file_size: i64,
    pub title: Option<String>,
    pub description: Option<String>,
}

/// Input for uploading a file's actual bytes.
///
/// The content travels base64-encoded in the request body, the same choice the
/// GEDCOM and GeneWeb import mutations make: adding the `Upload` scalar would
/// mean multipart GraphQL requests, a transport every client would then have
/// to special-case for one field. REST's `POST .../media/upload` is the
/// efficient path and is what the UI uses; this exists so no operation is
/// reachable from only one of the two APIs.
///
/// Base64 inflates the payload by a third, so the effective size ceiling here
/// is correspondingly lower than REST's.
#[derive(Debug, InputObject)]
pub struct UploadMediaFileInput {
    pub file_name: String,
    /// Base64-encoded file content.
    pub content_base64: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Attach the bytes to an existing record instead of creating one.
    pub media_id: Option<String>,
}

/// Input for updating media metadata.
///
/// A media carries the same descriptive fields a fact does. There is no source
/// field on purpose: a media *is* a source document. `dateSort` is absent
/// because the server derives it, exactly as it does for an event.
#[derive(Debug, InputObject)]
pub struct UpdateMediaInput {
    pub title: MaybeUndefined<String>,
    pub description: MaybeUndefined<String>,
    pub date_value: MaybeUndefined<String>,
    pub date_value2: MaybeUndefined<String>,
    pub date_qualifier: Option<GqlDateQualifier>,
    pub calendar: Option<GqlCalendar>,
    pub place_id: MaybeUndefined<String>,
    /// The URL of a remote media. Refused for a media whose bytes we hold.
    pub file_path: Option<String>,
    pub mime_type: Option<String>,
    /// Whether this is shown when the tree is published.
    pub privacy: Option<GqlPrivacy>,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    pub source_media_type: Option<GqlSourceMediaType>,
    /// What kind of record it is. Setting it without a `sourceMediaType` also
    /// sets the medium it implies, so a census return does not export as
    /// `OTHER`.
    pub document_category: MaybeUndefined<GqlDocumentCategory>,
}

// ── Vignette Inputs ──────────────────────────────────────────────────

/// Input for cropping a region out of a media file.
#[derive(Debug, InputObject)]
pub struct CreateVignetteInput {
    pub media_id: String,
    /// Zero-based page of a multi-page document; defaults to 0.
    #[graphql(default)]
    pub page: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub person_id: Option<String>,
    pub event_id: Option<String>,
}

/// Input for moving or re-attributing a vignette.
///
/// The four rectangle fields travel together: send all of them or none.
#[derive(Debug, InputObject)]
pub struct UpdateVignetteInput {
    pub page: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub person_id: MaybeUndefined<String>,
    pub event_id: MaybeUndefined<String>,
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
    /// The media this note is about — distinct from the media's own
    /// description, which is the caption shown under its tile.
    pub media_id: Option<String>,
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
