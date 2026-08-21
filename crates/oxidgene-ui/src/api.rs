//! HTTP API client for communicating with the OxidGene backend.
//!
//! Provides a typed client wrapping [`reqwest::Client`] that maps to the
//! REST API defined in `oxidgene-api`.  All methods return domain types
//! from [`oxidgene_core`] directly, since those types already derive
//! `Serialize` / `Deserialize`.

use oxidgene_core::projection::{Pedigree, PedigreeDelta, SearchResult};
use oxidgene_core::types::{
    AncestryLink, Citation, Connection, Event, EventWitness, Family, FamilyChild, FamilySpouse,
    Media, Note, Person, PersonName, Place, QualifiedYear, Source, Tree, Vignette,
};
use oxidgene_core::{
    Calendar, ChildType, Confidence, DateQualifier, DocumentCategory, EventType, NameType, Privacy,
    Sex, SourceMediaType, SpouseRole, TreeDefaultPrivacy,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

// ── Dictionary — distinct-value aggregations with usage counts ──────

/// A distinct free-text value (surname, occupation label) plus how many
/// persons carry it.
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

// ── Tree request bodies ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateTreeBody {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct UpdateTreeBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sosa_root_person_id: Option<Option<Uuid>>,
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

/// One person's portrait, as the tree-wide endpoint returns it.
#[derive(Debug, Clone, Deserialize)]
pub struct PortraitRow {
    pub person_id: uuid::Uuid,
    pub media_id: Option<uuid::Uuid>,
    pub vignette_id: Option<uuid::Uuid>,
    pub file_path: String,
    pub has_thumbnail: bool,
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

impl MediaWithLink {
    /// Which of the three states this media is in.
    pub fn source(&self) -> MediaSource {
        if self.media.storage_key.is_some() {
            MediaSource::Stored
        } else if is_remote(&self.media.file_path) {
            MediaSource::Remote
        } else {
            MediaSource::Unheld
        }
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
        self.media
            .mime_type
            .rsplit('/')
            .next()
            .unwrap_or("file")
            .trim_start_matches("x-")
            .split('+')
            .next()
            .unwrap_or("file")
            .to_uppercase()
    }

    /// What to write under a tile: the title if there is one, else the file name.
    pub fn caption(&self) -> &str {
        match self.media.title.as_deref() {
            Some(title) if !title.trim().is_empty() => title,
            _ => &self.media.file_name,
        }
    }
}

/// Classify a MIME type into what the UI can do with it.
pub fn media_kind(mime_type: &str) -> MediaKind {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("image/") {
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

// ── Vignette DTOs ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CreateVignetteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<uuid::Uuid>,
}

/// The four rectangle fields travel together — send all or none.
#[derive(Debug, Default, Serialize)]
pub struct UpdateVignetteBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<Option<String>>,
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
    /// Names this run so its progress can be polled while it runs.
    pub progress_id: Option<Uuid>,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ExportGedcomResult {
    pub gedcom: String,
    pub warnings: Vec<String>,
}

// ── Tree Snapshot ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct TreeSnapshot {
    pub persons: Vec<oxidgene_core::types::Person>,
    pub names: Vec<oxidgene_core::types::PersonName>,
    pub events: Vec<oxidgene_core::types::Event>,
    pub places: Vec<oxidgene_core::types::Place>,
    pub spouses: Vec<oxidgene_core::types::FamilySpouse>,
    pub children: Vec<oxidgene_core::types::FamilyChild>,
}

// ── Response Cache ───────────────────────────────────────────────────

const CACHE_TTL_SECS: i64 = 30;

/// In-memory GET response cache with a fixed TTL.
///
/// Keyed by the request URL (path + serialised query string).
/// Values are raw JSON bytes + the Unix timestamp when they were stored.
type CacheInner =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (Vec<u8>, i64)>>>;

#[derive(Clone, Default)]
struct ResponseCache(CacheInner);

impl std::fmt::Debug for ResponseCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResponseCache({})",
            self.0.lock().map(|c| c.len()).unwrap_or(0)
        )
    }
}

