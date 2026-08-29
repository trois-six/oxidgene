//! GEDCOM and GeneWeb import/export for OxidGene.
//!
//! Wraps the [`ged_io`] crate to convert between GEDCOM files and OxidGene
//! domain model types, and the [`geneweb`] crate to read GeneWeb `.gw` files —
//! which it converts to the same `ged_io` model, so both formats share one
//! mapping into the domain model.

pub mod date;
pub mod export;
pub mod geneweb;
pub mod import;

use serde::{Deserialize, Serialize};

use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Source, Vignette,
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
    pub event_witnesses: Vec<EventWitness>,
    pub places: Vec<Place>,
    pub sources: Vec<Source>,
    pub citations: Vec<Citation>,
    pub media: Vec<Media>,
    pub media_links: Vec<MediaLink>,
    pub vignettes: Vec<Vignette>,
    pub notes: Vec<Note>,
    /// Warnings collected during import (non-fatal issues).
    pub warnings: Vec<String>,
    /// The `@I…@` xref each imported person was given a UUID for.
    ///
    /// Exposed because a caller can hold links keyed by something the domain
    /// model has no room for, and needs to turn those into person ids after
    /// the fact. The Geneanet import is the one that does: its person↔photo
    /// mapping is keyed by GeneWeb reference, joined onto the `.gw` by
    /// position, and `GwDatabase::persons[i]` becomes the individual with xref
    /// `@I{i+1}@`.
    pub person_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The `@M…@` xref each imported media record was given a UUID for.
    ///
    /// OxidGene's vignette extension uses the record xref to attach each crop
    /// to its source image after the standard GEDCOM model has been imported.
    pub media_by_xref: std::collections::HashMap<String, uuid::Uuid>,
}

/// The result of exporting domain model entities to a GEDCOM string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    /// The GEDCOM 5.5.1 formatted string.
    pub gedcom: String,
    /// Warnings collected during export.
    pub warnings: Vec<String>,
}
