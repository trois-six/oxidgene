//! Request/response DTOs for REST endpoints.

use oxidgene_core::types::{Place, Source};
use oxidgene_core::{DateQualifier, EventType};
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

/// Query parameters of the audit log.
#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Number of entries to return (default: 25, max: 100).
    pub first: Option<u64>,
    /// Cursor to continue after (the last entry's ID).
    pub after: Option<String>,
    /// Only the writes of this category.
    pub category: Option<oxidgene_core::history::AuditCategory>,
    /// Only the writes about this record.
    pub subject_id: Option<uuid::Uuid>,
}

/// Body of a restore: the version to put the record back as.
#[derive(Debug, Deserialize)]
pub struct RevertRecordRequest {
    pub version: i32,
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

/// Held picture sources to resolve to inline data in one operation.
#[derive(Debug, Deserialize)]
pub struct ImageDataRequest {
    pub sources: Vec<oxidgene_core::types::ImageSource>,
}

// ── Tree DTOs ────────────────────────────────────────────────────────

/// Request body for duplicating a tree.
#[derive(Debug, Deserialize)]
pub struct DuplicateTreeRequest {
    pub name: String,
}

// ── Person DTOs ──────────────────────────────────────────────────────

/// Text, paging and ordering of a person search (Sprint E.6).
///
/// The search goes through the `person_search_fts` table (accent-folded,
/// all words must match) and returns a paginated `SearchResult`. An empty
/// or missing `q` lists all persons sorted by name (browse mode).
///
/// The structured filters are read from the same query string into
/// [`oxidgene_db::repo::PersonSearchFilters`] by a second extractor, so the
/// filter list is declared once rather than restated here.
#[derive(Debug, Deserialize)]
pub struct PersonSearchQuery {
    /// Free-text query.
    pub q: Option<String>,
    /// Maximum results to return (default and ceiling: see
    /// [`crate::profile::service::SEARCH_DEFAULT_LIMIT`]).
    pub limit: Option<usize>,
    /// Offset for pagination (default: 0).
    pub offset: Option<usize>,
    #[serde(default)]
    pub sort: oxidgene_db::repo::PersonSearchSort,
}

/// Query parameters for GET /api/v1/trees/:tree_id/persons/recently-modified.
#[derive(Debug, Deserialize)]
pub struct RecentlyModifiedQuery {
    /// Maximum persons to return (default and ceiling: see
    /// [`crate::service::history::RECENT_PERSONS_DEFAULT_LIMIT`]).
    pub limit: Option<usize>,
}

/// Query parameters for GET /api/v1/trees/recent-persons.
#[derive(Debug, Deserialize)]
pub struct RecentPersonsQuery {
    /// The trees, comma-separated.
    pub tree_ids: String,
    /// Maximum persons per tree (as [`RecentlyModifiedQuery::limit`]).
    pub limit: Option<usize>,
}

/// Response for GET /api/v1/trees/:tree_id/persons/:person_id.
/// Wraps the core `Person` with the server-computed SOSA number.
#[derive(Debug, Serialize)]
pub struct PersonDetailResponse {
    #[serde(flatten)]
    pub person: oxidgene_core::types::Person,
    pub sosa_number: Option<u64>,
}

/// Query parameters for listing persons: pagination, and a name filter.
#[derive(Debug, Deserialize)]
pub struct PersonListQuery {
    pub first: Option<u64>,
    pub after: Option<String>,
    /// Keep the persons with a name — given names, surname or nickname —
    /// containing this.
    pub search: Option<String>,
}

/// Request body for recording that a person differs from their homonyms.
#[derive(Debug, Deserialize)]
pub struct MarkPersonsDistinctRequest {
    /// The persons the path's person is confirmed not to be.
    pub person_ids: Vec<uuid::Uuid>,
}

/// Request body for merging a duplicate into the path's person.
#[derive(Debug, Deserialize)]
pub struct MergePersonRequest {
    /// The record absorbed and soft-deleted; the path's person is kept.
    pub duplicate_id: uuid::Uuid,
    /// What the comparison chose; absent, everything the duplicate carried
    /// moves and the kept person's name and sex stay.
    #[serde(default)]
    pub choices: MergeChoicesBody,
}

/// The choices of a merge (`docs/api.md`, merge).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MergeChoicesBody {
    /// Own events of either person left out of the merged record.
    pub left_out_events: Vec<uuid::Uuid>,
    /// The duplicate's direct media links not taken.
    pub left_out_media_links: Vec<uuid::Uuid>,
    pub surname_from_duplicate: bool,
    pub given_names_from_duplicate: bool,
    pub sex_from_duplicate: bool,
}

impl From<MergeChoicesBody> for crate::service::duplicates::MergeChoices {
    fn from(body: MergeChoicesBody) -> Self {
        Self {
            left_out_events: body.left_out_events,
            left_out_media_links: body.left_out_media_links,
            surname_from_duplicate: body.surname_from_duplicate,
            given_names_from_duplicate: body.given_names_from_duplicate,
            sex_from_duplicate: body.sex_from_duplicate,
        }
    }
}

// ── Ancestry query params ────────────────────────────────────────────

