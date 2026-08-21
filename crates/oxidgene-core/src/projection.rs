//! Denormalized read projections.
//!
//! These are the read-side shapes assembled from the normalized entities:
//! a full person profile, a windowed pedigree, and search results. They are
//! the wire format of the `/api/v1/trees/{id}/profiles` and `/pedigree`
//! endpoints, so both the backend (which builds and persists them) and the
//! frontend (which renders them) depend on this module.
//!
//! The person profile is materialized into the `person_denorm` table on every
//! mutation — see `docs/specifications/read-projections.md`.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::enums::{Calendar, ChildType, DateQualifier, NameType, Sex, SpouseRole};

/// The shape of a stored [`PersonProfile`] payload.
///
/// Stored beside the row in `person_denorm.schema_version`, and compared on
/// every read: a row written by an older build is treated as *absent*, so the
/// paths that already rebuild a missing projection rebuild a stale one too.
///
/// **Raise this whenever a change alters what a payload means.** Adding a field
/// is the usual case, and it is exactly the one that needs it: new fields carry
/// `#[serde(default)]` so old rows keep deserializing, which means they come
/// back looking complete rather than empty. Without a bump the feature is
/// simply invisible on every existing install — that is what happened when
/// `date_qualifier` arrived and every card went on drawing bare years.
///
/// A bump costs one lazy rebuild per tree on first read. Not bumping costs a
/// silent wrong answer, so when in doubt, bump.
pub const PROJECTION_SCHEMA_VERSION: i32 = 1;

// ─── Person profile ─────────────────────────────────────────────────────────

/// A fully denormalized person profile, containing everything needed
/// to display a person card, detail page, or edit modal in a single read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonProfile {
    // Core identity
    pub person_id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,

    // Names (denormalized from PersonName)
    pub primary_name: Option<ProfileName>,
    pub other_names: Vec<ProfileName>,

    // Key life events (denormalized from Event + Place)
    pub birth: Option<ProfileEvent>,
    pub death: Option<ProfileEvent>,
    pub baptism: Option<ProfileEvent>,
    pub burial: Option<ProfileEvent>,
    pub occupation: Option<String>,
    pub other_events: Vec<ProfileEvent>,

    // Family links
    pub families_as_spouse: Vec<ProfileFamilyLink>,
    pub family_as_child: Option<ProfileChildLink>,

    // Attached media / sources / notes (counts + primary)
    pub primary_media: Option<ProfileMediaRef>,
    pub media_count: u32,
    pub citation_count: u32,
    pub note_count: u32,

    // Metadata
    pub updated_at: DateTime<Utc>,
    /// When this projection was last rebuilt from the normalized tables.
    pub built_at: DateTime<Utc>,
}

impl PersonProfile {
    /// The event a card should date this person's life *from*: the birth, or
    /// the baptism when no birth was recorded.
    ///
    /// Mirrors GeneWeb's `Gutil.get_birth_death_date`. A parish register very
    /// often holds a baptism and no birth, and "1620" from the baptism is far
    /// more use on a card than an empty line — the baptism's own qualifier
    /// still says how firm it is.
    ///
    /// Note what this deliberately does *not* copy from GeneWeb: there, the
    /// fallback sets a single `approx` flag covering *both* ends of the life,
    /// so a person whose birth came from a baptism gets "ca" stamped on their
    /// death year too — which is how a death recorded as "between 1691 and
    /// 1693" ends up displayed as "ca 1691". Each event keeps its own
    /// precision here.
    /// The fallback triggers on a missing **date**, not a missing event. A
    /// birth recorded with no date at all is extremely common — the register
    /// entry is the baptism, and the birth is an empty stub someone created to
    /// hang a source on. Testing `self.birth.is_none()` would keep that stub
    /// and draw a blank year while a perfectly good "vers 1620" sat unused on
    /// the baptism. GeneWeb tests the date for the same reason
    /// (`Date.od_of_cdate (get_birth p)`).
    pub fn birth_or_baptism(&self) -> Option<&ProfileEvent> {
        pick_dated(self.birth.as_ref(), self.baptism.as_ref())
    }

