//! GEDCOM import/export for OxidGene.
//!
//! Wraps the [`ged_io`] crate to convert between GEDCOM files and OxidGene
//! domain model types.

pub mod export;
pub mod import;

use serde::{Deserialize, Serialize};

use oxidgene_core::types::{
    Citation, Event, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note, Person,
    PersonAncestry, PersonName, Place, Source,
};

/// The result of importing a GEDCOM file — all domain model entities extracted
/// from the file, ready to be persisted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportResult {
    pub persons: Vec<Person>,
    pub person_names: Vec<PersonName>,
    pub families: Vec<Family>,
    pub family_spouses: Vec<FamilySpouse>,
    pub family_children: Vec<FamilyChild>,
    pub events: Vec<Event>,
    pub places: Vec<Place>,
    pub sources: Vec<Source>,
    pub citations: Vec<Citation>,
    pub media: Vec<Media>,
    pub media_links: Vec<MediaLink>,
    pub notes: Vec<Note>,
    pub person_ancestry: Vec<PersonAncestry>,
    /// Warnings collected during import (non-fatal issues).
    pub warnings: Vec<String>,
}

/// The result of exporting domain model entities to a GEDCOM string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    /// The GEDCOM 5.5.1 formatted string.
    pub gedcom: String,
    /// Warnings collected during export.
    pub warnings: Vec<String>,
}
