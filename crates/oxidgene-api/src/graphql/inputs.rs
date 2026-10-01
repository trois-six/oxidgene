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

use super::mutation::{patch, patch_id, patch_scalar};
use super::scope::{opt_uuid, uuid, uuids};
use super::types::{
    GqlCalendar, GqlChildType, GqlConfidence, GqlDateQualifier, GqlDocumentCategory, GqlEventType,
    GqlGeneanetMediaFidelity, GqlMediaFileKind, GqlNameType, GqlPrivacy, GqlSex,
    GqlSourceMediaType, GqlSpouseRole, GqlTreeDefaultPrivacy,
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
    /// Whether entry fields suggest values as the user types.
    pub entry_suggestions: Option<bool>,
    pub name: Option<String>,
    pub description: MaybeUndefined<String>,
    pub sosa_root_person_id: MaybeUndefined<String>,
    pub self_person_id: MaybeUndefined<String>,
}

impl From<CreateTreeInput> for crate::service::tree::NewTree {
    fn from(input: CreateTreeInput) -> Self {
        Self {
            name: input.name,
            description: input.description,
        }
    }
}

impl TryFrom<UpdateTreeInput> for crate::service::tree::TreePatch {
    type Error = Error;

    fn try_from(input: UpdateTreeInput) -> Result<Self> {
        Ok(Self {
            name: input.name,
            description: patch(input.description),
            sosa_root_person_id: patch_id(input.sosa_root_person_id)?,
            self_person_id: patch_id(input.self_person_id)?,
            default_privacy: input.default_privacy.map(Into::into),
            entry_suggestions: input.entry_suggestions,
        })
    }
}

// ── Person merge input ───────────────────────────────────────────────

/// What the comparison of a merge chose (`docs/api.md`, merge). Every field
/// defaults to "as the kept person has it, everything else moves".
#[derive(Debug, Default, InputObject)]
pub struct MergeChoicesInput {
    /// Own events of either person left out of the merged record.
    #[graphql(default)]
    pub left_out_events: Vec<ID>,
    /// The duplicate's direct media links not taken.
    #[graphql(default)]
    pub left_out_media_links: Vec<ID>,
    #[graphql(default)]
    pub surname_from_duplicate: bool,
    #[graphql(default)]
    pub given_names_from_duplicate: bool,
    #[graphql(default)]
    pub sex_from_duplicate: bool,
}

impl MergeChoicesInput {
    pub fn into_choices(self) -> Result<crate::service::duplicates::MergeChoices> {
        Ok(crate::service::duplicates::MergeChoices {
            left_out_events: uuids(&self.left_out_events)?,
            left_out_media_links: uuids(&self.left_out_media_links)?,
            surname_from_duplicate: self.surname_from_duplicate,
            given_names_from_duplicate: self.given_names_from_duplicate,
            sex_from_duplicate: self.sex_from_duplicate,
        })
    }
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
    /// Which bytes to keep per medium. `RENDITIONS`, the default, ignores
    /// `depositSizes` and `archivePaths`.
    #[graphql(default)]
    pub media_fidelity: GqlGeneanetMediaFidelity,
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
    /// Which bytes to keep per medium. `RENDITIONS`, the default, ignores
    /// `depositSizes` and `archivePaths`.
    #[graphql(default)]
    pub media_fidelity: GqlGeneanetMediaFidelity,
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

impl From<CreatePersonInput> for crate::service::person::NewPerson {
    fn from(input: CreatePersonInput) -> Self {
        Self {
            sex: input.sex.into(),
        }
    }
}

impl From<UpdatePersonInput> for crate::service::person::PersonPatch {
    fn from(input: UpdatePersonInput) -> Self {
        Self {
            sex: input.sex.map(Into::into),
            privacy: input.privacy.map(Into::into),
        }
    }
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

impl From<PersonNameInput> for crate::service::person_name::NewPersonName {
    fn from(input: PersonNameInput) -> Self {
        Self {
            name_type: input.name_type.into(),
            given_names: input.given_names,
            surname: input.surname,
            surname_prefix: input.surname_prefix,
            prefix: input.prefix,
            suffix: input.suffix,
            nickname: input.nickname,
            is_primary: input.is_primary,
            sort_order: input.sort_order.unwrap_or(0),
        }
    }
}

impl From<UpdatePersonNameInput> for crate::service::person_name::PersonNamePatch {
    fn from(input: UpdatePersonNameInput) -> Self {
        Self {
            name_type: input.name_type.map(Into::into),
            given_names: patch(input.given_names),
            surname: patch(input.surname),
            surname_prefix: patch(input.surname_prefix),
            prefix: patch(input.prefix),
            suffix: patch(input.suffix),
            nickname: patch(input.nickname),
            is_primary: input.is_primary,
            sort_order: input.sort_order,
        }
    }
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

impl From<UpdateFamilyInput> for crate::service::family::FamilyPatch {
    fn from(input: UpdateFamilyInput) -> Self {
        Self {
            privacy: input.privacy.map(Into::into),
        }
    }
}

impl TryFrom<AddSpouseInput> for crate::service::family::NewSpouse {
    type Error = Error;

