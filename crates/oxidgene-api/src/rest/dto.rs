//! Request/response DTOs for REST endpoints.

use oxidgene_core::types::{Place, Source};
use oxidgene_core::{
    Calendar, ChildType, Confidence, DateQualifier, EventType, NameType, Privacy, Sex, SpouseRole,
};
use serde::{Deserialize, Serialize};

/// Deserializer for update fields that must tell "absent" from `null`.
///
/// serde maps a JSON `null` to `None` for *any* `Option`, so a plain
/// `Option<Option<T>>` collapses `{"x": null}` and `{}` to the same `None` —
/// which these DTOs read as "leave unchanged". The effect was that no nullable
/// field could ever be cleared over REST: the request was accepted and the old
/// value silently kept. Paired with `#[serde(default)]` this restores the
/// distinction — absent stays `None`, `null` becomes `Some(None)`.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// ── Pagination query params ──────────────────────────────────────────

/// Query parameters for cursor-based pagination.
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    /// Number of items to return (default: 25, max: 100).
    pub first: Option<u64>,
    /// Cursor to start after (UUID string).
    pub after: Option<String>,
}

/// People whose display-ready portraits should be loaded together.
#[derive(Debug, Deserialize)]
pub struct PortraitImagesRequest {
    pub person_ids: Vec<uuid::Uuid>,
}

/// Person and family relations whose display labels should be loaded together.
#[derive(Debug, Deserialize)]
pub struct RelationLabelsRequest {
    pub person_ids: Vec<uuid::Uuid>,
    pub family_ids: Vec<uuid::Uuid>,
}

/// Media tiles and vignettes whose gallery data should be loaded together.
#[derive(Debug, Deserialize)]
pub struct GalleryBundleRequest {
    pub media_ids: Vec<uuid::Uuid>,
    pub vignette_ids: Vec<uuid::Uuid>,
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
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    /// `null` clears the root person; absent field leaves it unchanged.
    #[serde(default, deserialize_with = "double_option")]
    pub sosa_root_person_id: Option<Option<uuid::Uuid>>,
    /// `null` clears the person identifying the current user; absent leaves it unchanged.
    #[serde(default, deserialize_with = "double_option")]
    pub self_person_id: Option<Option<uuid::Uuid>>,
    /// What `privacy: "default"` resolves to for everything in this tree.
    pub default_privacy: Option<oxidgene_core::enums::TreeDefaultPrivacy>,
}

/// Request body for duplicating a tree.
#[derive(Debug, Deserialize)]
pub struct DuplicateTreeRequest {
    pub name: String,
}

// ── Person DTOs ──────────────────────────────────────────────────────