impl ResponseCache {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        let cache = self.0.lock().ok()?;
        let (data, ts) = cache.get(key)?;
        let age = chrono::Utc::now().timestamp() - ts;
        if age < CACHE_TTL_SECS {
            Some(data.clone())
        } else {
            None
        }
    }

    fn set(&self, key: String, data: Vec<u8>) {
        if let Ok(mut cache) = self.0.lock() {
            cache.insert(key, (data, chrono::Utc::now().timestamp()));
        }
    }

    /// Remove all entries whose key starts with `prefix`.
    fn invalidate_prefix(&self, prefix: &str) {
        if let Ok(mut cache) = self.0.lock() {
            cache.retain(|k, _| !k.starts_with(prefix));
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
}

/// Errors returned by the API client.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("API error ({status}): {body}")]
    Api { status: u16, body: String },
}

impl ApiClient {
    /// Create a new API client pointing at the given base URL.
    ///
    /// The `base_url` should include scheme and port, e.g.
    /// `http://127.0.0.1:3000`.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("failed to build reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            cache: ResponseCache::default(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Invalidate all cached responses for a given tree.
    pub fn invalidate_tree(&self, tree_id: Uuid) {
        self.cache
            .invalidate_prefix(&format!("/api/v1/trees/{tree_id}"));
    }

