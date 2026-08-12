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

/// Query parameters for listing citations by entity.
#[derive(Debug, Deserialize)]
pub struct CitationListQuery {
    pub person_id: Option<uuid::Uuid>,
    pub event_id: Option<uuid::Uuid>,
    pub family_id: Option<uuid::Uuid>,
    pub source_id: Option<uuid::Uuid>,
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
    #[serde(default, deserialize_with = "double_option")]
    pub title: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
}

// ── MediaLink DTOs ──────────────────────────────────────────────────

/// Row returned by the bulk media-links endpoint.
#[derive(Debug, Serialize)]
pub struct MediaLinkListRow {
    pub entity_id: uuid::Uuid,
    pub entity_type: String,
    pub media_id: uuid::Uuid,
    pub file_path: String,
    pub file_name: String,
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
#[derive(Debug, Serialize)]
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
    pub death_year: Option<i32>,
}

impl From<oxidgene_db::repo::PersonUsageEntry> for PersonUsageEntryDto {
    fn from(e: oxidgene_db::repo::PersonUsageEntry) -> Self {
        Self {
            person_id: e.person_id,
            given_names: e.given_names,
            surname: e.surname,
            birth_year: e.birth_year,
            death_year: e.death_year,
        }
    }
}