/// Query parameters for free-text person search (Sprint E.6).
///
/// The search goes through the `person_search_fts` table (accent-folded,
/// all words must match) and returns a paginated `SearchResult`. An empty
/// or missing `q` lists all persons sorted by name (browse mode).
#[derive(Debug, Deserialize)]
pub struct PersonSearchQuery {
    /// Free-text query.
    pub q: Option<String>,
    /// Maximum results to return (default: 25, max: 100).
    pub limit: Option<usize>,
    /// Offset for pagination (default: 0).
    pub offset: Option<usize>,
    pub sex: Option<Sex>,
    pub surname: Option<String>,
    pub given_names: Option<String>,
    pub occupation: Option<String>,
    pub spouse_surname: Option<String>,
    pub spouse_given_names: Option<String>,
    pub father_surname: Option<String>,
    pub father_given_names: Option<String>,
    pub mother_surname: Option<String>,
    pub mother_given_names: Option<String>,
    pub birth_from: Option<i32>,
    pub birth_to: Option<i32>,
    pub death_from: Option<i32>,
    pub death_to: Option<i32>,
    pub place: Option<String>,
    pub event_type: Option<EventType>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    #[serde(default)]
    pub has_media: bool,
    #[serde(default)]
    pub sort: PersonSearchSortQuery,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonSearchSortQuery {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

impl From<PersonSearchSortQuery> for oxidgene_db::repo::PersonSearchSort {
    fn from(value: PersonSearchSortQuery) -> Self {
        match value {
            PersonSearchSortQuery::Relevance => Self::Relevance,
            PersonSearchSortQuery::NameAsc => Self::NameAsc,
            PersonSearchSortQuery::NameDesc => Self::NameDesc,
            PersonSearchSortQuery::BirthAsc => Self::BirthAsc,
            PersonSearchSortQuery::BirthDesc => Self::BirthDesc,
        }
    }
}

/// Response for GET /api/v1/trees/:tree_id/persons/:person_id.
/// Wraps the core `Person` with the server-computed SOSA number.
#[derive(Debug, Serialize)]
pub struct PersonDetailResponse {
    #[serde(flatten)]
    pub person: oxidgene_core::types::Person,
    pub sosa_number: Option<u64>,
}

/// Request body for creating a person.
#[derive(Debug, Deserialize)]
pub struct CreatePersonRequest {
    pub sex: Sex,
}

/// Request body for updating a person.
#[derive(Debug, Deserialize)]
pub struct UpdatePersonRequest {
    pub sex: Option<Sex>,
    pub privacy: Option<Privacy>,
}

// ── PersonName DTOs ──────────────────────────────────────────────────

/// Request body for creating a person name.
#[derive(Debug, Deserialize)]
pub struct CreatePersonNameRequest {
    pub name_type: NameType,
    pub given_names: Option<String>,
    /// The surname root, particle excluded.
    ///
    /// The server stores this verbatim — it does not try to detect a particle
    /// hiding in it. Callers that hold a full surname should split it with
    /// `oxidgene_core::types::split_surname_particle` first, as the UI does.
    pub surname: Option<String>,
    /// The surname particle, GEDCOM `SPFX` ("de la", "van der").
    #[serde(default)]
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    #[serde(default)]
    pub sort_order: i32,
}

/// Request body for updating a person name.
#[derive(Debug, Deserialize)]
pub struct UpdatePersonNameRequest {
    pub name_type: Option<NameType>,
    #[serde(default, deserialize_with = "double_option")]
    pub given_names: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub surname: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub surname_prefix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub prefix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub suffix: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub nickname: Option<Option<String>>,
    pub is_primary: Option<bool>,
    #[serde(default)]
    pub sort_order: Option<i32>,
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
    #[serde(default)]
    pub date_qualifier: DateQualifier,
    #[serde(default)]
    pub date_value2: Option<String>,
    #[serde(default)]
    pub calendar: Calendar,
    #[serde(default)]
    pub cause: Option<String>,
    pub place_id: Option<uuid::Uuid>,
    pub person_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub description: Option<String>,
}

/// Request body for updating an event.
#[derive(Debug, Deserialize)]
pub struct UpdateEventRequest {
    pub event_type: Option<EventType>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value: Option<Option<String>>,
    pub date_qualifier: Option<DateQualifier>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value2: Option<Option<String>>,
    pub calendar: Option<Calendar>,
    #[serde(default, deserialize_with = "double_option")]
    pub cause: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub place_id: Option<Option<uuid::Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
}

// ── EventWitness DTOs ────────────────────────────────────────────────

/// Request body for adding a witness to an event.
#[derive(Debug, Deserialize)]
pub struct AddEventWitnessRequest {
    pub person_id: uuid::Uuid,
    pub relation: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
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
    #[serde(default, deserialize_with = "double_option")]
    pub latitude: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
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
    #[serde(default, deserialize_with = "double_option")]
    pub author: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub publisher: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub abbreviation: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
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
    /// Repoints the citation at another source.
    pub source_id: Option<uuid::Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    pub page: Option<Option<String>>,
    pub confidence: Option<Confidence>,
    #[serde(default, deserialize_with = "double_option")]
    pub text: Option<Option<String>>,
}

/// Query parameters for deleting a source.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteSourceQuery {
    /// Keep the source if any citation, note or media link still points at
    /// it. Answered by the status code: `204` deleted, `200` kept.
    #[serde(default)]
    pub only_if_unused: bool,
}

/// Query parameters for definitively deleting a media record.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteMediaQuery {
    /// Keep the media when anything other than this gallery link references
    /// it. The status says the result: `204` deleted, `200` retained.
    #[serde(default)]
    pub only_if_unreferenced_elsewhere: bool,
    pub allowed_link_id: Option<uuid::Uuid>,
}

