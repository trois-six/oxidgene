//! HTTP API client for communicating with the OxidGene backend.
//!
//! Provides a typed client wrapping [`reqwest::Client`] that maps to the
//! REST API defined in `oxidgene-api`.  All methods return domain types
//! from [`oxidgene_core`] directly, since those types already derive
//! `Serialize` / `Deserialize`.

use base64::Engine as _;
#[cfg(feature = "telemetry-client")]
use opentelemetry::global;
#[cfg(feature = "telemetry-client")]
use opentelemetry::propagation::Injector;
use oxidgene_core::projection::{Pedigree, PedigreeDelta, PersonProfile, SearchResult};
use oxidgene_core::types::{
    AncestryLink, Citation, Connection, DOCUMENT_MIME, Event, EventWitness, Family, FamilyChild,
    FamilySpouse, ImageCrop, ImageSource, Media, Note, Person, PersonName, Place, QualifiedYear,
    Source, Tree, Vignette,
};
use oxidgene_core::{
    Calendar, ChildType, Confidence, DateQualifier, DocumentCategory, EventType, NameType, Privacy,
    Sex, SourceMediaType, SpouseRole, TreeDefaultPrivacy,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "telemetry-client")]
use tracing::Instrument as _;
#[cfg(feature = "telemetry-client")]
use tracing_opentelemetry::OpenTelemetrySpanExt as _;
use uuid::Uuid;

// ── PersonDetail — person + server-computed SOSA number ──────────────

/// Mirrors `PersonDetailResponse` from the API: all `Person` fields flat + SOSA.
#[derive(Debug, Clone, Deserialize)]
pub struct PersonDetail {
    pub id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,
    pub privacy: Privacy,
    /// Which image represents this person: a whole media, or a region of one.
    /// At most one is ever set.
    #[serde(default)]
    pub portrait_media_id: Option<Uuid>,
    #[serde(default)]
    pub portrait_vignette_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub sosa_number: Option<u64>,
}

// ── Re-usable request / response DTOs (client-side mirrors) ─────────

/// Paginated response returned by list endpoints.
/// Re-uses the same shape as `oxidgene_core::types::Connection<T>`.
type PaginatedResponse<T> = Connection<T>;

#[derive(Debug, Clone, Copy, Default)]
pub enum PersonSearchSort {
    #[default]
    Relevance,
    NameAsc,
    NameDesc,
    BirthAsc,
    BirthDesc,
}

impl PersonSearchSort {
    fn as_str(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::BirthAsc => "birth_asc",
            Self::BirthDesc => "birth_desc",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PersonSearchParams {
    pub query: String,
    pub limit: u32,
    pub offset: u32,
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
    pub has_media: bool,
    pub sort: PersonSearchSort,
}

// ── Dictionary — distinct-value aggregations with usage counts ──────

/// A distinct free-text value (surname, occupation label) plus how many
/// persons carry it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DictionaryEntry {
    pub value: String,
    /// Filing key when surname particles are ignored; see the sorting
    /// preference in `crate::prefs`.
    #[serde(default)]
    pub sort_key: String,
    pub count: i64,
}

/// A source paired with its citation count.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceDictionaryEntry {
    #[serde(flatten)]
    pub source: Source,
    pub count: i64,
}

/// A prefix group for the Sources tab's smart drill-down (see
/// ui-dictionary.md §8): `label` is the resolved prefix (see
/// `SourceDrillResponse`) extended by exactly one more character, paired
/// with how many sources fall under it.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceGroupEntry {
    pub label: String,
    pub count: i64,
}

/// Response for the Sources tab's smart drill-down (ui-dictionary.md
/// §8.10): the backend auto-skips forced single-choice levels, so `prefix`
/// may be longer than the prefix that was requested. `groups` is empty
/// once `total` has dropped to <= the drill threshold — fetch the final
/// flat list via `dictionary_sources(tree_id, &prefix)` instead.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceDrillResponse {
    pub prefix: String,
    pub total: i64,
    pub groups: Vec<SourceGroupEntry>,
}

/// A place paired with its usage count (events + media referencing it).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlaceDictionaryEntry {
    #[serde(flatten)]
    pub place: Place,
    pub count: i64,
}

/// A person resolved for a dictionary usage drill-down list: name parts +
/// birth/death years, computed server-side in one bulk query.
#[derive(Debug, Clone, Deserialize)]
pub struct PersonUsageEntry {
    pub person_id: Uuid,
    pub given_names: Option<String>,
    pub surname: Option<String>,
    pub birth_year: Option<i32>,
    #[serde(default)]
    pub birth_qualifier: DateQualifier,
    pub death_year: Option<i32>,
    #[serde(default)]
    pub death_qualifier: DateQualifier,
}

impl PersonUsageEntry {
    /// The birth/death years with their precision, ready for
    /// [`format_lifespan`](crate::components::pedigree_chart::format_lifespan).
    pub fn lifespan_years(&self) -> (Option<QualifiedYear>, Option<QualifiedYear>) {
        (
            self.birth_year
                .map(|y| QualifiedYear::new(y, self.birth_qualifier)),
            self.death_year
                .map(|y| QualifiedYear::new(y, self.death_qualifier)),
        )
    }
}

/// Body of the dictionary's bulk particle edit.
#[derive(Debug, Serialize)]
struct SetFamilyNameParticleBody {
    value: String,
    /// Empty means "this name has no particle".
    particle: String,
}

/// Outcome of a bulk particle edit.
#[derive(Debug, Clone, Deserialize)]
pub struct FamilyNameParticleUpdate {
    /// The surname as it will still be listed — re-cutting moves where the
    /// name files, not the text.
    pub value: String,
    pub surname_prefix: Option<String>,
    pub surname: String,
    pub names_updated: usize,
    pub persons_updated: usize,
}

// ── Reference content — occupation sheets, given-name meanings ──────

/// Occupation fiche content, localized to the requesting UI language.
#[derive(Debug, Clone, Deserialize)]
pub struct OccupationReference {
    pub label: String,
    pub summary: String,
    pub text: String,
}

/// Given-name meaning content, localized to the requesting UI language.
#[derive(Debug, Clone, Deserialize)]
pub struct GivenNameReference {
    pub label: String,
    pub origin: String,
    pub meaning: String,
    pub text: String,
    pub feast_day: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GivenNameReferenceMatch {
    pub term: String,
    #[serde(flatten)]
    pub reference: GivenNameReference,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OccupationReferenceMatch {
    pub term: String,
    #[serde(flatten)]
    pub reference: OccupationReference,
}

#[derive(Debug, Serialize)]
struct ReferenceTermsBody<'a> {
    terms: &'a [String],
}

const REFERENCE_TERM_BATCH_SIZE: usize = 128;

fn reference_term_batches(terms: &[String]) -> std::slice::Chunks<'_, String> {
    terms.chunks(REFERENCE_TERM_BATCH_SIZE)
}

// ── Tree request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateTreeBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A tree as listed on the home page, with transient server-job state.
#[derive(Debug, Clone, Deserialize)]
pub struct TreeListItem {
    #[serde(flatten)]
    pub tree: Tree,
    #[serde(default)]
    pub import_in_progress: bool,
    #[serde(default)]
    pub import_job_id: Option<Uuid>,
}

impl std::ops::Deref for TreeListItem {
    type Target = Tree;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

#[derive(Debug, Default, Serialize)]
pub struct UpdateTreeBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sosa_root_person_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_person_id: Option<Option<Uuid>>,
    /// What `Privacy::Default` resolves to for everything in this tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_privacy: Option<TreeDefaultPrivacy>,
}

#[derive(Debug, Serialize)]
pub struct DuplicateTreeBody {
    pub name: String,
}

// ── Person request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonBody {
    pub sex: Sex,
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sex: Option<Sex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
}

// ── PersonName request bodies ───────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePersonNameBody {
    pub name_type: NameType,
    pub given_names: Option<String>,
    /// Surname root only — split the particle off with
    /// `oxidgene_core::types::split_surname_particle` before sending.
    pub surname: Option<String>,
    pub surname_prefix: Option<String>,
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub is_primary: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub sort_order: i32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

#[derive(Debug, Serialize)]
pub struct UpdatePersonNameBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_type: Option<NameType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_names: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surname_prefix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suffix: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_primary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<i32>,
}

// ── Family member request bodies ────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AddSpouseBody {
    pub person_id: Uuid,
    pub role: SpouseRole,
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Serialize)]
pub struct AddChildBody {
    pub person_id: Uuid,
    pub child_type: ChildType,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Event request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateEventBody {
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub cause: Option<String>,
    pub place_id: Option<Uuid>,
    pub person_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateEventBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_qualifier: Option<DateQualifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Option<Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

/// Request body for adding a witness to an event.
#[derive(Debug, Serialize)]
pub struct AddEventWitnessBody {
    pub person_id: Uuid,
    pub relation: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
}

// ── Place request bodies ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreatePlaceBody {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct UpdatePlaceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<Option<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<Option<f64>>,
}

// ── Source request bodies ───────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateSourceBody {
    pub title: String,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub abbreviation: Option<String>,
    pub repository_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSourceBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abbreviation: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_name: Option<Option<String>>,
}

// ── Citation request bodies ─────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateCitationBody {
    pub source_id: Uuid,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub page: Option<String>,
    pub confidence: Confidence,
    pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateCitationBody {
    /// Repoints the citation at another source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Option<String>>,
}

// ── Note request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateNoteBody {
    pub text: String,
    pub person_id: Option<Uuid>,
    pub event_id: Option<Uuid>,
    pub family_id: Option<Uuid>,
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<uuid::Uuid>,
}

#[derive(Debug, Serialize)]
pub struct UpdateNoteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

// ── MediaLink DTOs ───────────────────────────────────────────────────

/// A row from the bulk media-links endpoint.
///
/// Carries what a small preview needs — the MIME type and whether a thumbnail
/// exists — so a timeline of forty events draws its evidence from the one call
/// the pedigree canvas already makes.
#[derive(Debug, Clone, Deserialize)]
pub struct MediaLinkRow {
    pub link_id: uuid::Uuid,
    pub entity_id: uuid::Uuid,
    /// `person` or `event`.
    pub entity_type: String,
    pub media_id: uuid::Uuid,
    pub file_path: String,
    pub file_name: String,
    pub mime_type: String,
    pub has_thumbnail: bool,
}

/// Whether a `file_path` is an address rather than a path.
///
/// The column holds whatever produced the record wrote there: a Windows path
/// out of a GEDCOM, a relative name, or — when the media is one we deliberately
/// never fetched — the URL it lives at. Only the last is something a browser
/// can be pointed at.
fn is_remote(file_path: &str) -> bool {
    file_path.starts_with("http://") || file_path.starts_with("https://")
}

#[derive(Debug, Serialize)]
struct PortraitImagesRequest {
    person_ids: Vec<Uuid>,
}

/// One portrait as the API sends it: where the picture lives, not the picture.
#[derive(Debug, Deserialize)]
struct WirePortraitImage {
    person_id: Uuid,
    #[serde(flatten)]
    image: WireCroppedSource,
}

/// A picture's address and the region to take out of it, as sent by the API.
/// [`ApiClient::resolve_source`] turns it into the drawable [`CroppedSource`]
/// the components take.
#[derive(Debug, Clone, Deserialize)]
struct WireCroppedSource {
    source: ImageSource,
    #[serde(default)]
    crop: Option<ImageCrop>,
}

const PORTRAIT_BATCH_SIZE: usize = 1_024;

/// Matches the server's `MAX_PEDIGREES_PER_REQUEST`.
const PEDIGREE_BATCH_SIZE: usize = 64;

