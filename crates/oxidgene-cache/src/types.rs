//! Cache type definitions.
//!
//! All types are serializable for storage in Redis (MessagePack) or on disk (bincode).

use chrono::{DateTime, NaiveDate, Utc};
use oxidgene_core::enums::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── PersonCache ────────────────────────────────────────────────────────────

/// A fully denormalized person profile, containing everything needed
/// to display a person card, detail page, or edit modal in a single read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPerson {
    // Core identity
    pub person_id: Uuid,
    pub tree_id: Uuid,
    pub sex: Sex,

    // Names (denormalized from PersonName)
    pub primary_name: Option<CachedName>,
    pub other_names: Vec<CachedName>,

    // Key life events (denormalized from Event + Place)
    pub birth: Option<CachedEvent>,
    pub death: Option<CachedEvent>,
    pub baptism: Option<CachedEvent>,
    pub burial: Option<CachedEvent>,
    pub occupation: Option<String>,
    pub other_events: Vec<CachedEvent>,

    // Family links
    pub families_as_spouse: Vec<CachedFamilyLink>,
    pub family_as_child: Option<CachedChildLink>,

    // Attached media / sources / notes (counts + primary)
    pub primary_media: Option<CachedMediaRef>,
    pub media_count: u32,
    pub citation_count: u32,
    pub note_count: u32,

    // Metadata
    pub updated_at: DateTime<Utc>,
    pub cached_at: DateTime<Utc>,
}

/// A person name, pre-computed for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedName {
    pub name_id: Uuid,
    pub name_type: NameType,
    pub display_name: String,
    pub given_names: Option<String>,
    pub surname: Option<String>,
}

/// An event with its place name denormalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedEvent {
    pub event_id: Uuid,
    pub event_type: EventType,
    pub date_value: Option<String>,
    pub date_sort: Option<NaiveDate>,
    pub place_name: Option<String>,
    pub place_id: Option<Uuid>,
    pub description: Option<String>,
}

/// A family in which this person is a spouse, with the other spouse's info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFamilyLink {
    pub family_id: Uuid,
    pub role: SpouseRole,
    pub spouse_id: Option<Uuid>,
    pub spouse_display_name: Option<String>,
    pub spouse_sex: Option<Sex>,
    pub marriage: Option<CachedEvent>,
    pub children_ids: Vec<Uuid>,
    pub children_count: u32,
}

/// The family in which this person is a child, with parent info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedChildLink {
    pub family_id: Uuid,
    pub child_type: ChildType,
    pub father_id: Option<Uuid>,
    pub father_display_name: Option<String>,
    pub mother_id: Option<Uuid>,
    pub mother_display_name: Option<String>,
}

/// A reference to a media item (portrait / primary photo).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMediaRef {
    pub media_id: Uuid,
    pub file_path: String,
    pub mime_type: String,
    pub title: Option<String>,
}

// ─── PedigreeCache ──────────────────────────────────────────────────────────

/// A windowed pedigree view for a given root person, containing only
/// the persons and edges visible at the loaded depth levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPedigree {
    pub tree_id: Uuid,
    pub root_person_id: Uuid,
    pub persons: HashMap<Uuid, PedigreeNode>,
    pub edges: Vec<PedigreeEdge>,
    pub ancestor_depth_loaded: u32,
    pub descendant_depth_loaded: u32,
    pub cached_at: DateTime<Utc>,
}

/// A person node in the pedigree chart, optimized for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PedigreeNode {
    pub person_id: Uuid,
    pub sex: Sex,
    pub display_name: String,
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

// ─── SearchIndex ────────────────────────────────────────────────────────────

/// A pre-built search index for a tree, enabling instant server-side search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSearchIndex {
    pub tree_id: Uuid,
    pub entries: Vec<SearchEntry>,
    pub cached_at: DateTime<Utc>,
}

/// A single entry in the search index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEntry {
    pub person_id: Uuid,
    pub sex: Sex,
    // Searchable fields (lowercased, accent-folded)
    pub surname_normalized: String,
    pub given_names_normalized: String,
    pub maiden_name_normalized: Option<String>,
    // Display fields (original casing)
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