/// Query parameters for checking whether a profile-gallery media can be
/// permanently deleted without affecting another record.
#[derive(Debug, Deserialize)]
pub struct MediaDeletionStatusQuery {
    pub allowed_link_id: uuid::Uuid,
}

/// Query parameters for listing citations by entity.
#[derive(Debug, Deserialize)]
pub struct CitationListQuery {
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
    pub first: Option<u64>,
    pub after: Option<String>,
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
///
/// A media carries the same descriptive fields a fact does — a date with its
/// qualifier and calendar, a place, a description — because "a photograph taken
/// around 1890 at Nantes" is the same kind of statement as an event. There is
/// deliberately **no source field**: a media *is* a source document, and asking
/// which source backs a scan of a parish register asks it to cite itself.
///
/// `date_sort` is absent on purpose: the server derives it from `calendar` +
/// `date_value`, exactly as it does for an event.
#[derive(Debug, Deserialize)]
pub struct UpdateMediaRequest {
    #[serde(default, deserialize_with = "double_option")]
    pub title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub date_value2: Option<Option<String>>,
    pub date_qualifier: Option<DateQualifier>,
    pub calendar: Option<Calendar>,
    #[serde(default, deserialize_with = "double_option")]
    pub place_id: Option<Option<uuid::Uuid>>,
    /// Where the file is. For a remote media this is the URL, and editing it is
    /// how a broken link gets fixed. Ignored for a media whose bytes we hold —
    /// there `file_path` is the GEDCOM value an export writes back, and
    /// repointing it would make the export lie about a file we are serving.
    pub file_path: Option<String>,
    /// Only meaningful alongside a `file_path` we cannot sniff. Left out, the
    /// server guesses from the URL's extension.
    pub mime_type: Option<String>,
    /// Whether this is shown when the tree is published. Recorded now,
    /// enforced when authentication lands.
    pub privacy: Option<oxidgene_core::enums::Privacy>,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    pub source_media_type: Option<oxidgene_core::enums::SourceMediaType>,
    /// What kind of record it is. Clearing it is meaningful — a scan can stop
    /// being classified — so this distinguishes "absent" from "set to null".
    /// Setting it without a `source_media_type` also sets the medium it
    /// implies, so a census return does not export as `OTHER`.
    #[serde(default, deserialize_with = "double_option")]
    pub document_category: Option<Option<oxidgene_core::enums::DocumentCategory>>,
}

/// One free-form tag to attach to or remove from a media item.
#[derive(Debug, Deserialize)]
pub struct MediaTagRequest {
    pub tag: String,
}

/// Request body for updating a couple.
///
/// Only privacy so far — a family's own facts live on its events and its
/// spouse rows, not on the row itself.
#[derive(Debug, Deserialize)]
pub struct UpdateFamilyRequest {
    pub privacy: Option<oxidgene_core::enums::Privacy>,
}

/// Request body for creating an empty multi-page document.
#[derive(Debug, Deserialize)]
pub struct CreateDocumentRequest {
    pub title: Option<String>,
}

/// Request body for setting a document's page order.
#[derive(Debug, Deserialize)]
pub struct ReorderPagesRequest {
    /// Exactly this document's pages, once each, in the wanted order.
    pub page_ids: Vec<uuid::Uuid>,
}

// ── Vignette DTOs ───────────────────────────────────────────────────

/// Query parameters for listing vignettes by what they are attributed to.
#[derive(Debug, Deserialize)]
pub struct VignetteListQuery {
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
}

/// Request body for cropping a region out of a media file.
#[derive(Debug, Deserialize)]
pub struct CreateVignetteRequest {
    /// Zero-based page of a multi-page document; defaults to `0`.
    pub page: Option<i32>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
}

/// Request body for moving or re-attributing a vignette.
///
/// The four rectangle fields travel together: send all of them or none.
#[derive(Debug, Deserialize)]
pub struct UpdateVignetteRequest {
    pub page: Option<i32>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub person_id: Option<Option<uuid::Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub event_id: Option<Option<uuid::Uuid>>,
}

// ── MediaLink DTOs ──────────────────────────────────────────────────

/// Row returned by the bulk media-links endpoint.
#[derive(Debug, Serialize)]
pub struct MediaLinkListRow {
    pub link_id: uuid::Uuid,
    pub entity_id: uuid::Uuid,
    /// `person` or `event` — which of the link's targets this row is about.
    pub entity_type: String,
    pub media_id: uuid::Uuid,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: String,
    /// Whether a thumbnail was generated; the caller draws an icon otherwise.
    pub has_thumbnail: bool,
}

/// Query parameters for the media-links list, which answers three questions.
///
/// With neither filter it is the tree-wide list the pedigree canvas and the
/// profile timeline read. With `entity_type` + `entity_id` it is one entity's
/// gallery. With `media_id` it is the other direction — everything one file is
/// attached to, which is what lets a media say which events it documents.
#[derive(Debug, Deserialize)]
pub struct MediaLinkListQuery {
    /// `person`, `family`, `event` or `source`.
    pub entity_type: Option<String>,
    pub entity_id: Option<uuid::Uuid>,
    /// Look the other way round: the links of one media.
    pub media_id: Option<uuid::Uuid>,
}

/// Request body for choosing what represents a person.
///
/// At most one of the two may be given. Both absent clears the portrait, which
/// is how "use the silhouette again" is said.
#[derive(Debug, Deserialize)]
pub struct SetPortraitRequest {
    #[serde(default)]
    pub media_id: Option<uuid::Uuid>,
    /// A region of a larger image — a face in a group photograph.
    #[serde(default)]
    pub vignette_id: Option<uuid::Uuid>,
}

impl SetPortraitRequest {
    /// Read the body as one value, refusing the state the model cannot hold.
    pub fn portrait(&self) -> Result<oxidgene_core::types::Portrait, String> {
        match (self.media_id, self.vignette_id) {
            (Some(_), Some(_)) => {
                Err("a portrait is a media or a vignette, never both".to_string())
            }
            (Some(id), None) => Ok(oxidgene_core::types::Portrait::Media(id)),
            (None, Some(id)) => Ok(oxidgene_core::types::Portrait::Vignette(id)),
            (None, None) => Ok(oxidgene_core::types::Portrait::None),
        }
    }
}

/// A media together with the link that attached it — one gallery tile.
#[derive(Debug, Serialize)]
pub struct MediaWithLink {
    pub link_id: uuid::Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: oxidgene_core::types::Media,
}

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
    pub media_id: Option<uuid::Uuid>,
    pub first: Option<u64>,
    pub after: Option<String>,
}