#[derive(Debug, Serialize)]
struct PedigreesRequest {
    root_person_ids: Vec<Uuid>,
    ancestor_depth: u32,
    descendant_depth: u32,
}

#[derive(Debug, Deserialize)]
struct PedigreeEntry {
    root_person_id: Uuid,
    pedigree: Pedigree,
}

fn portrait_batches(person_ids: &[Uuid]) -> impl Iterator<Item = &[Uuid]> {
    person_ids.chunks(PORTRAIT_BATCH_SIZE)
}

#[derive(Debug, Serialize)]
struct GalleryBundleRequest {
    media_ids: Vec<Uuid>,
    vignette_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
struct RelationLabelsRequest {
    person_ids: Vec<Uuid>,
    family_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RelationLabels {
    pub names: Vec<PersonName>,
    pub spouses: Vec<FamilySpouse>,
}

const RELATION_LABEL_BATCH_SIZE: usize = 1_024;

fn relation_label_batch_ranges(
    person_count: usize,
    family_count: usize,
) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let mut batches = Vec::new();
    let (mut person_offset, mut family_offset) = (0, 0);
    while person_offset < person_count || family_offset < family_count {
        let person_end = (person_offset + RELATION_LABEL_BATCH_SIZE).min(person_count);
        let remaining = RELATION_LABEL_BATCH_SIZE - (person_end - person_offset);
        let family_end = (family_offset + remaining).min(family_count);
        batches.push((person_offset..person_end, family_offset..family_end));
        person_offset = person_end;
        family_offset = family_end;
    }
    batches
}

/// A gallery's pictures, resolved to something drawable.
///
/// The wire form ([`WireGalleryBundle`]) carries addresses; the fields here
/// carry whatever this platform draws them from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GalleryBundle {
    pub media: Vec<GalleryMedia>,
    pub vignettes: Vec<GalleryVignette>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryMedia {
    pub media_id: Uuid,
    pub source: Option<String>,
    pub event_ids: Vec<Uuid>,
    pub document_previews: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryVignette {
    pub vignette_id: Uuid,
    pub image: CroppedSource,
}

#[derive(Debug, Clone, Deserialize)]
struct WireGalleryBundle {
    media: Vec<WireGalleryMedia>,
    vignettes: Vec<WireGalleryVignette>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireGalleryMedia {
    media_id: Uuid,
    source: Option<ImageSource>,
    event_ids: Vec<Uuid>,
    document_previews: Vec<ImageSource>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireGalleryVignette {
    vignette_id: Uuid,
    #[serde(flatten)]
    image: WireCroppedSource,
}

/// A picture to draw, and the region of it to show.
///
/// One value rather than two loose fields, because they are only ever read
/// together: `crop` is set exactly when `source` is a whole picture the server
/// could not cut — a region of a file we do not hold and never fetch — and
/// absent for every image that arrives already cut.
/// A picture ready to draw, and the region of it to show.
///
/// `source` is whatever this platform puts in an `src`: a path on the
/// application's own origin, an address we do not own, or a `data:` URL. It is
/// never a backend address — see `image_host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CroppedSource {
    pub source: String,
    pub crop: Option<ImageCrop>,
}

impl CroppedSource {
    /// A picture to be shown whole.
    pub fn whole(source: String) -> Self {
        Self { source, crop: None }
    }

    /// The default silhouette for someone with no portrait.
    ///
    /// One place rather than five, so the fallback cannot drift between the
    /// search list, the search grid, the pedigree cards and the profile header.
    pub fn silhouette(sex: oxidgene_core::Sex) -> Self {
        Self::whole(crate::components::pedigree_chart::default_portrait(sex).to_string())
    }
}

/// A media together with the link that attached it — one gallery tile.
///
/// Mirrors `MediaWithLink` on the API side, which flattens the media, so the
/// media's own fields sit at the top level here too.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MediaWithLink {
    pub link_id: uuid::Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

#[derive(Debug, Deserialize)]
struct MediaDeletionStatus {
    can_delete: bool,
}

/// Where a media's bytes actually are.
///
/// Three states, and every view has to tell them apart. A media OxidGene holds
/// is served by us, has a thumbnail and can be cropped. A remote one is a URL
/// someone else serves — worth recording, never fetched by us, and therefore
/// without a thumbnail or a crop. A record naming a file nobody ever uploaded
/// has no bytes at all, which is where every GEDCOM import starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSource {
    /// The bytes are in our store.
    Stored,
    /// `file_path` is an http(s) URL, served by whoever owns it.
    Remote,
    /// A path we were told about and never received.
    Unheld,
}

/// How a media should be presented when there is room to show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Pdf,
    Document,
    Other,
}

impl MediaKind {
    /// The glyph a tile draws when there is no picture to draw instead.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Image => "\u{1F5BC}",
            Self::Video => "\u{1F3AC}",
            Self::Audio => "\u{1F3B5}",
            Self::Pdf => "\u{1F4C4}",
            Self::Document => "\u{1F4C3}",
            Self::Other => "\u{1F4C1}",
        }
    }

    /// Whether the browser can render this inline, given a URL.
    ///
    /// Images, video and audio each have an element that takes a URL and
    /// plays it. Everything else — a PDF, a Word document, an archive — is a
    /// download, and pretending otherwise gives the reader an empty box.
    pub fn is_embeddable(self) -> bool {
        matches!(self, Self::Image | Self::Video | Self::Audio)
    }
}

/// Which of the three states a media row is in.
///
/// Takes the row rather than the tile: the viewer asks this of the page it is
/// showing, which is where the bytes and the URL actually are — a document is
/// always `Unheld`, and answering that about the register somebody is reading
/// would be true of the shell and wrong about the file.
pub fn media_source(media: &Media) -> MediaSource {
    if media.storage_key.is_some() {
        MediaSource::Stored
    } else if is_remote(&media.file_path) {
        MediaSource::Remote
    } else {
        MediaSource::Unheld
    }
}

impl MediaWithLink {
    /// Which of the three states this media is in.
    pub fn source(&self) -> MediaSource {
        media_source(&self.media)
    }

    /// How to present it.
    ///
    /// Reads `mime_type` and trusts it: every write path normalises it, so a
    /// second opinion here would only be a second place for the rule to live.
    pub fn kind(&self) -> MediaKind {
        media_kind(&self.media.mime_type)
    }

    /// Whether this tile can be shown as a picture rather than a file icon.
    pub fn is_image(&self) -> bool {
        self.kind() == MediaKind::Image
    }

    /// Whether a crop can be drawn on it.
    ///
    /// Only a stored raster: a crop is served by re-decoding our own copy, so
    /// a remote URL has nothing to cut, and a record with no bytes has nothing
    /// at all.
    pub fn is_croppable(&self) -> bool {
        self.source() == MediaSource::Stored
            && self.is_image()
            && self.media.width.is_some()
            && self.media.height.is_some()
    }

    /// A short badge for the file type — "PDF", "JPEG", "MP4".
    pub fn kind_label(&self) -> String {
        media_kind_label(&self.media.mime_type)
    }

    /// What to write under a tile: the title if there is one, else the file name.
    pub fn caption(&self) -> &str {
        match self.media.title.as_deref() {
            Some(title) if !title.trim().is_empty() => title,
            _ => &self.media.file_name,
        }
    }
}

/// A short badge for a MIME type — "PDF", "JPEG", "MP4".
///
/// A document's own MIME type is an internal marker, not a format anybody
/// recognises: spelled out it reads "OXIDGENE-DOCUMENT", which tells a reader
/// nothing and leaks a private name into the interface.
pub fn media_kind_label(mime_type: &str) -> String {
    if mime_type.trim().eq_ignore_ascii_case(DOCUMENT_MIME) {
        return "DOCUMENT".to_string();
    }
    mime_type
        .rsplit('/')
        .next()
        .unwrap_or("file")
        .trim_start_matches("x-")
        .split('+')
        .next()
        .unwrap_or("file")
        .to_uppercase()
}

/// Classify a MIME type into what the UI can do with it.
pub fn media_kind(mime_type: &str) -> MediaKind {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == DOCUMENT_MIME {
        // A document holds no bytes of its own — what can be drawn is its
        // pages. Reading its marker as a generic file would land it in
        // `Other`, which is why an imported photograph drew a folder.
        MediaKind::Document
    } else if mime.starts_with("image/") {
        MediaKind::Image
    } else if mime.starts_with("video/") {
        MediaKind::Video
    } else if mime.starts_with("audio/") {
        MediaKind::Audio
    } else if mime == "application/pdf" {
        MediaKind::Pdf
    } else if mime.starts_with("text/")
        || mime.contains("word")
        || mime.contains("opendocument")
        || mime.contains("officedocument")
    {
        MediaKind::Document
    } else {
        MediaKind::Other
    }
}

#[derive(Debug, Serialize)]
pub struct CreateMediaLinkBody {
    pub media_id: uuid::Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub sort_order: i32,
}

/// One file on its way up, and what it should become on arrival.
///
/// A struct rather than six positional arguments: three of them are
/// `Option<Uuid>`, and a call site that reads `(None, None, Some(id))` tells
/// nobody which of "attach to this record" and "make it a page of this
/// document" was meant.
#[derive(Debug, Clone)]
pub struct MediaUpload {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// Fill in an existing record that named a file without holding it.
    pub attach_to: Option<Uuid>,
    /// Append as the next page of this multi-page document.
    pub as_page_of: Option<Uuid>,
}

#[derive(Debug, Serialize, Default)]
pub struct SetPortraitBody {
    /// A whole media.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<uuid::Uuid>,
    /// A region of one — a face in a group photograph.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vignette_id: Option<uuid::Uuid>,
}

/// A media carries the same descriptive fields a fact does — and no source
/// field, because a media *is* a source document.
#[derive(Debug, Default, Serialize)]
pub struct UpdateMediaBody {
    /// `Some(None)` clears the field, absent leaves it alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_value2: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_qualifier: Option<DateQualifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar: Option<Calendar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place_id: Option<Option<uuid::Uuid>>,
    /// The URL of a remote media. The server refuses it for a media it stores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The picture's pixel size, sent together or not at all. Accepted only
    /// for a page we do not hold: nothing here ever opened that file, so the
    /// browser that displayed it is the only witness to how big it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// Whether this is shown when the tree is published.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy: Option<Privacy>,
    /// What the medium physically is, in GEDCOM's own vocabulary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_media_type: Option<SourceMediaType>,
    /// What kind of record it is. Sending it without a `source_media_type`
    /// also sets the medium it implies, so a census return does not export as
    /// `OTHER`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_category: Option<Option<DocumentCategory>>,
}

#[derive(Debug, Serialize)]
pub struct MediaTagBody {
    pub tag: String,
}

// ── Vignette DTOs ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateVignetteBody {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
}

/// The four rectangle fields travel together — send all or none.
#[derive(Debug, Default, Serialize)]
pub struct UpdateVignetteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<Option<uuid::Uuid>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<Option<uuid::Uuid>>,
}

// ── Import / export DTOs ────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ImportGedcomBody {
    pub gedcom: String,
}

// ── Geneanet import wizard ──────────────────────────────────────────
//
// Mirrors `oxidgene_api::rest::dto`. Step 3 has no type here: signing in and
// collecting the person↔photo mapping happens in the desktop login window, and
// what it produces is carried by the steps that follow.

/// What a `.gw` file turned out to hold. Step 1.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GwInspection {
    pub person_count: usize,
    pub family_count: usize,
    /// Blocks the lenient reader skipped — reported, never fatal.
    pub skipped_blocks: usize,
}