    /// The event a card should date this person's life *to*: the death, or the
    /// burial when the death carries no date. See [`Self::birth_or_baptism`].
    pub fn death_or_burial(&self) -> Option<&ProfileEvent> {
        pick_dated(self.death.as_ref(), self.burial.as_ref())
    }
}

/// `preferred` when it carries a date, otherwise `fallback` when *it* does,
/// otherwise whichever exists — so a dateless stub still identifies the event
/// for anything that only needs to know it happened.
fn pick_dated<'a>(
    preferred: Option<&'a ProfileEvent>,
    fallback: Option<&'a ProfileEvent>,
) -> Option<&'a ProfileEvent> {
    let dated = |e: &&ProfileEvent| e.date_value.is_some() || e.date_sort.is_some();
    preferred
        .filter(dated)
        .or_else(|| fallback.filter(dated))
        .or(preferred)
        .or(fallback)
}

/// A person name, pre-computed for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileName {
    pub name_id: Uuid,
    pub name_type: NameType,
    pub display_name: String,
    pub given_names: Option<String>,
    pub surname: Option<String>,
}

/// An event with its place name denormalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileEvent {
    pub event_id: Uuid,
    pub event_type: crate::enums::EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<NaiveDate>,
    /// How precise the date is. `serde(default)` because payloads written
    /// before this field existed are still in `person_denorm`, and an absent
    /// qualifier means the same thing as `Exact` did back then.
    #[serde(default)]
    pub date_qualifier: DateQualifier,
    /// The far end of an `Or`/`Between` range. Without it a "between 11 Nov
    /// 1691 and 20 Aug 1693" reads as a bare "between 1691" — the qualifier
    /// promises a second date the projection could not carry.
    #[serde(default)]
    pub date_value2: Option<String>,
    /// Calendar the date was recorded in, so a Republican or Hebrew date is
    /// not silently re-read as Gregorian.
    #[serde(default)]
    pub calendar: Calendar,
    pub place_name: Option<String>,
    pub place_id: Option<Uuid>,
    pub description: Option<String>,
}

/// A family in which this person is a spouse, with the other spouse's info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFamilyLink {
    pub family_id: Uuid,
    pub role: SpouseRole,
    pub spouse_id: Option<Uuid>,
    pub spouse_display_name: Option<String>,
    pub spouse_sex: Option<Sex>,
    pub marriage: Option<ProfileEvent>,
    /// All family events (marriage, divorce, annulment, etc.)
    #[serde(default)]
    pub events: Vec<ProfileEvent>,
    pub children_ids: Vec<Uuid>,
    pub children_count: u32,
}

/// The family in which this person is a child, with parent info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileChildLink {
    pub family_id: Uuid,
    pub child_type: ChildType,
    pub father_id: Option<Uuid>,
    pub father_display_name: Option<String>,
    pub mother_id: Option<Uuid>,
    pub mother_display_name: Option<String>,
}

/// A reference to a media item (portrait / primary photo).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMediaRef {
    pub media_id: Uuid,
    /// Set when the portrait is a region of that media rather than the whole
    /// of it — a face in a group photograph. The cropped image is served from
    /// the vignette, not the media.
    #[serde(default)]
    pub vignette_id: Option<Uuid>,
    pub file_path: String,
    pub mime_type: String,
    pub title: Option<String>,
}

// ─── Pedigree ───────────────────────────────────────────────────────────────

/// A windowed pedigree view for a given root person, containing only
/// the persons and edges visible at the requested depth levels.
///
/// Built on demand by walking the family links and joining the reached persons
/// against the `person_denorm` projections — it is never stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pedigree {
    pub tree_id: Uuid,
    pub root_person_id: Uuid,
    pub persons: HashMap<Uuid, PedigreeNode>,
    pub edges: Vec<PedigreeEdge>,
    /// Family events keyed by family_id (marriage, divorce, annulment, etc.)
    #[serde(default)]
    pub family_events: HashMap<Uuid, Vec<ProfileEvent>>,
    /// Family units with spouse and child membership (covers childless couples
    /// that produce no PedigreeEdge entries).
    #[serde(default)]
    pub families: HashMap<Uuid, PedigreeFamily>,
    pub ancestor_depth_loaded: u32,
    pub descendant_depth_loaded: u32,
    pub built_at: DateTime<Utc>,
}