/// Request body for creating a note.
#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub text: String,
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
    /// The media this note is about — distinct from the media's own
    /// description, which is the caption shown under its tile.
    pub media_id: Option<uuid::Uuid>,
}

/// Request body for updating a note.
#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub text: Option<String>,
}

// ── Import / export DTOs ─────────────────────────────────────────────

/// Request body for importing a GEDCOM string.
#[derive(Debug, Deserialize)]
pub struct ImportGedcomRequest {
    pub gedcom: String,
}

/// Query parameters for a GeneWeb `.gw` import.
#[derive(Debug, Deserialize)]
pub struct ImportGenewebQuery {
    /// Name of the uploaded file. GeneWeb records it on every family and it is
    /// echoed back in parse warnings; defaults to `import.gw` when omitted.
    pub filename: Option<String>,
}

/// Response body for an import, whatever the source format.
#[derive(Debug, Clone, Serialize)]
pub struct ImportResponse {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub media_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

/// Parser selected for an uploaded genealogy file.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileImportFormat {
    Gedcom,
    Gedzip,
    Geneweb,
}

impl FileImportFormat {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Gedcom => "gedcom",
            Self::Gedzip => "gedzip",
            Self::Geneweb => "geneweb",
        }
    }
}

/// Metadata for starting an asynchronous file import.
#[derive(Debug, Deserialize)]
pub struct StartFileImportQuery {
    pub format: FileImportFormat,
    /// Used only as GeneWeb's provenance label, never as a temporary path.
    pub filename: Option<String>,
}