    fn try_from(input: AddSpouseInput) -> Result<Self> {
        Ok(Self {
            person_id: uuid(&input.person_id)?,
            role: input.role.into(),
            sort_order: input.sort_order,
        })
    }
}

impl TryFrom<AddChildInput> for crate::service::family::NewChild {
    type Error = Error;

    fn try_from(input: AddChildInput) -> Result<Self> {
        Ok(Self {
            person_id: uuid(&input.person_id)?,
            child_type: input.child_type.into(),
            sort_order: input.sort_order,
        })
    }
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

impl TryFrom<CreateEventInput> for crate::service::event::NewEvent {
    type Error = Error;

    fn try_from(input: CreateEventInput) -> Result<Self> {
        Ok(Self {
            event_type: input.event_type.into(),
            date_value: input.date_value,
            date_qualifier: input.date_qualifier.map(Into::into).unwrap_or_default(),
            date_value2: input.date_value2,
            calendar: input.calendar.map(Into::into).unwrap_or_default(),
            cause: input.cause,
            place_id: opt_uuid(input.place_id)?,
            person_id: opt_uuid(input.person_id)?,
            family_id: opt_uuid(input.family_id)?,
            description: input.description,
        })
    }
}

impl TryFrom<UpdateEventInput> for crate::service::event::EventPatch {
    type Error = Error;

    fn try_from(input: UpdateEventInput) -> Result<Self> {
        Ok(Self {
            event_type: input.event_type.map(Into::into),
            date_value: patch(input.date_value),
            date_qualifier: patch_scalar(input.date_qualifier),
            date_value2: patch(input.date_value2),
            calendar: patch_scalar(input.calendar),
            cause: patch(input.cause),
            place_id: patch_id(input.place_id)?,
            description: patch(input.description),
        })
    }
}

impl TryFrom<AddEventWitnessInput> for crate::service::event::NewWitness {
    type Error = Error;

