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

use crate::enums::{ChildType, NameType, Sex, SpouseRole};

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
    pub birth_year: Option<String>,
    pub birth_place: Option<String>,
    pub death_year: Option<String>,
    pub death_place: Option<String>,
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
    pub birth_year: Option<String>,
    pub death_year: Option<String>,
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