/// Operation identifier returned once the upload has reached durable storage.
#[derive(Debug, Serialize)]
pub struct FileImportStartedResponse {
    pub job_id: uuid::Uuid,
}

/// Current server-side state of an asynchronous file import.
#[derive(Debug, Serialize)]
pub struct FileImportStatusResponse {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ImportResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geneanet_result: Option<GeneanetImportResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Options for creating an asynchronous GEDZIP export.
#[derive(Debug, Deserialize)]
pub struct StartExportJobQuery {
    pub merge_occupations: Option<bool>,
    pub merge_names: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ExportJobStartedResponse {
    pub job_id: uuid::Uuid,
}

#[derive(Debug, Serialize)]
pub struct ExportJobStatusResponse {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response body for GEDCOM export.
#[derive(Debug, Serialize)]
pub struct ExportGedcomResponse {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

/// Query parameters for GET /api/v1/trees/:tree_id/gedcom/export.
#[derive(Debug, Deserialize)]
pub struct ExportGedcomQuery {
    /// Export format: `gedcom` (default) or `gedzip`.
    pub format: Option<String>,
    /// Collapse each person's multiple `OCCU` tags back into one
    /// (comma-separated), for importers such as Geneanet that only support
    /// a single profession field. Defaults to `false` (one `OCCU` per
    /// profession, lossless).
    pub merge_occupations: Option<bool>,
    /// Collapse each person's non-primary names into the primary name's
    /// `SURN` tag (comma-separated), for importers such as Geneanet that
    /// only read the first `NAME` structure. Defaults to `false` (one
    /// `NAME` per name, lossless).
    pub merge_names: Option<bool>,
}

// ── Projection DTOs ─────────────────────────────────────────────────

/// Response body for projection rebuild operations.
#[derive(Debug, Serialize)]
pub struct ProfileRebuildResponse {
    pub rebuilt: bool,
    pub persons_count: usize,
}

/// Response body for dropping a tree's projections.
#[derive(Debug, Serialize)]
pub struct ProfileDropResponse {
    pub dropped: bool,
}

/// Query parameters for pedigree assembly.
#[derive(Debug, Deserialize)]
pub struct PedigreeQuery {
    /// Number of ancestor generations to include (e.g. 5).
    pub ancestor_depth: u32,
    /// Number of descendant generations to include (e.g. 3).
    pub descendant_depth: u32,
}

/// Query parameters for pedigree expansion.
#[derive(Debug, Deserialize)]
pub struct PedigreeExpandQuery {
    /// Direction to expand: "ancestors" or "descendants".
    pub direction: String,
    /// Current loaded depth in the expand direction.
    pub from_depth: u32,
    /// Target depth after expansion.
    pub to_depth: u32,
    /// Depth already loaded in the *opposite* direction. Supplied so the
    /// returned `*_depth_loaded` values match what the caller holds — the
    /// server keeps no per-client pedigree state.
    #[serde(default)]
    pub other_depth: u32,
}

// ── Dictionary DTOs ───────────────────────────────────────────────────

/// A distinct value (surname, occupation label) plus its usage count.
#[derive(Debug, Serialize)]
pub struct DictionaryEntryDto {
    pub value: String,
    /// Key to file this value under when surname particles are ignored.
    ///
    /// Entries arrive sorted by `value` (particles included); a client whose
    /// user prefers the other convention re-sorts on this without refetching.
    pub sort_key: String,
    pub count: i64,
}

impl From<oxidgene_db::repo::DictionaryValueEntry> for DictionaryEntryDto {
    fn from(e: oxidgene_db::repo::DictionaryValueEntry) -> Self {
        Self {
            value: e.value,
            sort_key: e.sort_key,
            count: e.count,
        }
    }
}

/// Body of the dictionary's bulk particle edit.
///
/// `value` is the surname as listed by the family-names endpoint, particle
/// included; `particle` is the new cut to apply to every occurrence of it, an
/// empty string meaning "this name has no particle". The particle must already
/// be at the head of `value` — this edit moves a boundary, it never adds a word.
#[derive(Debug, Deserialize)]
pub struct SetFamilyNameParticleRequest {
    pub value: String,
    pub particle: String,
}

/// Outcome of a bulk particle edit.
#[derive(Debug, Serialize)]
pub struct FamilyNameParticleUpdateDto {
    /// The surname as it will still be listed — unchanged, since re-cutting
    /// only moves where the name files.
    pub value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: usize,
    pub persons_updated: usize,
}

impl From<oxidgene_db::repo::FamilyNameParticleUpdate> for FamilyNameParticleUpdateDto {
    fn from(u: oxidgene_db::repo::FamilyNameParticleUpdate) -> Self {
        Self {
            value: u.value,
            surname_prefix: u.surname_prefix,
            surname: u.surname,
            names_updated: u.names_updated,
            persons_updated: u.persons_updated,
        }
    }
}

/// A source paired with its citation count.
#[derive(Debug, Serialize)]
pub struct SourceDictionaryEntry {
    #[serde(flatten)]
    pub source: Source,
    pub count: i64,
}

/// A place paired with its usage count (events + media referencing it).
#[derive(Debug, Serialize)]
pub struct PlaceDictionaryEntry {
    #[serde(flatten)]
    pub place: Place,
    pub count: i64,
}

/// Query parameters for value-based dictionary usage drill-downs.
#[derive(Debug, Deserialize)]
pub struct DictionaryUsageQuery {
    pub value: String,
}

/// Query parameters for reference-content lookups (occupation sheets,
/// given-name meanings): the raw free-text GEDCOM value to resolve.
#[derive(Debug, Deserialize)]
pub struct ReferenceTermQuery {
    pub term: String,
}

/// Query parameters for the Sources tab's smart drill-down (section 8 of
/// ui-dictionary.md). Both the group listing and the final filtered source
/// list share the same `prefix` parameter — empty/absent means "top level"
/// (no filtering).
#[derive(Debug, Deserialize)]
pub struct SourcePrefixQuery {
    pub prefix: Option<String>,
}

/// A prefix group (Sources tab smart drill-down) plus how many sources fall
/// under it. `label` is always `prefix` (see `SourceDrillResponse`) extended
/// by exactly one more character.
#[derive(Debug, Serialize)]
pub struct SourceGroupDto {
    pub label: String,
    pub count: i64,
}

/// Response for the Sources tab's smart drill-down (ui-dictionary.md §8.10):
/// the backend auto-skips forced single-choice levels — e.g. a single
/// town's records nested under a department that otherwise branches many
/// ways — so `prefix` may be longer than the request's `prefix` query
/// parameter. `groups` is empty once `total` has dropped to <= the drill
/// threshold; the caller should then fetch the final flat list via
/// `GET .../dictionary/sources?prefix={prefix}` instead of rendering
/// another drill-down level.
#[derive(Debug, Serialize)]
pub struct SourceDrillResponse {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<SourceGroupDto>,
}

/// A person resolved for a dictionary usage drill-down list: name parts +
/// birth/death years, computed server-side to avoid one request per person.
#[derive(Debug, Serialize)]
pub struct PersonUsageEntryDto {
    pub person_id: uuid::Uuid,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub birth_year: Option<i32>,
    pub birth_qualifier: DateQualifier,
    pub death_year: Option<i32>,
    pub death_qualifier: DateQualifier,
}

impl From<oxidgene_db::repo::PersonUsageEntry> for PersonUsageEntryDto {
    fn from(e: oxidgene_db::repo::PersonUsageEntry) -> Self {
        Self {
            person_id: e.person_id,
            given_names: e.given_names,
            surname: e.surname,
            birth_year: e.birth_year,
            birth_qualifier: e.birth_qualifier,
            death_year: e.death_year,
            death_qualifier: e.death_qualifier,
        }
    }
}

// ── Geneanet import wizard ──────────────────────────────────────────
//
// The wizard's steps each have a request/response pair here. Step 3 has none:
// signing in and collecting the person↔photo mapping happens inside the login
// WebView, so what reaches the server is its output, carried by the steps that
// follow.

/// What a `.gw` file turned out to hold. Step 1.
#[derive(Debug, Serialize)]
pub struct InspectGenewebResponse {
    pub person_count: usize,
    pub family_count: usize,
    /// Blocks the lenient reader skipped — reported, never fatal.
    pub skipped_blocks: usize,
}

/// Data archives to index, by path. Step 2.
///
/// Paths and not bytes: the archives run to gigabytes, and this step only
/// exists on desktop, where the server is in-process and reads the same
/// filesystem the user picked from.
#[derive(Debug, Deserialize)]
pub struct IndexArchivesRequest {
    pub paths: Vec<String>,
}

/// One archive's central directory, read without extracting anything.
#[derive(Debug, Serialize)]
pub struct IndexedArchive {
    pub path: String,
    pub file_name: String,
    pub file_count: usize,
    /// Entries whose extension looks like a medium. Zero means "is this the
    /// right download?" — a warning, not a rejection.
    pub image_count: usize,
    /// Set when this archive alone could not be read.
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct IndexArchivesResponse {
    pub archives: Vec<IndexedArchive>,
    pub file_count: usize,
}

/// Everything needed to say what an import would do, without doing it. Step 4.
#[derive(Debug, Deserialize)]
pub struct GeneanetPreviewRequest {
    /// The `.gw` file, base64-encoded because JSON cannot carry raw bytes and
    /// raw bytes are what the ISO-8859-1-or-UTF-8 reader needs.
    pub gw_base64: String,
    pub file_name: String,
    /// The JSON the login window's collection script produced.
    pub collection: String,
    /// Byte length of each single-page deposit, gathered in the login window.
    /// This is what decides whether a photo is already in the archives.
    #[serde(default)]
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    #[serde(default)]
    pub archive_paths: Vec<String>,
    /// Which bytes to keep per medium: `renditions` (the default) or
    /// `originals`. `renditions` ignores `deposit_sizes` and `archive_paths`.
    #[serde(default)]
    pub media_fidelity: crate::service::geneanet::MediaFidelity,
}

/// The stat row and the three explanatory lines of step 4.
#[derive(Debug, Serialize)]
pub struct GeneanetPreviewResponse {
    pub person_count: usize,
    pub photo_count: usize,
    pub persons_with_photo: usize,
    pub attachment_count: usize,
    pub in_archives: usize,
    /// Document pages recognised in the archives by content rather than size.
    pub to_match: usize,
    pub to_download: usize,
    pub group_photos: usize,
    pub unlinked_views: usize,
    /// Multi-page deposits imported as documents.
    pub documents: usize,
    /// Pages those documents hold — all of them are imported.
    pub document_pages: usize,
    pub unlinked_names: usize,
    pub outside_tree: usize,
    pub ambiguous: usize,
    pub unlinked_names_sample: Vec<String>,
    pub outside_tree_names: Vec<String>,
    pub ambiguous_names: Vec<String>,
    /// `true` when almost no photo matched — the `.gw` and the account are
    /// probably not the same tree, and the wizard blocks rather than importing.
    pub mismatch: bool,
}

/// A step-3 session to encode for saving.
#[derive(Debug, Deserialize)]
pub struct EncodeSessionRequest {
    pub collection: String,
    #[serde(default)]
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    #[serde(default)]
    pub account: Option<String>,
    /// Media already fetched, base64 by URL. Present only in a save made after
    /// step 4, and what lets the file be imported with no connection at all.
    #[serde(default)]
    pub media: std::collections::HashMap<String, String>,
}

/// What a saved session held.
#[derive(Debug, Serialize)]
pub struct DecodeSessionResponse {
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub account: Option<String>,
    pub photo_count: usize,
    /// Media the file carried. Empty means the wizard still has to gather them.
    pub media: std::collections::HashMap<String, String>,
}

/// One medium the server cannot produce on its own.
#[derive(Debug, Serialize)]
pub struct NeededMedia {
    pub deposit_id: i64,
    pub view_id: i64,
    pub page: Option<i64>,
    /// Where the login window should fetch it from.
    pub url: String,
    /// `true` for a deposit's exact original, `false` for a page rendition.
    pub original: bool,
}

/// What the login window has to fetch before an import can run.
#[derive(Debug, Serialize)]
pub struct GeneanetPlanResponse {
    pub needed: Vec<NeededMedia>,
}

/// The same inputs as the preview, plus what it takes to fetch bytes. Step 5.
#[derive(Debug, Deserialize)]
pub struct GeneanetImportRequest {
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    #[serde(default)]
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    #[serde(default)]
    pub archive_paths: Vec<String>,
    /// Media the login window fetched, keyed by the URL they came from —
    /// **filesystem paths**, not the bytes.
    ///
    /// The server never fetches anything itself: no direct request to Geneanet
    /// succeeds, whatever the cookie and whatever the client. The window the
    /// user signed in to does it, writes each medium to a temp directory, and
    /// names it here. Paths rather than bytes because the gather only runs on
    /// the desktop, where this server is in-process on the same filesystem —
    /// exactly like the archive paths of step 2. Carrying the bytes instead
    /// meant base64 inflating them by a third and a request body that grew
    /// with the size of somebody's photo collection.
    #[serde(default)]
    pub fetched: std::collections::HashMap<String, String>,
    /// Which bytes to keep per medium: `renditions` (the default) or
    /// `originals`. `renditions` ignores `deposit_sizes` and `archive_paths`.
    #[serde(default)]
    pub media_fidelity: crate::service::geneanet::MediaFidelity,
}

/// What the import actually did.
#[derive(Debug, Serialize)]
pub struct GeneanetImportResponse {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    /// Distinct photos stored.
    pub media_count: usize,
    /// Person↔photo rows; higher than `media_count` when a photo shows several
    /// people, which is what the Geneanet export could not express at all.
    pub links_count: usize,
    /// Links marked as a person's profile photo, from the `.gw`'s `#image`.
    pub portraits_count: usize,
    /// People created for identifications Geneanet marks "hors de l'arbre".
    pub isolated_count: usize,
    /// Identification boxes kept as regions on the stored pictures.
    pub vignettes_count: usize,
    /// Photos that could not be fetched, one line each.
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}