#[derive(Debug, Serialize)]
pub struct IndexArchivesBody {
    pub paths: Vec<String>,
}

/// One data archive's central directory, read without extracting anything.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IndexedArchive {
    pub path: String,
    pub file_name: String,
    pub file_count: usize,
    pub image_count: usize,
    /// Set when this archive alone could not be read; the others still stand.
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArchiveIndex {
    pub archives: Vec<IndexedArchive>,
    pub file_count: usize,
}

/// Which bytes a Geneanet import keeps for each medium.
///
/// `Renditions` is the default and needs nothing from the user but their
/// login: every page is stored as Geneanet's own `normal` variant, recompressed
/// and resized. `Originals` keeps the uploaded files, which means the data
/// archives — a separate Geneanet request and several gigabytes of ZIP — plus a
/// byte-length pass to match them on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFidelity {
    #[default]
    Renditions,
    Originals,
}

impl MediaFidelity {
    /// Whether this import consults the user's data archives at all.
    #[must_use]
    pub const fn uses_archives(self) -> bool {
        matches!(self, Self::Originals)
    }
}

#[derive(Debug, Serialize)]
pub struct GeneanetPreviewBody {
    /// The `.gw`, base64-encoded: JSON cannot carry the raw bytes the
    /// ISO-8859-1-or-UTF-8 reader needs, and this body carries other fields
    /// alongside it.
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub archive_paths: Vec<String>,
    pub media_fidelity: MediaFidelity,
}

/// A step-3 session, encoded for the file the wizard saves.
#[derive(Debug, Serialize)]
pub struct GeneanetSessionBody {
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub account: Option<String>,
    /// Media already fetched. Saving after step 4 includes them, which is what
    /// makes the file importable with no connection.
    pub media: std::collections::HashMap<String, String>,
}

/// What a saved session held.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetSession {
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub account: Option<String>,
    /// Media the collection covers, pages included.
    pub photo_count: usize,
    /// Media the file carried. Empty means the wizard must still gather them.
    pub media: std::collections::HashMap<String, String>,
}

/// One medium the server cannot produce on its own.
#[derive(Debug, Clone, PartialEq, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetPlan {
    pub needed: Vec<NeededMedia>,
}

/// The stat row and the explanatory lines of step 4.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct GeneanetPreview {
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
    /// `true` when almost no photo matched — the wizard blocks rather than
    /// importing a tree whose photos belong to a different one.
    pub mismatch: bool,
}

#[derive(Debug, Serialize)]
pub struct GeneanetImportBody {
    pub gw_base64: String,
    pub file_name: String,
    pub collection: String,
    pub deposit_sizes: std::collections::HashMap<i64, u64>,
    pub archive_paths: Vec<String>,
    /// Media the login window fetched, keyed by URL — **paths**, not bytes.
    ///
    /// The server never fetches anything itself: no direct request to Geneanet
    /// succeeds. The window writes each medium to a temp directory and this
    /// names them, which keeps the request small however many there are.
    pub fetched: std::collections::HashMap<String, String>,
    pub media_fidelity: MediaFidelity,
}

/// How far a running import has got.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportProgress {
    pub phase: String,
    pub done: usize,
    pub total: usize,
}

/// What the Geneanet import actually did.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GeneanetImportResult {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub media_count: usize,
    /// Higher than `media_count` when a photo shows several people.
    pub links_count: usize,
    /// Links marked as a person's profile photo.
    pub portraits_count: usize,
    /// People created for identifications Geneanet marks "hors de l'arbre".
    pub isolated_count: usize,
    /// Identification boxes kept as regions on the stored pictures.
    pub vignettes_count: usize,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

/// Summary returned by any import, whatever the source format.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportResult {
    pub persons_count: usize,
    pub families_count: usize,
    pub events_count: usize,
    pub sources_count: usize,
    pub media_count: usize,
    pub places_count: usize,
    pub notes_count: usize,
    pub warnings: Vec<String>,
}

/// Pollable state of an asynchronous genealogy file import.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FileImportJobStatus {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub result: Option<ImportResult>,
    pub geneanet_result: Option<GeneanetImportResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportJobStarted {
    pub job_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExportJobStarted {
    pub job_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ExportJobStatus {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub download_url: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportGedcomResult {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

/// Everything one person page renders, with its pictures already resolved to
/// something this platform can draw.
#[derive(Debug, Clone)]
pub struct PersonDetailBundle {
    pub sosa_number: Option<u64>,
    pub persons: Vec<oxidgene_core::types::Person>,
    pub names: Vec<oxidgene_core::types::PersonName>,
    pub events: Vec<oxidgene_core::types::Event>,
    pub places: Vec<oxidgene_core::types::Place>,
    pub spouses: Vec<oxidgene_core::types::FamilySpouse>,
    pub children: Vec<oxidgene_core::types::FamilyChild>,
    pub citations: Vec<oxidgene_core::types::Citation>,
    pub sources: Vec<oxidgene_core::types::Source>,
    pub profile_media: Vec<MediaWithLink>,
    pub profile_vignettes: Vec<Vignette>,
    pub event_media: Vec<EventMediaTile>,
    pub gallery: GalleryBundle,
}

/// The same bundle as the API sends it: its gallery carries addresses.
#[derive(Debug, Clone, Deserialize)]
struct WirePersonDetailBundle {
    sosa_number: Option<u64>,
    persons: Vec<oxidgene_core::types::Person>,
    names: Vec<oxidgene_core::types::PersonName>,
    events: Vec<oxidgene_core::types::Event>,
    places: Vec<oxidgene_core::types::Place>,
    spouses: Vec<oxidgene_core::types::FamilySpouse>,
    children: Vec<oxidgene_core::types::FamilyChild>,
    citations: Vec<oxidgene_core::types::Citation>,
    sources: Vec<oxidgene_core::types::Source>,
    profile_media: Vec<MediaWithLink>,
    profile_vignettes: Vec<Vignette>,
    event_media: Vec<EventMediaTile>,
    gallery: WireGalleryBundle,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EventMediaTile {
    pub event_id: Uuid,
    pub link_id: Uuid,
    pub sort_order: i32,
    #[serde(flatten)]
    pub media: Media,
}

// ── Response Cache ───────────────────────────────────────────────────

const CACHE_TTL_SECS: i64 = 30;

/// In-memory GET response cache with a fixed TTL.
///
/// Keyed by the request URL (path + serialised query string).
/// Values are raw JSON bytes + the Unix timestamp when they were stored.
type CacheInner =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, i64)>>>;

/// One in-flight request per cache key. Losers of the race wait on the gate
/// rather than issuing the same request again.
type GateInner = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
>;

#[derive(Clone, Default)]
struct ResponseCache {
    entries: CacheInner,
    gates: GateInner,
}

impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResponseCache({})",
            self.entries.lock().map(|c| c.len()).unwrap_or(0)
        )
    }
}

impl ResponseCache {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let cache = self.entries.lock().ok()?;
        let (data, ts) = cache.get(key)?;
        let age = chrono::Utc::now().timestamp() - ts;
        if age < CACHE_TTL_SECS {
            Some(data.clone())
        } else {
            None
        }
    }

    fn set(&self, key: String, data: Vec<u8>) {
        if let Ok(mut cache) = self.entries.lock() {
            cache.insert(key, (data, chrono::Utc::now().timestamp()));
        }
    }

    /// Remove all entries whose key starts with `prefix`.
    fn invalidate_prefix(&self, prefix: &str) {
        if let Ok(mut cache) = self.entries.lock() {
            cache.retain(|k, _| !k.starts_with(prefix));
        }
    }

    /// The gate guarding network access for `key`, created on first use.
    fn gate(&self, key: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let Ok(mut gates) = self.gates.lock() else {
            // A poisoned gate map costs a duplicate request, never a wrong
            // answer: fall back to an ungated lock nobody else holds.
            return std::sync::Arc::default();
        };
        gates.entry(key.to_string()).or_default().clone()
    }

    /// Drops the gate for `key` once nothing is waiting on it. Callers must
    /// have released their own handle first, so a remaining reference means
    /// another request is still queued behind this key.
    fn release_gate(&self, key: &str) {
        if let Ok(mut gates) = self.gates.lock()
            && gates
                .get(key)
                .is_some_and(|gate| std::sync::Arc::strong_count(gate) == 1)
        {
            gates.remove(key);
        }
    }
}

// ── API Client ──────────────────────────────────────────────────────

/// Typed HTTP client for the OxidGene REST API.
#[derive(Debug, Clone)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    cache: ResponseCache,
    /// The shell that serves backend-held pictures from its own origin, when
    /// this build has one. Absent on the web, where pictures are fetched here
    /// and handed to the markup as `data:` URLs instead.
    image_host: Option<crate::image_host::ImageHost>,
}

/// Errors returned by the API client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
}

/// Starts the browser save picker during the click, before any network awaits.
#[cfg(target_arch = "wasm32")]
pub(crate) struct BrowserDownload {
    eval: dioxus::document::Eval,
}

#[cfg(target_arch = "wasm32")]
impl BrowserDownload {
    pub fn new(file_name: &str) -> Self {
        let name = serde_json::to_string(file_name).expect("a string is serializable");
        Self {
            eval: dioxus::document::eval(&format!(
                "const fileName = {name};\n{}",
                include_str!("download.js")
            )),
        }
    }

    pub async fn ready(&mut self) -> Result<bool, ApiError> {
        match self.eval.recv::<String>().await.as_deref() {
            Ok("ready") => Ok(true),
            Ok("cancelled") => Ok(false),
            _ => Err(Self::error()),
        }
    }

    fn error() -> ApiError {
        std::io::Error::other("browser download failed").into()
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for BrowserDownload {
    fn drop(&mut self) {
        // Release a pending picker session if export preparation fails.
        let _ = self.eval.send(Option::<String>::None);
    }
}

impl ApiClient {
    async fn read_response_body(response: reqwest::Response) -> Result<Vec<u8>, reqwest::Error> {
        #[cfg(feature = "telemetry-client")]
        {
            let status = response.status().as_u16();
            let expected_size = response.content_length();
            let span = tracing::info_span!(
                "ui.response.read",
                otel.name = "read HTTP response body",
                http.response.status_code = status,
                http.response.body.size = tracing::field::Empty,
                http.response.body.size.expected = expected_size,
                otel.status_code = tracing::field::Empty,
            );
            let result = response.bytes().instrument(span.clone()).await;
            match &result {
                Ok(bytes) => {
                    span.record("http.response.body.size", bytes.len());
                }
                Err(_) => {
                    span.record("otel.status_code", "ERROR");
                }
            }
            result.map(|bytes| bytes.to_vec())
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            response.bytes().await.map(|bytes| bytes.to_vec())
        }
    }

    fn deserialize<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
        #[cfg(feature = "telemetry-client")]
        {
            let span = tracing::info_span!(
                "ui.response.deserialize",
                otel.name = "deserialize JSON response",
                serialization.format = "json",
                response.body.size = bytes.len(),
                otel.status_code = tracing::field::Empty,
            );
            let result = span.in_scope(|| serde_json::from_slice(bytes));
            if result.is_err() {
                span.record("otel.status_code", "ERROR");
            }
            result
        }

        #[cfg(not(feature = "telemetry-client"))]
        {
            serde_json::from_slice(bytes)
        }
    }

    /// Create a new API client pointing at the given base URL.
    ///
    /// The `base_url` should include scheme and port, e.g.
    /// `http://127.0.0.1:3000`.
    pub fn new(base_url: &str) -> Self {
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(std::time::Duration::from_secs(300));
        Self {
            client: builder.build().expect("failed to build reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            cache: ResponseCache::default(),
            image_host: None,
        }
    }

    /// Serve backend-held pictures through `host` rather than encoding them.
    #[must_use]
    pub fn with_image_host(mut self, host: crate::image_host::ImageHost) -> Self {
        self.image_host = Some(host);
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub fn openapi_url(&self) -> String {
        self.url("/api/v1/openapi.json")
    }

    pub fn graphql_url(&self) -> String {
        self.url("/graphql")
    }

    #[cfg(feature = "telemetry-client")]
    async fn send_request(
        &self,
        method: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let span = tracing::info_span!(
            "http.client.request",
            otel.name = method,
            otel.kind = "client",
            http.request.method = method,
            http.response.status_code = tracing::field::Empty,
            otel.status_code = tracing::field::Empty,
        );
        let mut request = request.build()?;
        global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&span.context(), &mut HeaderInjector(request.headers_mut()));
        });

        let response = self.client.execute(request).instrument(span.clone()).await;
        match &response {
            Ok(response) => {
                let status = response.status();
                span.record("http.response.status_code", status.as_u16());
                if status.is_server_error() {
                    span.record("otel.status_code", "ERROR");
                }
            }
            Err(_) => {
                span.record("otel.status_code", "ERROR");
            }
        }
        response
    }