    /// Helper: send a cached GET request and deserialize JSON response.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        if let Some(cached) = self.cache.get(path)
            && let Ok(val) = serde_json::from_slice(&cached)
        {
            tracing::debug!("GET {} (cached)", path);
            return Ok(val);
        }
        let url = self.url(path);
        tracing::debug!("GET {url}");
        let resp = self.client.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("GET {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?;
        tracing::debug!(
            "GET {url} -> {status} ({} bytes): {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
        let val: T = serde_json::from_slice(&bytes)?;
        self.cache.set(path.to_string(), bytes.to_vec());
        Ok(val)
    }

    /// Helper: GET with query parameters, treating a 404 as `Ok(None)`.
    /// Used for reference-content lookups, where "no fiche for this term
    /// yet" is the expected common case, not an error.
    async fn get_with_query_optional<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<Option<T>, ApiError> {
        match self.get_with_query(path, query).await {
            Ok(val) => Ok(Some(val)),
            Err(ApiError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
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
        if let Some(cached) = self.cache.get(&cache_key)
            && let Ok(val) = serde_json::from_slice(&cached)
        {
            tracing::debug!("GET {} (cached)", cache_key);
            return Ok(val);
        }
        let url = self.url(path);
        tracing::debug!(
            "GET {url} query={}",
            serde_json::to_string(query).unwrap_or_default()
        );
        let resp = self.client.get(&url).query(query).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("GET {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?;
        tracing::debug!(
            "GET {url} -> {status} ({} bytes): {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
        let val: T = serde_json::from_slice(&bytes)?;
        self.cache.set(cache_key, bytes.to_vec());
        Ok(val)
    }

    /// Helper: send a GET request with query parameters, returning the raw
    /// response body bytes (not cached, not JSON-decoded).
    async fn get_bytes_with_query<Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<Vec<u8>, ApiError> {
        let url = self.url(path);
        tracing::debug!(
            "GET {url} query={}",
            serde_json::to_string(query).unwrap_or_default()
        );
        let resp = self.client.get(&url).query(query).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("GET {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?;
        tracing::debug!("GET {url} -> {status} ({} bytes)", bytes.len());
        Ok(bytes.to_vec())
    }

    /// Helper: send a POST request with a JSON body.
    async fn post<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let body_json = serde_json::to_string(body).unwrap_or_default();
        tracing::debug!("POST {url} body={body_json}");
        let resp = self.client.post(&url).json(body).send().await?;
        Self::handle_response(&url, "POST", resp).await
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
        tracing::debug!("POST {url} ({} bytes)", body.len());
        let resp = self
            .client
            .post(&url)
            .query(query)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await?;
        Self::handle_response(&url, "POST", resp).await
    }

    /// Helper: send a PUT request with a JSON body.
    async fn put<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let body_json = serde_json::to_string(body).unwrap_or_default();
        tracing::debug!("PUT {url} body={body_json}");
        let resp = self.client.put(&url).json(body).send().await?;
        Self::handle_response(&url, "PUT", resp).await
    }

    /// Helper: send a PATCH request with a JSON body.
    async fn patch<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        let body_json = serde_json::to_string(body).unwrap_or_default();
        tracing::debug!("PATCH {url} body={body_json}");
        let resp = self.client.patch(&url).json(body).send().await?;
        Self::handle_response(&url, "PATCH", resp).await
    }

    /// Helper: send a DELETE request expecting 204 No Content.
    /// Like [`Self::delete_no_content`], but hands back the status code for
    /// the endpoints that answer with it (see `delete_source_if_unused`).
    async fn delete_status(&self, path: &str) -> Result<u16, ApiError> {
        let url = self.url(path);
        tracing::debug!("DELETE {url}");
        let resp = self.client.delete(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("DELETE {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        tracing::debug!("DELETE {url} -> {status}");
        Ok(status.as_u16())
    }

    /// Helper: send a DELETE whose response carries a body.
    async fn delete_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = self.url(path);
        tracing::debug!("DELETE {url}");
        let resp = self.client.delete(&url).send().await?;
        Self::handle_response(&url, "DELETE", resp).await
    }

    async fn delete_no_content(&self, path: &str) -> Result<(), ApiError> {
        let url = self.url(path);
        tracing::debug!("DELETE {url}");
        let resp = self.client.delete(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("DELETE {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        tracing::debug!("DELETE {url} -> {status}");
        Ok(())
    }

    /// Handle HTTP response: check status, parse JSON.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        url: &str,
        method: &str,
        resp: reqwest::Response,
    ) -> Result<T, ApiError> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::debug!("{method} {url} -> {status} {body}");
            return Err(ApiError::Api {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?;
        tracing::debug!(
            "{method} {url} -> {status} ({} bytes): {}",
            bytes.len(),
            String::from_utf8_lossy(&bytes)
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    // ── Trees ───────────────────────────────────────────────────────

    pub async fn list_trees(
        &self,
        first: Option<u64>,
        after: Option<&str>,
    ) -> Result<PaginatedResponse<Tree>, ApiError> {
        let mut params = Vec::new();
        if let Some(f) = first {
            params.push(("first", f.to_string()));
        }
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        self.get_with_query("/api/v1/trees", &params).await
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

    // ── Tree Snapshot ────────────────────────────────────────────────

    pub async fn get_tree_snapshot(&self, tree_id: Uuid) -> Result<TreeSnapshot, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/snapshot")).await
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

    /// Fetch all persons by paginating through all pages.
    pub async fn list_all_persons(&self, tree_id: Uuid) -> Result<Vec<Person>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_persons(tree_id, Some(500), cursor.as_deref())
                .await?;
            all.extend(page.edges.into_iter().map(|e| e.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(all)
    }

    pub async fn get_person(&self, tree_id: Uuid, id: Uuid) -> Result<PersonDetail, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/persons/{id}"))
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

    /// Fetch all families by paginating through all pages.
    pub async fn list_all_families(&self, tree_id: Uuid) -> Result<Vec<Family>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_families(tree_id, Some(500), cursor.as_deref())
                .await?;
            all.extend(page.edges.into_iter().map(|e| e.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(all)
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

    /// Fetch all events by paginating through all pages.
    pub async fn list_all_events(&self, tree_id: Uuid) -> Result<Vec<Event>, ApiError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_events(tree_id, Some(500), cursor.as_deref(), None, None, None)
                .await?;
            all.extend(page.edges.into_iter().map(|e| e.node));
            if !page.page_info.has_next_page {
                break;
            }
            cursor = page.page_info.end_cursor;
        }
        Ok(all)
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
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(eid) = event_id {
            params.push(("event_id", eid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        if let Some(sid) = source_id {
            params.push(("source_id", sid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/citations"), &params)
            .await
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
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(mid) = media_id {
            params.push(("media_id", mid.to_string()));
        }
        if let Some(pid) = person_id {
            params.push(("person_id", pid.to_string()));
        }
        if let Some(eid) = event_id {
            params.push(("event_id", eid.to_string()));
        }
        if let Some(fid) = family_id {
            params.push(("family_id", fid.to_string()));
        }
        if let Some(sid) = source_id {
            params.push(("source_id", sid.to_string()));
        }
        self.get_with_query(&format!("/api/v1/trees/{tree_id}/notes"), &params)
            .await
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
    pub fn media_file_url(&self, tree_id: Uuid, media_id: Uuid) -> String {
        self.url(&format!("/api/v1/trees/{tree_id}/media/{media_id}/file"))
    }

    /// Where a person's portrait can actually be shown from, if anywhere.
    ///
    /// Not `file_path`. That column is the *producer's* path — the `OBJE.FILE`
    /// a GEDCOM carried, or the address a Geneanet deposit was served under —
    /// kept verbatim so an export round-trips. It is not a URL this
    /// application can load, and putting it in an `<img src>` is what turned
    /// every card with a real photograph into a broken-image icon while the
    /// people with *no* photograph, who fell through to the silhouette,
    /// rendered correctly.
    ///
    /// In order:
    ///
    ///   - a **vignette** — the portrait is a region of a larger image, and
    ///     the server crops it on read, which is the whole point of storing a
    ///     face in a group photograph as coordinates rather than as a copy;
    ///   - a **thumbnail** — we hold the bytes and have rasterised them. A
    ///     pedigree card is 50 pixels wide, so the 400-pixel thumbnail is not
    ///     merely acceptable, it is the right file;
    ///   - a **remote** URL we recorded and never fetched, the only copy there
    ///     is;
    ///   - otherwise **nothing to show**, and `None` lets the caller draw the
    ///     silhouette rather than ask the browser for bytes that will 404.
    pub fn portrait_url(&self, tree_id: Uuid, row: &PortraitRow) -> Option<String> {
        if let Some(vignette_id) = row.vignette_id {
            return Some(self.vignette_image_url(tree_id, vignette_id));
        }
        let media_id = row.media_id?;
        if row.has_thumbnail {
            Some(self.media_thumbnail_url(tree_id, media_id))
        } else if is_remote(&row.file_path) {
            Some(row.file_path.clone())
        } else {
            None
        }
    }

    /// One portrait per person, ready to put in an `<img src>`.
    pub fn portrait_map(&self, tree_id: Uuid, rows: &[PortraitRow]) -> HashMap<Uuid, String> {
        rows.iter()
            .filter_map(|row| Some((row.person_id, self.portrait_url(tree_id, row)?)))
            .collect()
    }

    /// Every person's portrait in a tree, in one request.
    pub async fn list_portraits(&self, tree_id: Uuid) -> Result<Vec<PortraitRow>, ApiError> {
        self.get(&format!("/api/v1/trees/{tree_id}/portraits"))
            .await
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

    /// Absolute URL of a document's pages, packed into one ZIP.
    ///
    /// Only meaningful for a media with pages: a forty-page register is one
    /// document to the reader, and saving it a page at a time is forty save
    /// dialogs and a directory whose alphabetical order has nothing to do
    /// with the document's.
    pub fn media_archive_url(&self, tree_id: Uuid, media_id: Uuid) -> String {
        self.url(&format!("/api/v1/trees/{tree_id}/media/{media_id}/archive"))
    }

    /// Absolute URL of a media's generated thumbnail.
    pub fn media_thumbnail_url(&self, tree_id: Uuid, media_id: Uuid) -> String {
        self.url(&format!(
            "/api/v1/trees/{tree_id}/media/{media_id}/thumbnail"
        ))
    }

    /// Absolute URL of a vignette's cropped image.
    pub fn vignette_image_url(&self, tree_id: Uuid, vignette_id: Uuid) -> String {
        self.url(&format!(
            "/api/v1/trees/{tree_id}/vignettes/{vignette_id}/image"
        ))
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
        tracing::debug!("POST {url} ({} bytes, {file_name})", bytes.len());

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

        let resp = self.client.post(&url).multipart(form).send().await?;
        let media = Self::handle_response(&url, "POST", resp).await?;
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

    /// Detach a page. It survives as an ordinary media.
    pub async fn detach_media_page(
        &self,
        tree_id: Uuid,
        media_id: Uuid,
        page_id: Uuid,
    ) -> Result<Media, ApiError> {
        let page = self
            .delete_json(&format!(
                "/api/v1/trees/{tree_id}/media/{media_id}/pages/{page_id}"
            ))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(page)
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

    /// Soft-delete a media record. The stored bytes stay.
    pub async fn delete_media(&self, tree_id: Uuid, media_id: Uuid) -> Result<(), ApiError> {
        self.delete_no_content(&format!("/api/v1/trees/{tree_id}/media/{media_id}"))
            .await?;
        self.invalidate_tree(tree_id);
        Ok(())
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
        let resp = self.client.post(&url).json(body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(ApiError::Api {
                status,
                body: resp.text().await.unwrap_or_default(),
            });
        }

        Ok(resp.bytes().await?.to_vec())
    }

    /// Read a saved session back, checking it really is one.
    pub async fn decode_geneanet_session(
        &self,
        json: Vec<u8>,
    ) -> Result<GeneanetSession, ApiError> {
        self.post_bytes("/api/v1/geneanet/session/decode", json, &())
            .await
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

    /// How far a running import has got.
    ///
    /// `None` once it has finished — the import's own response is what says it
    /// is done, and a poll racing the end should not read as an error.
    pub async fn geneanet_import_progress(
        &self,
        progress_id: Uuid,
    ) -> Result<Option<ImportProgress>, ApiError> {
        self.get(&format!("/api/v1/geneanet/import/{progress_id}"))
            .await
    }

    /// Import the tree and attach every photo that joins onto it. Step 5.
    pub async fn import_geneanet(
        &self,
        tree_id: Uuid,
        body: &GeneanetImportBody,
    ) -> Result<GeneanetImportResult, ApiError> {
        let result = self
            .post(&format!("/api/v1/trees/{tree_id}/geneanet/import"), body)
            .await?;
        self.invalidate_tree(tree_id);
        Ok(result)
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

    /// Export a tree as a GEDZIP archive (`.gdz`) — a ZIP file wrapping the
    /// same GEDCOM data. Returns the raw archive bytes. See `export_gedcom`
    /// for `merge_occupations` and `merge_names`.
    pub async fn export_gedzip(
        &self,
        tree_id: Uuid,
        merge_occupations: bool,
        merge_names: bool,
    ) -> Result<Vec<u8>, ApiError> {
        let query = [
            ("format", "gedzip".to_string()),
            ("merge_occupations", merge_occupations.to_string()),
            ("merge_names", merge_names.to_string()),
        ];
        self.get_bytes_with_query(&format!("/api/v1/trees/{tree_id}/gedcom/export"), &query)
            .await
    }

    // ── Pedigree Cache ──────────────────────────────────────────────

    /// Helper: send a PATCH request with query parameters (no body).
    async fn patch_with_query<T: serde::de::DeserializeOwned, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> Result<T, ApiError> {
        let url = self.url(path);
        tracing::debug!(
            "PATCH {url} query={}",
            serde_json::to_string(query).unwrap_or_default()
        );
        let resp = self.client.patch(&url).query(query).send().await?;
        Self::handle_response(&url, "PATCH", resp).await
    }

    /// Fetch a windowed pedigree for a root person.
    ///
    /// Assembled server-side from the closure table and the stored person
    /// projections on every call.
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

    /// Occupation-sheet content for a raw GEDCOM occupation label (e.g.
    /// "Laboureur"), localized to `lang` ("fr"/"en"). `None` when no fiche
    /// exists yet for that term — not an error, the caller should just
    /// skip showing a tooltip.
    pub async fn reference_occupation(
        &self,
        lang: &str,
        term: &str,
    ) -> Result<Option<OccupationReference>, ApiError> {
        self.get_with_query_optional(
            &format!("/api/v1/reference/{lang}/occupations"),
            &[("term", term)],
        )
        .await
    }

    /// Given-name meaning content for a raw GEDCOM given name (e.g.
    /// "Marie"), localized to `lang` ("fr"/"en"). `None` when no fiche
    /// exists yet for that name.
    pub async fn reference_given_name(
        &self,
        lang: &str,
        term: &str,
    ) -> Result<Option<GivenNameReference>, ApiError> {
        self.get_with_query_optional(
            &format!("/api/v1/reference/{lang}/given-names"),
            &[("term", term)],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portrait_row(media: Option<Uuid>, path: &str, thumb: bool) -> PortraitRow {
        PortraitRow {
            person_id: Uuid::now_v7(),
            media_id: media,
            vignette_id: None,
            file_path: path.to_string(),
            has_thumbnail: thumb,
        }
    }

    #[test]
    fn a_stored_portrait_is_served_from_our_thumbnail_not_the_producers_path() {
        let api = ApiClient::new("http://localhost:3000");
        let (tree, media) = (Uuid::now_v7(), Uuid::now_v7());
        // The address a Geneanet deposit was recorded under. Loading it
        // directly is what turned every card holding a real photograph into a
        // broken-image icon.
        let row = portrait_row(Some(media), "https://www.geneanet.org/deposit/4713", true);
        assert_eq!(
            api.portrait_url(tree, &row),
            Some(api.media_thumbnail_url(tree, media))
        );
    }

    #[test]
    fn a_portrait_that_is_a_face_in_a_group_photo_is_served_as_the_crop() {
        let api = ApiClient::new("http://localhost:3000");
        let (tree, vignette) = (Uuid::now_v7(), Uuid::now_v7());
        // The portrait most people in an old family archive actually have.
        // Serving the containing scan here would show the whole wedding party
        // on a card meant to show one face.
        let mut row = portrait_row(None, "", false);
        row.vignette_id = Some(vignette);
        assert_eq!(
            api.portrait_url(tree, &row),
            Some(api.vignette_image_url(tree, vignette))
        );
    }

    #[test]
    fn a_crop_wins_over_a_media_if_a_row_somehow_carries_both() {
        let api = ApiClient::new("http://localhost:3000");
        let (tree, vignette) = (Uuid::now_v7(), Uuid::now_v7());
        // The write path refuses both, but a deterministic answer beats an
        // arbitrary one if a row ever escapes it: the crop is the more
        // specific statement.
        let mut row = portrait_row(Some(Uuid::now_v7()), "", true);
        row.vignette_id = Some(vignette);
        assert_eq!(
            api.portrait_url(tree, &row),
            Some(api.vignette_image_url(tree, vignette))
        );
    }

    #[test]
    fn a_remote_media_we_never_fetched_is_shown_from_its_own_url() {
        let api = ApiClient::new("http://localhost:3000");
        let row = portrait_row(Some(Uuid::now_v7()), "https://example.org/photo.jpg", false);
        assert_eq!(
            api.portrait_url(Uuid::now_v7(), &row).as_deref(),
            Some("https://example.org/photo.jpg")
        );
    }

    #[test]
    fn a_record_naming_a_file_nobody_uploaded_has_no_portrait() {
        let api = ApiClient::new("http://localhost:3000");
        // No thumbnail, and a path that is not an address: there is nothing to
        // load. `None` lets the card draw the silhouette rather than ask the
        // browser for bytes that will 404.
        let row = portrait_row(Some(Uuid::now_v7()), "C:\\Photos\\scan.jpg", false);
        assert_eq!(api.portrait_url(Uuid::now_v7(), &row), None);
    }

    #[test]
    fn a_person_with_nothing_loadable_is_left_out_of_the_map() {
        let api = ApiClient::new("http://localhost:3000");
        let (tree, person) = (Uuid::now_v7(), Uuid::now_v7());
        let mut unheld = portrait_row(Some(Uuid::now_v7()), "scan.jpg", false);
        unheld.person_id = person;
        assert!(api.portrait_map(tree, &[unheld]).is_empty());

        let mut held = portrait_row(Some(Uuid::now_v7()), "", true);
        held.person_id = person;
        assert!(api.portrait_map(tree, &[held]).contains_key(&person));
    }
}