/// A person node in the pedigree chart, optimized for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeNode {
    pub person_id: Uuid,
    pub sex: Sex,
    pub display_name: String,
    #[serde(default)]
    pub given_names: Option<String>,
    #[serde(default)]
    pub surname: Option<String>,
    /// The whole birth event, not a year pulled out of it.
    ///
    /// It used to be a `birth_year` string plus a `birth_place` string, and
    /// every fact that did not fit those two — the day and month, the far end
    /// of a range, the calendar, the place's id — was gone before the frontend
    /// saw it. That is why a death recorded as "between 11 Nov 1691 and 20 Aug
    /// 1693" reached the events panel as "between 1691". Carrying the event
    /// itself costs a few fields per node and cannot lose anything.
    ///
    /// Falls back to baptism when there is no birth, the way GeneWeb's
    /// `get_birth_death_date` does — see [`birth_or_baptism`].
    #[serde(default)]
    pub birth: Option<ProfileEvent>,
    /// The whole death event, falling back to burial. See [`Self::birth`].
    #[serde(default)]
    pub death: Option<ProfileEvent>,
    pub occupation: Option<String>,
    pub primary_media_path: Option<String>,
    pub generation: i32,
    pub sosa_number: Option<u64>,
}

/// A parent-child edge in the pedigree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeEdge {
    pub parent_id: Uuid,
    pub child_id: Uuid,
    pub family_id: Uuid,
    pub edge_type: ChildType,
}

/// A family unit in the pedigree, capturing spouse and child relationships
/// independently of parent→child edges (which miss childless couples).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeFamily {
    pub family_id: Uuid,
    pub spouse_ids: Vec<Uuid>,
    pub children_ids: Vec<Uuid>,
    /// Minimal info for family members (children, spouses) who may be outside
    /// the pedigree window — enough to build synthetic events in the UI.
    #[serde(default)]
    pub members: Vec<PedigreeFamilyMember>,
}

/// Minimal person info for a family member in the pedigree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeFamilyMember {
    pub person_id: Uuid,
    pub display_name: String,
    #[serde(default)]
    pub given_names: Option<String>,
    #[serde(default)]
    pub surname: Option<String>,
    pub sex: Sex,
    /// Whole events, for the same reason as [`PedigreeNode::birth`].
    #[serde(default)]
    pub birth: Option<ProfileEvent>,
    #[serde(default)]
    pub death: Option<ProfileEvent>,
}

/// The result of an incremental pedigree expansion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeDelta {
    pub new_nodes: Vec<PedigreeNode>,
    pub new_edges: Vec<PedigreeEdge>,
    pub ancestor_depth_loaded: u32,
    pub descendant_depth_loaded: u32,
}

/// Direction for pedigree expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PedigreeDirection {
    Ancestors,
    Descendants,
}

// ─── Search ─────────────────────────────────────────────────────────────────
//
// Since Sprint E.6 the search index lives in the database itself (SQLite FTS5
// virtual table / plain PostgreSQL table `person_search_fts`). These types
// remain as the API wire shape for search results.

/// A single search result entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEntry {
    pub person_id: Uuid,
    pub sex: Sex,
    // Searchable fields (lowercased, accent-folded)
    pub surname_normalized: String,
    pub given_names_normalized: String,
    pub maiden_name_normalized: Option<String>,
    // Display fields (original casing) — always populated, so callers never
    // need to guess the surname/given-name split by parsing `display_name`.
    pub surname: String,
    pub given_names: String,
    pub display_name: String,
    // Key dates for result display
    pub birth_year: Option<String>,
    pub birth_place: Option<String>,
    pub death_year: Option<String>,
    // For sorting / filtering
    pub date_sort: Option<NaiveDate>,
}

/// Paginated search results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entries: Vec<SearchEntry>,
    pub total_count: usize,
}