/// Query parameters for ancestor/descendant queries.
#[derive(Debug, Deserialize)]
pub struct AncestryQuery {
    /// Maximum depth to traverse.
    pub max_depth: Option<i32>,
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

// ── Place DTOs ───────────────────────────────────────────────────────

/// Query parameters for listing places (search + pagination).
#[derive(Debug, Deserialize)]
pub struct PlaceListQuery {
    pub first: Option<u64>,
    pub after: Option<String>,
    pub search: Option<String>,
}

/// Query parameters for deleting a source.
#[derive(Debug, Default, Deserialize)]
pub struct DeleteSourceQuery {
    /// Keep the source if any citation, note, media link or repository link still points at
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

/// Query parameters of a repository delete.
#[derive(Debug, Deserialize)]
pub struct DeleteRepositoryQuery {
    /// Keep the repository if it still holds a source or is the subject of
    /// a note. Answered by the status code: `204` deleted, `200` kept.
    #[serde(default)]
    pub only_if_unused: bool,
}

/// A source a repository holds, with the link saying under which call
/// number.
#[derive(Debug, Serialize)]
pub struct HeldSource {
    #[serde(flatten)]
    pub link: oxidgene_core::types::SourceRepository,
    pub source: Source,
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

/// One free-form tag to attach to or remove from a media item.
#[derive(Debug, Deserialize)]
pub struct MediaTagRequest {
    pub tag: String,
}

/// Query parameters of the media list: pagination, then filters that
/// combine with AND. Spelled out rather than flattened, because a flattened
/// query string cannot carry the numbers and dates.
#[derive(Debug, Deserialize)]
pub struct MediaListQuery {
    /// Number of items to return (default: 25, max: 100).
    pub first: Option<u64>,
    /// Cursor to start after (UUID string).
    pub after: Option<String>,
    // `tag` may be repeated, which this map-shaped query cannot hold: the
    // handler reads it from the raw pairs (`tag_values`).
    pub kind: Option<oxidgene_core::enums::MediaFileKind>,
    pub category: Option<oxidgene_core::enums::DocumentCategory>,
    pub name: Option<String>,
    pub linked_name: Option<String>,
    pub event_from: Option<i32>,
    pub event_to: Option<i32>,
    pub added_from: Option<chrono::NaiveDate>,
    pub added_to: Option<chrono::NaiveDate>,
}

impl MediaListQuery {
    /// The filter part of the query, with the tags given.
    pub fn filters(&self, tags: Vec<String>) -> crate::service::media_library::MediaListFilters {
        crate::service::media_library::MediaListFilters {
            tags,
            kind: self.kind,
            category: self.category,
            name: self.name.clone(),
            linked_name: self.linked_name.clone(),
            event_from: self.event_from,
            event_to: self.event_to,
            added_from: self.added_from,
            added_to: self.added_to,
        }
    }
}

/// Every value of a repeatable query parameter, in the order given.
pub fn tag_values(pairs: Vec<(String, String)>) -> Vec<String> {
    pairs
        .into_iter()
        .filter(|(key, _)| key == "tag")
        .map(|(_, value)| value)
        .collect()
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

/// A media together with the link that attached it — one gallery tile.
#[derive(Debug, Serialize)]
pub struct MediaWithLink {
    pub link_id: uuid::Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: oxidgene_core::types::Media,
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
    pub repository_id: Option<uuid::Uuid>,
    pub first: Option<u64>,
    pub after: Option<String>,
}

// ── Import / export DTOs ─────────────────────────────────────────────

/// Query parameters naming a GeneWeb `.gw` file.
#[derive(Debug, Deserialize)]
pub struct ImportGenewebQuery {
    /// Name of the uploaded file. GeneWeb records it on every family and it is
    /// echoed back in parse warnings; defaults to `import.gw` when omitted.
    pub filename: Option<String>,
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

/// Body of the batched pedigree operation.
#[derive(Debug, Deserialize)]
pub struct PedigreesRequest {
    pub root_person_ids: Vec<uuid::Uuid>,
    /// Number of ancestor generations to include, for every root.
    pub ancestor_depth: u32,
    /// Number of descendant generations to include, for every root.
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
    /// Family names only: how many of `count` carry the value as their
    /// primary name, i.e. how many a rename would reach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_count: Option<i64>,
}

impl From<oxidgene_db::repo::DictionaryValueEntry> for DictionaryEntryDto {
    fn from(e: oxidgene_db::repo::DictionaryValueEntry) -> Self {
        Self {
            value: e.value,
            sort_key: e.sort_key,
            count: e.count,
            primary_count: e.primary_count,
        }
    }
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

/// Outcome of a family-name rename.
#[derive(Debug, Serialize)]
pub struct FamilyNameRenameDto {
    pub value: String,
    pub new_value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: usize,
    pub persons_updated: usize,
    /// `new_value` was already listed: the renamed names joined it.
    pub merged: bool,
}

impl From<oxidgene_db::repo::FamilyNameRename> for FamilyNameRenameDto {
    fn from(r: oxidgene_db::repo::FamilyNameRename) -> Self {
        Self {
            value: r.value,
            new_value: r.new_value,
            surname_prefix: r.surname_prefix,
            surname: r.surname,
            names_updated: r.names_updated,
            persons_updated: r.persons_updated,
            merged: r.merged,
        }
    }
}

pub use crate::service::source::SourceDictionaryEntry;

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

/// Query parameters for place suggestions from the place dictionary.
#[derive(Debug, Deserialize)]
pub struct PlaceSuggestionQuery {
    pub q: String,
    pub limit: Option<usize>,
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
/// threshold, and the final flat list of the level comes with it in
/// `sources`, so the page needs no second request.
#[derive(Debug, Serialize)]
pub struct SourceDrillResponse {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<SourceGroupDto>,
    /// The sources under `prefix`, when there is no group left to choose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceDictionaryEntry>>,
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

/// The staged media of a decoded session the wizard no longer needs.
#[derive(Debug, Deserialize)]
pub struct ReleaseSessionMediaRequest {
    /// Local paths, as `/geneanet/session/decode` returned them. A path the
    /// backend did not stage is ignored.
    pub paths: Vec<String>,
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