    #[cfg(not(feature = "telemetry-client"))]
    async fn send_request(
        &self,
        _method: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, reqwest::Error> {
        request.send().await
    }

    /// Invalidate all cached responses for a given tree.
    pub fn invalidate_tree(&self, tree_id: Uuid) {
        self.cache
            .invalidate_prefix(&format!("/api/v1/trees/{tree_id}"));
    }

    /// Helper: send a cached GET request and deserialize JSON response.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = self.url(path);
        self.get_deduplicated(path, || self.client.get(&url)).await
    }

    /// Serves `cache_key` from the cache when it is warm, and lets at most one
    /// request per key reach the network at a time.
    ///
    /// Without that gate the cache only ever helps the *next* render: a page
    /// whose components mount together has them all miss the still-empty cache
    /// and all send the same request. Whoever loses the race waits here and
    /// then finds the winner's response already cached.
    async fn get_deduplicated<T: serde::de::DeserializeOwned>(
        &self,
        cache_key: &str,
        request: impl FnOnce() -> reqwest::RequestBuilder,
    ) -> Result<T, ApiError> {
        if let Some(cached) = self.cache.get(cache_key)
            && let Ok(val) = Self::deserialize(&cached)
        {
            tracing::debug!(method = "GET", cached = true, "API request completed");
            return Ok(val);
        }
        let result = {
            let gate = self.cache.gate(cache_key);
            let _guard = gate.lock().await;
            if let Some(cached) = self.cache.get(cache_key)
                && let Ok(val) = Self::deserialize(&cached)
            {
                tracing::debug!(
                    method = "GET",
                    cached = true,
                    coalesced = true,
                    "API request completed"
                );
                Ok(val)
            } else {
                self.fetch_and_cache(cache_key, request()).await
            }
        };
        self.cache.release_gate(cache_key);
        result
    }

    /// Sends one GET, stores its body under `cache_key`, and deserializes it.
    async fn fetch_and_cache<T: serde::de::DeserializeOwned>(
        &self,
        cache_key: &str,
        request: reqwest::RequestBuilder,
    ) -> Result<T, ApiError> {
        let resp = self.send_request("GET", request).await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!(method = "GET", %status, "API request failed");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = Self::read_response_body(resp).await?;
        tracing::debug!(method = "GET", %status, bytes = bytes.len(), "API request completed");
        let val: T = Self::deserialize(&bytes)?;
        self.cache.set(cache_key.to_string(), bytes);
        Ok(val)
    }

    /// Helper: send a cached GET request with query parameters.
    async fn get_with_query<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<T, ApiError> {
        let cache_key = format!(
            "{}?{}",
            path,
            serde_json::to_string(query).unwrap_or_default()
        );
        let url = self.url(path);
        self.get_deduplicated(&cache_key, || self.client.get(&url).query(query))
            .await
    }