    fn try_from(input: AddEventWitnessInput) -> Result<Self> {
        Ok(Self {
            person_id: uuid(&input.person_id)?,
            relation: input.relation,
            sort_order: input.sort_order,
        })
    }
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

impl From<CreatePlaceInput> for crate::service::place::NewPlace {
    fn from(input: CreatePlaceInput) -> Self {
        Self {
            name: input.name,
            latitude: input.latitude,
            longitude: input.longitude,
        }
    }
}

impl From<UpdatePlaceInput> for crate::service::place::PlacePatch {
    fn from(input: UpdatePlaceInput) -> Self {
        Self {
            name: input.name,
            latitude: patch(input.latitude),
            longitude: patch(input.longitude),
        }
    }
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

impl From<CreateSourceInput> for crate::service::source::NewSource {
    fn from(input: CreateSourceInput) -> Self {
        Self {
            title: input.title,
            author: input.author,
            publisher: input.publisher,
            abbreviation: input.abbreviation,
            repository_name: input.repository_name,
        }
    }
}

impl From<UpdateSourceInput> for crate::service::source::SourcePatch {
    fn from(input: UpdateSourceInput) -> Self {
        Self {
            title: input.title,
            author: patch(input.author),
            publisher: patch(input.publisher),
            abbreviation: patch(input.abbreviation),
            repository_name: patch(input.repository_name),
        }
    }
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
    /// The document this becomes a page of. Bytes and URLs live on pages, and
    /// a page always belongs to a document; create one with
    /// `createMediaDocument` first.
    pub document_id: String,
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
    /// The document the file becomes a page of. Ignored when `mediaId` names
    /// an existing page to fill in.
    pub document_id: Option<String>,
    pub file_name: String,
    /// Base64-encoded file content.
    pub content_base64: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Attach the bytes to an existing page instead of creating one.
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
    /// The picture's pixel size, sent together or not at all. Accepted only
    /// for a page we do not hold: we never fetch a remote file, so the client
    /// that displayed it is the only witness to how big it is.
    pub width: Option<i32>,
    pub height: Option<i32>,
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

/// Input for the dictionary's family-name rename.
///
/// Every person whose primary name carries surname `value` (as listed, matched
/// exactly) gets `new_value`, stored as sent. `particle` chooses where
/// `new_value` splits and must be at its head; absent, the split `new_value`
/// already has in the tree is kept, or detected when it is new.
#[derive(Debug, InputObject)]
pub struct RenameFamilyNameInput {
    pub value: String,
    pub new_value: String,
    pub particle: Option<String>,
}

/// Where a held picture lives, as an input. Mirrors `ImageSource`: `kind`
/// selects which of the payload fields is meaningful.
#[derive(async_graphql::InputObject)]
pub struct ImageSourceInput {
    pub kind: super::types::GqlImageSourceKind,
    pub url: Option<String>,
    pub media_id: Option<async_graphql::ID>,
    pub vignette_id: Option<async_graphql::ID>,
}

impl TryFrom<ImageSourceInput> for oxidgene_core::types::ImageSource {
    type Error = async_graphql::Error;

    fn try_from(input: ImageSourceInput) -> Result<Self, Self::Error> {
        use super::types::GqlImageSourceKind;
        let missing = |field: &str| async_graphql::Error::new(format!("{field} is required"));
        Ok(match input.kind {
            GqlImageSourceKind::Remote => Self::Remote {
                url: input.url.ok_or_else(|| missing("url"))?,
            },
            GqlImageSourceKind::Thumbnail => Self::Thumbnail {
                media_id: uuid(input.media_id.ok_or_else(|| missing("mediaId"))?)?,
            },
            GqlImageSourceKind::Crop => Self::Crop {
                vignette_id: uuid(input.vignette_id.ok_or_else(|| missing("vignetteId"))?)?,
            },
        })
    }
}

// ── Media library ────────────────────────────────────────────────────

/// Filters of `mediaList`. Every field is optional and the set ones combine
/// with AND; blank text is no filter.
#[derive(Debug, Default, InputObject)]
pub struct MediaListFilterInput {
    /// Tags in any spelling, matched case-insensitively: a document must
    /// carry all of them.
    #[graphql(default)]
    pub tags: Vec<String>,
    /// Documents with at least one page of this file kind.
    pub kind: Option<GqlMediaFileKind>,
    pub category: Option<GqlDocumentCategory>,
    /// Substring of the title or a file name, case- and accent-insensitive.
    pub name: Option<String>,
    /// Substring of a connected person's name, case- and accent-insensitive.
    pub linked_name: Option<String>,
    /// Earliest year, inclusive, of a linked event.
    pub event_from: Option<i32>,
    /// Latest year, inclusive, of a linked event.
    pub event_to: Option<i32>,
    /// Earliest day, inclusive and in UTC, the document was added.
    pub added_from: Option<chrono::NaiveDate>,
    /// Latest day, inclusive and in UTC, the document was added.
    pub added_to: Option<chrono::NaiveDate>,
}

impl From<MediaListFilterInput> for crate::service::media_library::MediaListFilters {
    fn from(input: MediaListFilterInput) -> Self {
        Self {
            tags: input.tags,
            kind: input.kind.map(Into::into),
            category: input.category.map(Into::into),
            name: input.name,
            linked_name: input.linked_name,
            event_from: input.event_from,
            event_to: input.event_to,
            added_from: input.added_from,
            added_to: input.added_to,
        }
    }
}