    /// Helper: send a POST request with a JSON body.
    async fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("POST", self.client.post(&url).json(body))
            .await?;
        Self::handle_response("POST", resp).await
    }

    /// Helper: send a POST request with a raw binary body.
    ///
    /// Used by importers whose payload is a file whose encoding is the file's
    /// own business (see `import_geneweb`) — wrapping those bytes in JSON would
    /// force them through UTF-8 first.
    async fn post_bytes<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        body: Vec<u8>,
        query: &Q,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let bytes = body.len();
        let resp = self
            .send_request(
                "POST",
                self.client
                    .post(&url)
                    .query(query)
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        tracing::debug!(method = "POST", bytes, "API binary request sent");
        Self::handle_response("POST", resp).await
    }

    /// Helper: send a PUT request with a JSON body.
    async fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("PUT", self.client.put(&url).json(body))
            .await?;
        Self::handle_response("PUT", resp).await
    }

    /// Helper: send a PATCH request with a JSON body.
    async fn patch<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("PATCH", self.client.patch(&url).json(body))
            .await?;
        Self::handle_response("PATCH", resp).await
    }

    /// Send a DELETE request and return its successful status code.
    async fn delete_status(&self, path: &str) -> Result<u16, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("DELETE", self.client.delete(&url))
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!(method = "DELETE", %status, "API request failed");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        tracing::debug!(method = "DELETE", %status, "API request completed");
        Ok(status.as_u16())
    }

    async fn delete_no_content(&self, path: &str) -> Result<(), ApiError> {
        self.delete_status(path).await.map(|_| ())
    }

    async fn delete_no_content_with_body<B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(), ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("DELETE", self.client.delete(&url).json(body))
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ApiError::Api {
                status: status.as_u16(),
                body: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Handle HTTP response: check status, parse JSON.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        method: &str,
        resp: reqwest::Response,
    ) -> Result<T, ApiError> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!(method, %status, "API request failed");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = Self::read_response_body(resp).await?;
        tracing::debug!(method, %status, bytes = bytes.len(), "API request completed");
        Ok(Self::deserialize(&bytes)?)
    }

    // ── Trees ───────────────────────────────────────────────────────

    pub async fn list_trees(
        &self,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<TreeListItem>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query("/api/v1/trees", &params).await
    }

    /// Force the next home-page tree list request to observe live job state.
    pub fn invalidate_tree_list(&self) {
        self.cache.invalidate_prefix("/api/v1/trees");
    }

    pub async fn get_tree(&self, id: Uuid) -> Result<Tree, ApiError> {
        self.get(&format!("/api/v1/trees/{id}")).await
    }

    pub async fn create_tree(&self, body: &CreateTreeBody) -> Result<Tree, ApiError> {
        let result = self.post("/api/v1/trees", body).await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn update_tree(&self, id: Uuid, body: &UpdateTreeBody) -> Result<Tree, ApiError> {
        let result = self.put(&format!("/api/v1/trees/{id}"), body).await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn duplicate_tree(
        &self,
        id: Uuid,
        body: &DuplicateTreeBody,
    ) -> Result<Tree, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{id}/duplicate"), body)
            .await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(result)
    }

    pub async fn delete_tree(&self, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{id}"))
            .await?;
        self.cache.invalidate_prefix("/api/v1/trees");
        Ok(())
    }

    pub async fn get_person_detail_bundle(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonDetailBundle, ApiError> {
        let wire: WirePersonDetailBundle = self
            .get(&format!(
                "/api/v1/trees/{tree_id}/persons/{person_id}/detail-bundle"
            ))
            .await?;
        Ok(PersonDetailBundle {
            gallery: self.resolve_gallery(tree_id, wire.gallery).await,
            sosa_number: wire.sosa_number,
            persons: wire.persons,
            names: wire.names,
            events: wire.events,
            places: wire.places,
            spouses: wire.spouses,
            children: wire.children,
            citations: wire.citations,
            sources: wire.sources,
            profile_media: wire.profile_media,
            profile_vignettes: wire.profile_vignettes,
            event_media: wire.event_media,
        })
    }

    // ── Persons ─────────────────────────────────────────────────────

    /// Free-text person search, server-side (Sprint E.6).
    ///
    /// Backed by the `person_search_fts` DB table (SQLite FTS5 / PostgreSQL):
    /// accent-folded, every word of the query must match (prefix matching on
    /// SQLite). An empty query lists persons sorted by name (browse mode).
    pub async fn search_persons(
        &self,
        tree_id: Uuid,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResult, ApiError> {
        let params = [
            ("q", query.to_string()),
            ("limit", limit.to_string()),
            ("offset", offset.to_string()),
        ];
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons/search"), &params)
            .await
    }

    pub async fn search_persons_filtered(
        &self,
        tree_id: Uuid,
        search: &PersonSearchParams,
    ) -> Result<SearchResult, ApiError> {
        let mut params = vec![
            ("q", search.query.clone()),
            ("limit", search.limit.to_string()),
            ("offset", search.offset.to_string()),
            ("sort", search.sort.as_str().to_string()),
        ];
        for (name, value) in [
            ("surname", search.surname.as_deref()),
            ("given_names", search.given_names.as_deref()),
            ("occupation", search.occupation.as_deref()),
            ("spouse_surname", search.spouse_surname.as_deref()),
            ("spouse_given_names", search.spouse_given_names.as_deref()),
            ("father_surname", search.father_surname.as_deref()),
            ("father_given_names", search.father_given_names.as_deref()),
            ("mother_surname", search.mother_surname.as_deref()),
            ("mother_given_names", search.mother_given_names.as_deref()),
            ("place", search.place.as_deref()),
        ] {
            if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
                params.push((name, value.trim().to_string()));
            }
        }
        for (name, value) in [
            ("birth_from", search.birth_from),
            ("birth_to", search.birth_to),
            ("death_from", search.death_from),
            ("death_to", search.death_to),
            ("event_from", search.event_from),
            ("event_to", search.event_to),
        ] {
            if let Some(value) = value {
                params.push((name, value.to_string()));
            }
        }
        if let Some(sex) = search.sex {
            params.push(("sex", sex.to_string()));
        }
        if let Some(event_type) = search.event_type {
            params.push(("event_type", event_type.to_string()));
        }
        if search.has_media {
            params.push(("has_media", "true".to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons/search"), &params)
            .await
    }

    pub async fn list_persons(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Person>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/persons"), &params)
            .await
    }

    pub async fn get_person(&self, tree_id: Uuid, id: Uuid) -> Result<PersonDetail, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await
    }

    pub async fn get_person_profile(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<PersonProfile, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/profiles/{person_id}"))
            .await
    }

    /// Resolve a SOSA-Stradonitz number to a person, relative to the tree's
    /// configured SOSA root. Errors (including "not found") should be
    /// treated as a cue to fall back to a normal name search.
    pub async fn get_person_by_sosa(
        &self,
        tree_id: Uuid,
        number: u64,
    ) -> Result<PersonDetail, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/sosa/{number}"))
            .await
    }

    pub async fn create_person(
        &self,
        tree_id: Uuid,
        body: &CreatePersonBody,
    ) -> Result<Person, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/persons"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_person(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePersonBody,
    ) -> Result<Person, ApiError> {
        let result = self
            .put(&format!("/api/v1/trees/{tree_id}/persons/{id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_person(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    pub async fn get_ancestors(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<AncestryLink>, ApiError> {
        let mut params = Vec::new();
        if let Some(d) = max_depth {
            params.push(("max_depth", d.to_string()));
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/ancestors"),
            &params,
        )
        .await
    }

    pub async fn get_descendants(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        max_depth: Option<i32>,
    ) -> Result<Vec<AncestryLink>, ApiError> {
        let mut params = Vec::new();
        if let Some(d) = max_depth {
            params.push(("max_depth", d.to_string()));
        }
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/persons/{person_id}/descendants"),
            &params,
        )
        .await
    }

    // ── Person Names ────────────────────────────────────────────────

    pub async fn list_person_names(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<PersonName>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names"
        ))
        .await
    }

    /// Load names and spouse links in bounded batches, issuing as many
    /// requests as needed for a larger logical set.
    pub async fn relation_labels(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
        family_ids: &[Uuid],
    ) -> Result<RelationLabels, ApiError> {
        let mut labels = RelationLabels::default();
        for (person_range, family_range) in
            relation_label_batch_ranges(person_ids.len(), family_ids.len())
        {
            let body = RelationLabelsRequest {
                person_ids: person_ids[person_range].to_vec(),
                family_ids: family_ids[family_range].to_vec(),
            };
            let batch = self
                .post::<RelationLabels, _>(
                    &format!("/api/v1/trees/{tree_id}/relation-labels"),
                    &body,
                )
                .await?;
            labels.names.extend(batch.names);
            labels.spouses.extend(batch.spouses);
        }
        Ok(labels)
    }

    pub async fn create_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        body: &CreatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
        body: &UpdatePersonNameBody,
    ) -> Result<PersonName, ApiError> {
        let result = self
            .put(
                &format!("/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_person_name(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        name_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/persons/{person_id}/names/{name_id}"
        ))
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Families ────────────────────────────────────────────────────

    pub async fn list_families(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Family>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/families"), &params)
            .await
    }

    pub async fn get_family(&self, tree_id: Uuid, id: Uuid) -> Result<Family, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await
    }

    pub async fn create_family(&self, tree_id: Uuid) -> Result<Family, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/families"),
                &serde_json::json!({}),
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    /// Set a couple's privacy.
    pub async fn update_family_privacy(
        &self,
        tree_id: Uuid,
        id: Uuid,
        privacy: Privacy,
    ) -> Result<Family, ApiError> {
        let family = self
            .put(
                &format!("/api/v1/trees/{tree_id}/families/{id}"),
                &serde_json::json!({ "privacy": privacy }),
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(family)
    }

    pub async fn delete_family(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/families/{id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Family Spouses ──────────────────────────────────────────────

    pub async fn list_family_spouses(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilySpouse>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/spouses"
        ))
        .await
    }

    pub async fn add_spouse(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddSpouseBody,
    ) -> Result<serde_json::Value, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/families/{family_id}/spouses"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn remove_spouse(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        spouse_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/spouses/{spouse_id}"
        ))
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Family Children ─────────────────────────────────────────────

    pub async fn list_family_children(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
    ) -> Result<Vec<FamilyChild>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children"
        ))
        .await
    }

    pub async fn add_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        body: &AddChildBody,
    ) -> Result<serde_json::Value, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/families/{family_id}/children"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn remove_child(
        &self,
        tree_id: Uuid,
        family_id: Uuid,
        child_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/families/{family_id}/children/{child_id}"
        ))
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Events ──────────────────────────────────────────────────────

    pub async fn list_events(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
        event_type: Option<EventType>,
        person_id: Option<Uuid>,
        family_id: Option<Uuid>,
    ) -> Result<PaginatedResponse<Event>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        if let Some(et) = event_type {
            params.push((
                "event_type",
                serde_json::to_string(&et)
                    .unwrap()
                    .trim_matches('"')
                    .to_string(),
            ));
        }
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/events"), &params)
            .await
    }

    pub async fn get_event(&self, tree_id: Uuid, id: Uuid) -> Result<Event, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/events/{id}"))
            .await
    }

    pub async fn create_event(
        &self,
        tree_id: Uuid,
        body: &CreateEventBody,
    ) -> Result<Event, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/events"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_event(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateEventBody,
    ) -> Result<Event, ApiError> {
        let result = self
            .put(&format!("/api/v1/trees/{tree_id}/events/{id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_event(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/events/{id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Event Witnesses ────────────────────────────────────────────────

    pub async fn list_event_witnesses(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
    ) -> Result<Vec<EventWitness>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/events/{event_id}/witnesses"
        ))
        .await
    }

    pub async fn add_event_witness(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
        body: &AddEventWitnessBody,
    ) -> Result<EventWitness, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/events/{event_id}/witnesses"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn remove_event_witness(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
        witness_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/events/{event_id}/witnesses/{witness_id}"
        ))
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Places ──────────────────────────────────────────────────────

    pub async fn list_places(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
        search: Option<&str>,
    ) -> Result<PaginatedResponse<Place>, ApiError> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        if let Some(s) = search {
            params.push(("search", s.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/places"), &params)
            .await
    }

    /// Fetch all places by paginating through all pages.
    pub async fn list_all_places(&self, tree_id: Uuid) -> Result<Vec<Place>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_places(tree_id, Some(500), cursor.as_deref(), None)
                .await?;
            all.extend(page.edges.into_iter().map(|e| e.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(all)
    }

    pub async fn get_place(&self, tree_id: Uuid, id: Uuid) -> Result<Place, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/places/{id}"))
            .await
    }

    pub async fn create_place(
        &self,
        tree_id: Uuid,
        body: &CreatePlaceBody,
    ) -> Result<Place, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/places"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_place(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdatePlaceBody,
    ) -> Result<Place, ApiError> {
        let result = self
            .put(&format!("/api/v1/trees/{tree_id}/places/{id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_place(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/places/{id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Sources ─────────────────────────────────────────────────────

    pub async fn list_sources(
        &self,
        tree_id: Uuid,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Source>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/sources"), &params)
            .await
    }

    /// Fetch all sources by paginating through all pages.
    pub async fn list_all_sources(&self, tree_id: Uuid) -> Result<Vec<Source>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_sources(tree_id, Some(500), cursor.as_deref())
                .await?;
            all.extend(page.edges.into_iter().map(|e| e.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(all)
    }

    pub async fn get_source(&self, tree_id: Uuid, id: Uuid) -> Result<Source, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/sources/{id}"))
            .await
    }

    pub async fn create_source(
        &self,
        tree_id: Uuid,
        body: &CreateSourceBody,
    ) -> Result<Source, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/sources"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_source(
        &self,
        tree_id: Uuid,
        id: Uuid,
        body: &UpdateSourceBody,
    ) -> Result<Source, ApiError> {
        let result = self
            .put(&format!("/api/v1/trees/{tree_id}/sources/{id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_source(&self, tree_id: Uuid, id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/sources/{id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    /// Deletes a source only if no citation, note or media link still points
    /// at it. Returns whether it was deleted — `false` means it is still in
    /// use and was kept.
    pub async fn delete_source_if_unused(&self, tree_id: Uuid, id: Uuid) -> Result<bool, ApiError> {
        let status = self
            .delete_status(&format!(
                "/api/v1/trees/{tree_id}/sources/{id}?only_if_unused=true"
            ))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(status == 204)
    }

    // ── Dictionary ───────────────────────────────────────────────────

    /// Distinct surnames in the tree, with the number of persons carrying each.
    pub async fn dictionary_family_names(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/family-names"))
            .await
    }

    /// Persons carrying a given family name.
    pub async fn dictionary_family_name_usage(
        &self,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/family-names/usage"),
            &[("value", value)],
        )
        .await
    }

    /// Re-cut every occurrence of a family name at `particle` — the bulk
    /// repair for an import that guessed the particle wrong across a whole
    /// family. An empty `particle` means "this name has no particle".
    pub async fn set_family_name_particle(
        &self,
        tree_id: Uuid,
        value: &str,
        particle: &str,
    ) -> Result<FamilyNameParticleUpdate, ApiError> {
        let result = self
            .patch(
                &format!("/api/v1/trees/{tree_id}/dictionary/family-names/particle"),
                &SetFamilyNameParticleBody {
                    value: value.to_string(),
                    particle: particle.to_string(),
                },
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    /// Distinct occupation labels in the tree, with the number of persons holding each.
    pub async fn dictionary_occupations(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<DictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/occupations"))
            .await
    }

    /// Sources in the tree whose title starts with `prefix` (empty = all),
    /// each paired with its citation count. Used as the final flat-list step
    /// of the Sources tab's smart drill-down once a prefix's count is small
    /// enough to display directly (see ui-dictionary.md §8).
    pub async fn dictionary_sources(
        &self,
        tree_id: Uuid,
        prefix: &str,
    ) -> Result<Vec<SourceDictionaryEntry>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/sources"),
            &[("prefix", prefix)],
        )
        .await
    }

    /// Resolves the Sources tab's smart drill-down starting from `prefix`
    /// (empty = start from the top): the backend auto-skips forced
    /// single-choice levels and returns either the real next branch
    /// choices, or an empty `groups` list once the count is small enough to
    /// fetch the final flat list (via `dictionary_sources`, passing back
    /// the response's `prefix`). See ui-dictionary.md §8.10.
    pub async fn dictionary_source_groups(
        &self,
        tree_id: Uuid,
        prefix: &str,
    ) -> Result<SourceDrillResponse, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/sources/groups"),
            &[("prefix", prefix)],
        )
        .await
    }

    /// All places in the tree, each paired with its usage count.
    pub async fn dictionary_places(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<PlaceDictionaryEntry>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/dictionary/places"))
            .await
    }

    /// Persons citing a given source.
    pub async fn dictionary_source_usage(
        &self,
        tree_id: Uuid,
        source_id: Uuid,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/dictionary/sources/{source_id}/usage"
        ))
        .await
    }

    /// Persons with an event at a given place.
    pub async fn dictionary_place_usage(
        &self,
        tree_id: Uuid,
        place_id: Uuid,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/dictionary/places/{place_id}/usage"
        ))
        .await
    }

    /// Persons holding a given occupation label.
    pub async fn dictionary_occupation_usage(
        &self,
        tree_id: Uuid,
        value: &str,
    ) -> Result<Vec<PersonUsageEntry>, ApiError> {
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/dictionary/occupations/usage"),
            &[("value", value)],
        )
        .await
    }

    // ── Citations ────────────────────────────────────────────────────

    pub async fn create_citation(
        &self,
        tree_id: Uuid,
        body: &CreateCitationBody,
    ) -> Result<Citation, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/citations"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_citation(
        &self,
        tree_id: Uuid,
        citation_id: Uuid,
        body: &UpdateCitationBody,
    ) -> Result<Citation, ApiError> {
        let result = self
            .put(
                &format!("/api/v1/trees/{tree_id}/citations/{citation_id}"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_citation(&self, tree_id: Uuid, citation_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/citations/{citation_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    pub async fn list_citations(
        &self,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
    ) -> Result<Vec<Citation>, ApiError> {
        let mut citations = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = vec![("first", "100".to_string())];
            if let Some(person_id) = person_id {
                params.push(("person_id", person_id.to_string()));
            }
            if let Some(event_id) = event_id {
                params.push(("event_id", event_id.to_string()));
            }
            if let Some(family_id) = family_id {
                params.push(("family_id", family_id.to_string()));
            }
            if let Some(source_id) = source_id {
                params.push(("source_id", source_id.to_string()));
            }
            if let Some(after) = cursor.as_ref() {
                params.push(("after", after.clone()));
            }
            let page: PaginatedResponse<Citation> = self
                .get_with_query(&format!("/api/v1/trees/{tree_id}/citations"), &params)
                .await?;
            citations.extend(page.edges.into_iter().map(|edge| edge.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(citations)
    }

    // ── Notes ─────────────────────────────────────────────────────────

    pub async fn list_notes(
        &self,
        tree_id: Uuid,
        person_id: Option<Uuid>,
        event_id: Option<Uuid>,
        family_id: Option<Uuid>,
        source_id: Option<Uuid>,
        media_id: Option<Uuid>,
    ) -> Result<Vec<Note>, ApiError> {
        let mut notes = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut params = vec![("first", "100".to_string())];
            if let Some(media_id) = media_id {
                params.push(("media_id", media_id.to_string()));
            }
            if let Some(person_id) = person_id {
                params.push(("person_id", person_id.to_string()));
            }
            if let Some(event_id) = event_id {
                params.push(("event_id", event_id.to_string()));
            }
            if let Some(family_id) = family_id {
                params.push(("family_id", family_id.to_string()));
            }
            if let Some(source_id) = source_id {
                params.push(("source_id", source_id.to_string()));
            }
            if let Some(after) = cursor.as_ref() {
                params.push(("after", after.clone()));
            }
            let page: PaginatedResponse<Note> = self
                .get_with_query(&format!("/api/v1/trees/{tree_id}/notes"), &params)
                .await?;
            notes.extend(page.edges.into_iter().map(|edge| edge.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(notes)
    }

    pub async fn create_note(
        &self,
        tree_id: Uuid,
        body: &CreateNoteBody,
    ) -> Result<Note, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/notes"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn update_note(
        &self,
        tree_id: Uuid,
        note_id: Uuid,
        body: &UpdateNoteBody,
    ) -> Result<Note, ApiError> {
        let result = self
            .put(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    pub async fn delete_note(&self, tree_id: Uuid, note_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/notes/{note_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Media ───────────────────────────────────────────────────────

    /// Absolute URL of a media's stored bytes.
    ///
    /// Returned as a URL rather than as bytes because these go straight into
    /// an `<img src>`: letting the engine fetch them means it also gets the
    /// `ETag` revalidation the endpoint offers, which pulling them through
    /// this client would throw away.
    /// Fetch a file's raw bytes and its content type.
    ///
    /// Public so a shell that serves pictures from its own origin can answer
    /// through the same client, connection pool and tracing as every other
    /// request, rather than opening a second path to the backend.
    pub async fn get_binary(&self, path: &str) -> Result<(Vec<u8>, String), ApiError> {
        let response = self
            .send_request("GET", self.client.get(self.url(path)))
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::debug!(method = "GET", %status, "API binary request failed");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = Self::read_response_body(response).await?;
        tracing::debug!(method = "GET", %status, bytes = bytes.len(), "API binary request completed");
        Ok((bytes, content_type))
    }

    async fn get_binary_data_url(&self, path: &str) -> Result<String, ApiError> {
        let (bytes, content_type) = self.get_binary(path).await?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(format!("data:{content_type};base64,{encoded}"))
    }

    /// Load a media through the API client without exposing its endpoint as a
    /// browser or WebView navigation target.
    pub async fn media_file_data_url(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<String, ApiError> {
        self.get_binary_data_url(&format!("/api/v1/trees/{tree_id}/media/{media_id}/file"))
            .await
    }

    /// Turn a picture's address into something this platform can draw.
    ///
    /// A shell that serves pictures from its own origin answers synchronously
    /// with a path; everywhere else the bytes are fetched here and handed over
    /// as a `data:` URL. Either way the markup never carries a backend address.
    async fn resolve_source(&self, tree_id: Uuid, source: ImageSource) -> Option<String> {
        if let ImageSource::Remote { url } = source {
            return Some(url);
        }
        if let Some(host) = &self.image_host
            && let Some(path) = host.path(tree_id, &source)
        {
            return Some(path);
        }
        let path = crate::image_host::api_path(tree_id, &source)?;
        match self.get_binary_data_url(&path).await {
            Ok(url) => Some(url),
            Err(error) => {
                tracing::warn!(%error, %path, "picture could not be loaded");
                None
            }
        }
    }

    /// Resolve every address in a gallery to something drawable.
    async fn resolve_gallery(&self, tree_id: Uuid, wire: WireGalleryBundle) -> GalleryBundle {
        let mut bundle = GalleryBundle::default();
        for item in wire.media {
            let mut previews = Vec::with_capacity(item.document_previews.len());
            for preview in item.document_previews {
                if let Some(source) = self.resolve_source(tree_id, preview).await {
                    previews.push(source);
                }
            }
            let source = match item.source {
                Some(source) => self.resolve_source(tree_id, source).await,
                None => None,
            };
            bundle.media.push(GalleryMedia {
                media_id: item.media_id,
                source,
                event_ids: item.event_ids,
                document_previews: previews,
            });
        }
        for item in wire.vignettes {
            if let Some(image) = self.resolve_cropped(tree_id, item.image).await {
                bundle.vignettes.push(GalleryVignette {
                    vignette_id: item.vignette_id,
                    image,
                });
            }
        }
        bundle
    }

    async fn resolve_cropped(
        &self,
        tree_id: Uuid,
        wire: WireCroppedSource,
    ) -> Option<CroppedSource> {
        Some(CroppedSource {
            source: self.resolve_source(tree_id, wire.source).await?,
            crop: wire.crop,
        })
    }

    /// Load portraits in bounded batches, issuing as many batches as needed.
    pub async fn portrait_map_for_ids(
        &self,
        tree_id: Uuid,
        person_ids: &[Uuid],
    ) -> HashMap<Uuid, CroppedSource> {
        let mut portraits = HashMap::new();
        for person_ids in portrait_batches(person_ids) {
            let body = PortraitImagesRequest {
                person_ids: person_ids.to_vec(),
            };
            match self
                .post::<Vec<WirePortraitImage>, _>(
                    &format!("/api/v1/trees/{tree_id}/portrait-images"),
                    &body,
                )
                .await
            {
                Ok(images) => {
                    for image in images {
                        if let Some(resolved) = self.resolve_cropped(tree_id, image.image).await {
                            portraits.insert(image.person_id, resolved);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, count = person_ids.len(), "portrait image batch could not be loaded");
                }
            }
        }
        portraits
    }

    /// Choose what represents a person — a media, a crop of one, or nothing.
    pub async fn set_person_portrait(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
        portrait: SetPortraitBody,
    ) -> Result<serde_json::Value, ApiError> {
        let person = self
            .put(
                &format!("/api/v1/trees/{tree_id}/persons/{person_id}/portrait"),
                &portrait,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(person)
    }

    /// One media's metadata.
    pub async fn get_media(&self, tree_id: Uuid, media_id: Uuid) -> Result<Media, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/media/{media_id}"))
            .await
    }

    /// Load a generated thumbnail without exposing its API URL.
    pub async fn media_thumbnail_data_url(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<String, ApiError> {
        self.get_binary_data_url(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/thumbnail"
        ))
        .await
    }

    /// Load a vignette image without exposing its API URL.
    pub async fn vignette_image_data_url(
        &self,
        tree_id: Uuid,
        vignette_id: Uuid,
    ) -> Result<String, ApiError> {
        self.get_binary_data_url(&format!(
            "/api/v1/trees/{tree_id}/vignettes/{vignette_id}/image"
        ))
        .await
    }

    /// Upload a file and record it.
    ///
    /// `attach_to` fills in an existing record that named a file without
    /// holding it — the state every GEDCOM import leaves behind — instead of
    /// creating a new one.
    pub async fn upload_media(
        &self,
        tree_id: Uuid,
        upload: MediaUpload,
    ) -> Result<Media, ApiError> {
        let url = self.url(&format!("/api/v1/trees/{tree_id}/media/upload"));
        let MediaUpload {
            file_name,
            bytes,
            title,
            description,
            attach_to,
            as_page_of,
        } = upload;
        tracing::debug!(
            method = "POST",
            bytes = bytes.len(),
            "API media upload started"
        );

        let part = reqwest::multipart::Part::bytes(bytes).file_name(file_name);
        let mut form = reqwest::multipart::Form::new().part("file", part);
        if let Some(title) = title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            form = form.text("title", title.to_string());
        }
        if let Some(description) = description
            .as_deref()
            .map(str::trim)
            .filter(|d| !d.is_empty())
        {
            form = form.text("description", description.to_string());
        }
        if let Some(media_id) = attach_to {
            form = form.text("media_id", media_id.to_string());
        }
        if let Some(document_id) = as_page_of {
            form = form.text("document_id", document_id.to_string());
        }

        let resp = self
            .send_request("POST", self.client.post(&url).multipart(form))
            .await?;
        let media = Self::handle_response("POST", resp).await?;
        self.invalidate_tree(tree_id);
        Ok(media)
    }

    /// Create an empty multi-page document.
    ///
    /// Pages are added by uploading images with `document_id` set; the
    /// document itself holds the title, date, place, description and note that
    /// describe the whole thing.
    pub async fn create_media_document(
        &self,
        tree_id: Uuid,
        title: Option<&str>,
    ) -> Result<Media, ApiError> {
        let media = self
            .post(
                &format!("/api/v1/trees/{tree_id}/media/document"),
                &serde_json::json!({ "title": title }),
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(media)
    }

    /// The pages of a document, in order.
    pub async fn list_media_pages(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<Media>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/media/{media_id}/pages"))
            .await
    }

    /// Set a document's page order. Must name exactly its pages, once each.
    pub async fn reorder_media_pages(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        page_ids: &[Uuid],
    ) -> Result<Vec<Media>, ApiError> {
        let pages = self
            .put(
                &format!("/api/v1/trees/{tree_id}/media/{media_id}/pages"),
                &serde_json::json!({ "page_ids": page_ids }),
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(pages)
    }

    /// Delete a document page and its external relations.
    pub async fn delete_media_page(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        page_id: Uuid,
    ) -> Result<(), ApiError> {
        self.delete_no_content(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/pages/{page_id}"
        ))
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    /// Update a media's title and description.
    pub async fn update_media(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        body: &UpdateMediaBody,
    ) -> Result<Media, ApiError> {
        let media = self
            .put(&format!("/api/v1/trees/{tree_id}/media/{media_id}"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(media)
    }

    /// Add one media tag without replacing the other tags.
    pub async fn add_media_tag(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        tag: String,
    ) -> Result<Media, ApiError> {
        let media = self
            .post(
                &format!("/api/v1/trees/{tree_id}/media/{media_id}/tags"),
                &MediaTagBody { tag },
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(media)
    }

    /// Remove one media tag without replacing the other tags.
    pub async fn remove_media_tag(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        tag: String,
    ) -> Result<(), ApiError> {
        self.delete_no_content_with_body(
            &format!("/api/v1/trees/{tree_id}/media/{media_id}/tags"),
            &MediaTagBody { tag },
        )
        .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    /// Permanently delete a media record and its associated information.
    pub async fn delete_media(&self, tree_id: Uuid, media_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/media/{media_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    /// Delete a media only when the supplied gallery link is its sole external
    /// reference. Returns `false` when another reference keeps it alive.
    pub async fn delete_media_if_unreferenced_elsewhere(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<bool, ApiError> {
        let status = self
            .delete_status(&format!(
                "/api/v1/trees/{tree_id}/media/{media_id}?only_if_unreferenced_elsewhere=true&allowed_link_id={allowed_link_id}"
            ))
            .await?;
        if status == 204 {
            self.invalidate_tree(tree_id);
        }
        Ok(status == 204)
    }

    /// Whether the supplied gallery link is the media's sole external
    /// reference, and therefore whether showing a definitive-delete
    /// confirmation is truthful.
    pub async fn can_delete_media_if_unreferenced_elsewhere(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        allowed_link_id: Uuid,
    ) -> Result<bool, ApiError> {
        let status: MediaDeletionStatus = self
            .get(&format!(
                "/api/v1/trees/{tree_id}/media/{media_id}/deletion-status?allowed_link_id={allowed_link_id}"
            ))
            .await?;
        Ok(status.can_delete)
    }

    // ── MediaLinks ──────────────────────────────────────────────────

    /// Fetch all media links for persons in a tree (for photo display).
    pub async fn list_media_links_for_tree(
        &self,
        tree_id: Uuid,
    ) -> Result<Vec<MediaLinkRow>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/media-links"))
            .await
    }

    /// Load gallery thumbnails, mosaics, crops and event links in bounded batches.
    pub async fn gallery_bundle(
        &self,
        tree_id: Uuid,
        media_ids: &[Uuid],
        vignette_ids: &[Uuid],
    ) -> GalleryBundle {
        const BATCH_SIZE: usize = 1_024;

        let mut bundle = GalleryBundle::default();
        let (mut media_offset, mut vignette_offset) = (0, 0);
        while media_offset < media_ids.len() || vignette_offset < vignette_ids.len() {
            let media_end = (media_offset + BATCH_SIZE).min(media_ids.len());
            let remaining = BATCH_SIZE - (media_end - media_offset);
            let vignette_end = (vignette_offset + remaining).min(vignette_ids.len());
            let body = GalleryBundleRequest {
                media_ids: media_ids[media_offset..media_end].to_vec(),
                vignette_ids: vignette_ids[vignette_offset..vignette_end].to_vec(),
            };
            match self
                .post::<WireGalleryBundle, _>(
                    &format!("/api/v1/trees/{tree_id}/gallery-bundle"),
                    &body,
                )
                .await
            {
                Ok(batch) => {
                    let batch = self.resolve_gallery(tree_id, batch).await;
                    bundle.media.extend(batch.media);
                    bundle.vignettes.extend(batch.vignettes);
                }
                Err(error) => {
                    tracing::warn!(%error, "gallery bundle could not be loaded");
                }
            }
            media_offset = media_end;
            vignette_offset = vignette_end;
        }
        bundle
    }

    /// Every media attached to one entity — a person, a family, an event or a
    /// source — with the link that attached it.
    pub async fn list_entity_media(
        &self,
        tree_id: Uuid,
        entity_type: &str,
        entity_id: Uuid,
    ) -> Result<Vec<MediaWithLink>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media-links?entity_type={entity_type}&entity_id={entity_id}"
        ))
        .await
    }

    /// Everything one media file is attached to.
    ///
    /// The other direction from [`Self::list_entity_media`]: what lets a
    /// media's own panel say which events it documents.
    pub async fn list_media_links_of(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<oxidgene_core::types::MediaLink>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media-links?media_id={media_id}"
        ))
        .await
    }

    /// Attach a media to an entity.
    pub async fn create_media_link(
        &self,
        tree_id: Uuid,
        body: &CreateMediaLinkBody,
    ) -> Result<serde_json::Value, ApiError> {
        let link = self
            .post(&format!("/api/v1/trees/{tree_id}/media-links"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(link)
    }

    /// Detach a media from an entity. The media itself is untouched.
    pub async fn delete_media_link(&self, tree_id: Uuid, link_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/media-links/{link_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Vignettes ───────────────────────────────────────────────────

    /// Every crop recorded on a media file, in page order.
    pub async fn list_media_vignettes(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
    ) -> Result<Vec<Vignette>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/vignettes"
        ))
        .await
    }

    /// Crops attributed to a person.
    pub async fn list_person_vignettes(
        &self,
        tree_id: Uuid,
        person_id: Uuid,
    ) -> Result<Vec<Vignette>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/vignettes?person_id={person_id}"
        ))
        .await
    }

    /// Crops standing as evidence for an event.
    pub async fn list_event_vignettes(
        &self,
        tree_id: Uuid,
        event_id: Uuid,
    ) -> Result<Vec<Vignette>, ApiError> {
        self.get(&format!(
            "/api/v1/trees/{tree_id}/vignettes?event_id={event_id}"
        ))
        .await
    }

    pub async fn create_vignette(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        body: &CreateVignetteBody,
    ) -> Result<Vignette, ApiError> {
        let vignette = self
            .post(
                &format!("/api/v1/trees/{tree_id}/media/{media_id}/vignettes"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(vignette)
    }

    pub async fn update_vignette(
        &self,
        tree_id: Uuid,
        vignette_id: Uuid,
        body: &UpdateVignetteBody,
    ) -> Result<Vignette, ApiError> {
        let vignette = self
            .put(
                &format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}"),
                body,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(vignette)
    }

    pub async fn delete_vignette(&self, tree_id: Uuid, vignette_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/vignettes/{vignette_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
    }

    // ── Import / export ─────────────────────────────────────────────

    pub async fn import_gedcom(
        &self,
        tree_id: Uuid,
        gedcom: &str,
    ) -> Result<ImportResult, ApiError> {
        let result = self
            .post(
                &format!("/api/v1/trees/{tree_id}/gedcom/import"),
                &ImportGedcomBody {
                    gedcom: gedcom.to_string(),
                },
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    /// Import a GeneWeb `.gw` file.
    ///
    /// Takes the raw file bytes, never a `String`: `.gw` is ISO-8859-1 unless
    /// the file opts into UTF-8 with an `encoding:` directive, so decoding it
    /// here would mangle accented names. `file_name` is passed through to the
    /// reader, which records it on every family and quotes it in warnings.
    pub async fn import_geneweb(
        &self,
        tree_id: Uuid,
        content: Vec<u8>,
        file_name: &str,
    ) -> Result<ImportResult, ApiError> {
        let query = [("filename", file_name.to_string())];
        let result = self
            .post_bytes(
                &format!("/api/v1/trees/{tree_id}/geneweb/import"),
                content,
                &query,
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    /// Import a GEDZIP archive (`.gdz`) — a ZIP wrapping a GEDCOM together
    /// with the media files it references.
    ///
    /// Takes the raw archive: it is binary, so there is nothing to gain from
    /// wrapping it in JSON and a third of its size to lose. Media the archive
    /// carries are stored as it is read, so a `.gdz` arrives with its
    /// photographs where a `.ged` arrives with their names only.
    pub async fn import_gedzip(
        &self,
        tree_id: Uuid,
        archive: Vec<u8>,
    ) -> Result<ImportResult, ApiError> {
        let result = self
            .post_bytes(
                &format!("/api/v1/trees/{tree_id}/gedzip/import"),
                archive,
                &(),
            )
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
    }

    /// Absolute endpoint used by the browser's XHR upload.
    pub fn file_import_upload_url(&self, tree_id: Uuid) -> String {
        self.url(&format!("/api/v1/trees/{tree_id}/import-jobs"))
    }

    /// Stream a native file to durable import-job storage.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn start_file_import(
        &self,
        tree_id: Uuid,
        format: &str,
        filename: Option<String>,
        body: reqwest::Body,
    ) -> Result<ImportJobStarted, ApiError> {
        let mut query = vec![("format", format.to_string())];
        if let Some(filename) = filename {
            query.push(("filename", filename));
        }
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.file_import_upload_url(tree_id))
                    .query(&query)
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Poll a file import without caching its deliberately changing response.
    pub async fn file_import_status(
        &self,
        tree_id: Uuid,
        job_id: Uuid,
    ) -> Result<FileImportJobStatus, ApiError> {
        let response = self
            .send_request(
                "GET",
                self.client
                    .get(self.url(&format!("/api/v1/trees/{tree_id}/import-jobs/{job_id}"))),
            )
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::Api {
                status: status.as_u16(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        Ok(response.json().await?)
    }

    // ── Geneanet import wizard ──────────────────────────────────────

    /// Parse a `.gw` and report what it holds, writing nothing. Step 1.
    ///
    /// Runs on every selection because it costs nothing and is the first
    /// moment the user learns whether they picked the right export — a `.ged`
    /// fails here rather than four steps later.
    pub async fn inspect_geneweb(
        &self,
        content: Vec<u8>,
        file_name: &str,
    ) -> Result<GwInspection, ApiError> {
        let query = [("filename", file_name.to_string())];
        self.post_bytes("/api/v1/geneweb/inspect", content, &query)
            .await
    }

    /// Index the named data archives in place, extracting nothing. Step 2.
    ///
    /// Desktop only: it sends **paths**, which is sound because there the
    /// server runs in-process on the same filesystem the user picked from.
    pub async fn index_geneanet_archives(
        &self,
        paths: Vec<String>,
    ) -> Result<ArchiveIndex, ApiError> {
        self.post("/api/v1/geneanet/archives", &IndexArchivesBody { paths })
            .await
    }

    /// Join the collected mapping onto the `.gw` and report what an import
    /// would do, without doing it. Step 4.
    pub async fn preview_geneanet_import(
        &self,
        body: &GeneanetPreviewBody,
    ) -> Result<GeneanetPreview, ApiError> {
        self.post("/api/v1/geneanet/preview", body).await
    }

    /// Encode a collected session as the JSON the wizard writes to disk.
    ///
    /// Done server-side so the file format lives in one place — the same
    /// module the loader validates against — rather than being assembled by
    /// hand in the UI.
    pub async fn encode_geneanet_session(
        &self,
        body: &GeneanetSessionBody,
    ) -> Result<Vec<u8>, ApiError> {
        // The archive itself, not JSON around it: the wizard writes these
        // bytes straight to the file the user chose, and wrapping a ZIP in
        // JSON would only base64 it again — the very thing the container
        // exists to stop.
        let url = self.url("/api/v1/geneanet/session/encode");
        let resp = self
            .send_request("POST", self.client.post(&url).json(body))
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(ApiError::Api {
                status,
                body: resp.text().await.unwrap_or_default(),
            });
        }

        Self::read_response_body(resp).await.map_err(ApiError::from)
    }

    /// Read a saved session back, checking it really is one.
    pub async fn decode_geneanet_session(
        &self,
        body: reqwest::Body,
    ) -> Result<GeneanetSession, ApiError> {
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.url("/api/v1/geneanet/session/decode"))
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(body),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Ask what the login window has to fetch before an import can run.
    ///
    /// The server never reaches Geneanet — every direct request is challenged
    /// whatever the cookie — so anything it cannot find in the local archives
    /// has to come through the window the user signed in to.
    pub async fn plan_geneanet_import(
        &self,
        body: &GeneanetPreviewBody,
    ) -> Result<GeneanetPlan, ApiError> {
        self.post("/api/v1/geneanet/plan", body).await
    }

    /// Stage every local input and queue the Geneanet import. Step 5.
    pub async fn import_geneanet(
        &self,
        tree_id: Uuid,
        body: &GeneanetImportBody,
    ) -> Result<ImportJobStarted, ApiError> {
        self.post(&format!("/api/v1/trees/{tree_id}/geneanet/import"), body)
            .await
    }

    /// `merge_occupations` collapses each person's multiple `OCCU` tags back
    /// into one, comma-separated (for importers, e.g. Geneanet, that only
    /// support a single profession field). `merge_names` collapses each
    /// person's non-primary names into the primary name's `SURN` tag,
    /// comma-separated (for importers, e.g. Geneanet, that only read the
    /// first `NAME` structure).
    pub async fn export_gedcom(
        &self,
        tree_id: Uuid,
        merge_occupations: bool,
        merge_names: bool,
    ) -> Result<ExportGedcomResult, ApiError> {
        let query = [
            ("merge_occupations", merge_occupations.to_string()),
            ("merge_names", merge_names.to_string()),
        ];
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/gedcom/export"), &query)
            .await
    }

    /// Queue a GEDZIP export without holding the HTTP request while it is built.
    pub async fn start_export_job(
        &self,
        tree_id: Uuid,
        merge_occupations: bool,
        merge_names: bool,
    ) -> Result<ExportJobStarted, ApiError> {
        let query = [
            ("merge_occupations", merge_occupations.to_string()),
            ("merge_names", merge_names.to_string()),
        ];
        let response = self
            .send_request(
                "POST",
                self.client
                    .post(self.url(&format!("/api/v1/trees/{tree_id}/export-jobs")))
                    .query(&query),
            )
            .await?;
        Self::handle_response("POST", response).await
    }

    /// Poll an export job without caching its changing response.
    pub async fn export_job_status(
        &self,
        tree_id: Uuid,
        job_id: Uuid,
    ) -> Result<ExportJobStatus, ApiError> {
        let response = self
            .send_request(
                "GET",
                self.client
                    .get(self.url(&format!("/api/v1/trees/{tree_id}/export-jobs/{job_id}"))),
            )
            .await?;
        Self::handle_response("GET", response).await
    }

    fn download_url(&self, path: &str) -> String {
        if oxidgene_core::types::is_remote_url(path) {
            path.to_string()
        } else {
            self.url(path)
        }
    }

    /// Keep browser response bytes outside WASM and the JSON evaluation bridge.
    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn download_in_browser(
        &self,
        download: BrowserDownload,
        path: &str,
    ) -> Result<(), ApiError> {
        download
            .eval
            .send(self.download_url(path))
            .map_err(|_| BrowserDownload::error())?;
        match download.eval.join::<String>().await.as_deref() {
            Ok("saved") => Ok(()),
            _ => Err(BrowserDownload::error()),
        }
    }

    /// Stream a download to disk, replacing the destination only after success.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_to_file(
        &self,
        path: &str,
        destination: &std::path::Path,
    ) -> Result<(), ApiError> {
        use tokio::io::AsyncWriteExt;

        let mut response = self
            .send_request("GET", self.client.get(self.download_url(path)))
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(ApiError::Api {
                status: status.as_u16(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        let pending = tempfile::NamedTempFile::new_in(parent)?;
        let mut file = tokio::fs::File::from_std(pending.reopen()?);
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        pending.persist(destination).map_err(|error| error.error)?;
        Ok(())
    }

    // ── Pedigree Cache ──────────────────────────────────────────────

    /// Helper: send a PATCH request with query parameters (no body).
    async fn patch_with_query<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let resp = self
            .send_request("PATCH", self.client.patch(&url).query(query))
            .await?;
        Self::handle_response("PATCH", resp).await
    }

    /// Fetch a windowed pedigree for a root person.
    ///
    /// Assembled server-side from family links and the stored person
    /// projections on every call.
    /// Assemble several pedigrees in one operation, in bounded batches.
    ///
    /// A screen that draws one small pedigree per row asks for the whole page
    /// at once: a request per row is both slower and drowns the load trace in
    /// one resource per row.
    pub async fn get_pedigrees(
        &self,
        tree_id: Uuid,
        root_person_ids: &[Uuid],
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> HashMap<Uuid, Pedigree> {
        let mut pedigrees = HashMap::new();
        for roots in root_person_ids.chunks(PEDIGREE_BATCH_SIZE) {
            let body = PedigreesRequest {
                root_person_ids: roots.to_vec(),
                ancestor_depth,
                descendant_depth,
            };
            match self
                .post::<Vec<PedigreeEntry>, _>(&format!("/api/v1/trees/{tree_id}/pedigrees"), &body)
                .await
            {
                Ok(entries) => pedigrees.extend(
                    entries
                        .into_iter()
                        .map(|entry| (entry.root_person_id, entry.pedigree)),
                ),
                Err(error) => {
                    tracing::warn!(%error, count = roots.len(), "pedigree batch could not be loaded");
                }
            }
        }
        pedigrees
    }

    pub async fn get_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        ancestor_depth: u32,
        descendant_depth: u32,
    ) -> Result<Pedigree, ApiError> {
        let params = [
            ("ancestor_depth", ancestor_depth.to_string()),
            ("descendant_depth", descendant_depth.to_string()),
        ];
        self.get_with_query(
            &format!("/api/v1/trees/{tree_id}/pedigree/{root_person_id}"),
            &params,
        )
        .await
    }

    /// Expand a pedigree in one direction, returning only the new nodes and
    /// edges (delta).
    ///
    /// `other_depth` is the depth already loaded in the opposite direction —
    /// the server keeps no per-client pedigree state, so it has to be told.
    pub async fn expand_pedigree(
        &self,
        tree_id: Uuid,
        root_person_id: Uuid,
        direction: &str,
        from_depth: u32,
        to_depth: u32,
        other_depth: u32,
    ) -> Result<PedigreeDelta, ApiError> {
        let params = [
            ("direction", direction.to_string()),
            ("from_depth", from_depth.to_string()),
            ("to_depth", to_depth.to_string()),
            ("other_depth", other_depth.to_string()),
        ];
        self.patch_with_query(
            &format!("/api/v1/trees/{tree_id}/pedigree/{root_person_id}/expand"),
            &params,
        )
        .await
    }

    /// Given-name references in bounded batches, issuing as many requests as
    /// needed when `terms` exceeds the server limit.
    pub async fn reference_given_names(
        &self,
        lang: &str,
        terms: &[String],
    ) -> Result<Vec<GivenNameReferenceMatch>, ApiError> {
        let mut matches = Vec::new();
        for terms in reference_term_batches(terms) {
            let mut batch = self
                .post::<Vec<GivenNameReferenceMatch>, _>(
                    &format!("/api/v1/reference/{lang}/given-names/bundle"),
                    &ReferenceTermsBody { terms },
                )
                .await?;
            matches.append(&mut batch);
        }
        Ok(matches)
    }

    /// Occupation references in bounded batches, issuing as many requests as
    /// needed when `terms` exceeds the server limit.
    pub async fn reference_occupations(
        &self,
        lang: &str,
        terms: &[String],
    ) -> Result<Vec<OccupationReferenceMatch>, ApiError> {
        let mut matches = Vec::new();
        for terms in reference_term_batches(terms) {
            let mut batch = self
                .post::<Vec<OccupationReferenceMatch>, _>(
                    &format!("/api/v1/reference/{lang}/occupations/bundle"),
                    &ReferenceTermsBody { terms },
                )
                .await?;
            matches.append(&mut batch);
        }
        Ok(matches)
    }
}

#[cfg(feature = "telemetry-client")]
struct HeaderInjector<'a>(&'a mut reqwest::header::HeaderMap);

#[cfg(feature = "telemetry-client")]
impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_is_recognised_by_its_marker_and_never_spelled_out() {
        // Read as a generic file it lands in `Other`, whose glyph is a folder —
        // which is what an imported photograph drew — and whose badge spells
        // out an internal name nobody outside the codebase has heard of.
        assert_eq!(media_kind(DOCUMENT_MIME), MediaKind::Document);
        assert_eq!(media_kind_label(DOCUMENT_MIME), "DOCUMENT");
        assert_eq!(media_kind("image/jpeg"), MediaKind::Image);
        assert_eq!(media_kind_label("image/jpeg"), "JPEG");
        assert_eq!(media_kind_label("application/pdf"), "PDF");
        assert_eq!(media_kind_label("image/svg+xml"), "SVG");
    }

    #[test]
    fn a_media_is_stored_remote_or_held_by_nobody() {
        let mut media: Media = serde_json::from_value(serde_json::json!({
            "id": Uuid::from_u128(1),
            "tree_id": Uuid::from_u128(2),
            "file_name": "scan.jpg",
            "file_path": "media/scan.jpg",
            "mime_type": "image/jpeg",
            "page_count": 1,
            "file_size": 0,
            "created_at": "2000-01-01T00:00:00Z",
            "updated_at": "2000-01-01T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(media_source(&media), MediaSource::Unheld);
        media.file_path = "https://archives.example.invalid/scan.jpg".to_string();
        assert_eq!(media_source(&media), MediaSource::Remote);
        media.storage_key = Some("tree/scan.jpg".to_string());
        assert_eq!(
            media_source(&media),
            MediaSource::Stored,
            "our own copy wins: the URL is then only where it came from"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn deleting_a_page_accepts_no_content_and_invalidates_the_tree() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = ApiClient::new(&format!("http://{}", listener.local_addr().unwrap()));
        let tree = Uuid::now_v7();
        let document = Uuid::now_v7();
        let page = Uuid::now_v7();
        let cache_key = format!("/api/v1/trees/{tree}/media");
        api.cache.set(cache_key.clone(), b"cached".to_vec());
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = String::new();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            while !request.ends_with("\r\n\r\n") {
                assert!(reader.read_line(&mut request).await.unwrap() > 0);
            }
            assert!(request.starts_with(&format!(
                "DELETE /api/v1/trees/{tree}/media/{document}/pages/{page} "
            )));
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(api.delete_media_page(tree, document, page), server);
        result.unwrap();
        assert!(api.cache.get(&cache_key).is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn downloads_write_chunks_to_disk_before_the_response_finishes() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("download.bin");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = ApiClient::new(&format!("http://{}", listener.local_addr().unwrap()));
        let server = async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = String::new();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            while !request.ends_with("\r\n\r\n") {
                assert!(reader.read_line(&mut request).await.unwrap() > 0);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\npart")
                .await
                .unwrap();

            // The second chunk is withheld until the first reaches disk. A
            // response.bytes() implementation would deadlock until timeout.
            loop {
                let mut files = tokio::fs::read_dir(directory.path()).await.unwrap();
                if let Some(file) = files.next_entry().await.unwrap()
                    && file.metadata().await.unwrap().len() == 4
                {
                    assert!(!destination.exists());
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            socket.write_all(b"done").await.unwrap();
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let (result, ()) =
                tokio::join!(api.download_to_file("/download", &destination), server);
            result.unwrap();
        })
        .await
        .unwrap();
        assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"partdone");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn failed_downloads_preserve_existing_files_and_remove_partial_data() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        for response in [
            &b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\npartial"[..],
        ] {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join("download.bin");
            tokio::fs::write(&destination, b"original").await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let remote = format!("http://{}/download", listener.local_addr().unwrap());
            let api = ApiClient::new("http://unused.invalid");
            let server = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = String::new();
                let mut reader = tokio::io::BufReader::new(&mut socket);
                while !request.ends_with("\r\n\r\n") {
                    assert!(reader.read_line(&mut request).await.unwrap() > 0);
                }
                socket.write_all(response).await.unwrap();
            };
            let (result, ()) = tokio::join!(api.download_to_file(&remote, &destination), server);
            assert!(result.is_err());
            assert_eq!(tokio::fs::read(&destination).await.unwrap(), b"original");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn reference_terms_over_the_limit_are_split_into_subsequent_requests() {
        let terms = (0..129).map(|index| index.to_string()).collect::<Vec<_>>();
        let batches = reference_term_batches(&terms).collect::<Vec<_>>();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 128);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn relation_labels_over_the_limit_are_split_into_subsequent_requests() {
        let batches = relation_label_batch_ranges(1_000, 25);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], (0..1_000, 0..24));
        assert_eq!(batches[1], (1_000..1_000, 24..25));
    }

    #[test]
    fn portraits_over_the_limit_are_split_into_subsequent_requests() {
        let person_ids = (0..1_025).map(|_| Uuid::now_v7()).collect::<Vec<_>>();
        let batches = portrait_batches(&person_ids).collect::<Vec<_>>();

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 1_024);
        assert_eq!(batches[1].len(), 1);
    }
}
