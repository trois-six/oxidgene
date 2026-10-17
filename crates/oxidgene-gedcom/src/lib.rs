//! GEDCOM and GeneWeb import/export for OxidGene.
//!
//! Wraps the [`oxidged`] crate to convert between GEDCOM files and OxidGene
//! domain model types, and the [`geneweb`] crate to read GeneWeb `.gw` files —
//! which it converts to the same `oxidged` model, so both formats share one
//! mapping into the domain model.

pub mod date;
pub mod export;
pub mod geneweb;
pub mod import;
pub mod kept;

use serde::{Deserialize, Serialize};

use oxidgene_core::types::{
    Citation, Event, EventWitness, Family, FamilyChild, FamilySpouse, Media, MediaLink, Note,
    Person, PersonName, Place, Repository, Source, SourceRepository, Vignette,
};
use oxidgene_core::{Calendar, DateQualifier, DocumentCategory, Privacy, SourceMediaType};

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
    /// The `REPO` records, and the repositories pointerless `SOUR.REPO`
    /// citations name.
    pub repositories: Vec<Repository>,
    /// Which repositories hold each source, with the call numbers.
    pub source_repositories: Vec<SourceRepository>,
    pub citations: Vec<Citation>,
    pub media: Vec<Media>,
    pub media_links: Vec<MediaLink>,
    pub vignettes: Vec<Vignette>,
    pub notes: Vec<Note>,
    /// The tree the records were read for.
    pub tree_id: uuid::Uuid,
    /// The submitter the header points at (`HEAD.SUBM`), for the tree's
    /// settings to take where they are empty.
    pub submitter: Option<ImportedSubmitter>,
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
    /// The `@F…@` xref each imported family was given a UUID for.
    pub family_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The `@S…@` xref each imported source was given a UUID for.
    pub source_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The `@R…@` xref each imported repository was given a UUID for.
    pub repository_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The `@N…@` xref of each shared note record, and the group UUID that
    /// the notes read from it carry as `Note::shared_group`.
    pub note_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The `@M…@` xref each imported media record was given a UUID for.
    ///
    /// OxidGene's vignette extension uses the record xref to attach each crop
    /// to its source image after the standard GEDCOM model has been imported.
    pub media_by_xref: std::collections::HashMap<String, uuid::Uuid>,
    /// The GEDCOM structures the model has no place for, per entity (see
    /// `kept`).
    pub kept: Vec<oxidgene_core::types::GedcomKept>,
}

/// The submitter a file says it is from: each field when the file states it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedSubmitter {
    pub name: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
}

impl ImportResult {
    /// Every record the import produced, as an export takes them.
    pub fn records(&self) -> export::ExportRecords<'_> {
        export::ExportRecords {
            persons: &self.persons,
            person_names: &self.person_names,
            families: &self.families,
            family_spouses: &self.family_spouses,
            family_children: &self.family_children,
            events: &self.events,
            event_witnesses: &self.event_witnesses,
            places: &self.places,
            sources: &self.sources,
            repositories: &self.repositories,
            source_repositories: &self.source_repositories,
            citations: &self.citations,
            media: &self.media,
            media_links: &self.media_links,
            vignettes: &self.vignettes,
            notes: &self.notes,
        }
    }
}

/// The result of exporting domain model entities to a GEDCOM string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    /// The GEDCOM text: a `.ged` as its destination reads it (5.5.1 unless
    /// the destination reads 7.0), 7.0 after a byte-order mark for the
    /// `gedcom.ged` of a GEDZIP (see [`export::ExportTarget`]).
    pub gedcom: String,
    /// Warnings collected during export.
    pub warnings: Vec<String>,
    /// What writing a `.ged` for its destination lost; `None` for a GEDZIP,
    /// which no destination shapes.
    pub report: Option<DestinationReport>,
    /// Every record the file holds, by its xref text (`"@I1@"`, `"@F1@"`,
    /// `"@S1@"`, `"@R1@"`, `"@M1@"`, `"@N1@"`, and the submitter's), mapped to
    /// the entity id it was written for: the submitter to `Uuid::nil()`, a
    /// shared note record to its `Note::shared_group`.
    pub xrefs: std::collections::HashMap<String, uuid::Uuid>,
}

/// What writing a `.ged` for its destination lost: the folds of the
/// destination's export profile that lost something, and the limits of the
/// destination program the file exceeds. Empty for the standard
/// destinations, which fold nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationReport {
    /// The destination's export profile id (`generic-551`, `geneanet`, …).
    pub destination: String,
    /// The folds that lost something, in the order made.
    pub losses: Vec<ExportLoss>,
    /// The destination's limits the file exceeds.
    pub limits: Vec<ExportLimit>,
}

/// A fold that lost something: several occupations made one, a structure
/// written as a note, a link left out, ….
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportLoss {
    /// What kind of change: `merged`, `moved`, `relabelled`, `added`,
    /// `as_text`, `omitted`, `reordered`, `recoded`, or `other`.
    pub kind: String,
    /// The cross-reference of the record it was in (`@I12@`), if one.
    pub record: Option<String>,
    /// What was changed, in `oxidged`'s words (English).
    pub detail: String,
}

/// A limit of the destination program the file exceeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportLimit {
    /// The cross-reference of the record concerned, if one.
    pub record: Option<String>,
    /// The limit and what exceeds it, in `oxidged`'s words (English).
    pub detail: String,
}

/// Page-level metadata GEDCOM 5.5.1 cannot represent on an `OBJE` record.
///
/// Strictly about the page: what the file was called, when the row was made,
/// and the transcript of this page. Everything that describes the document as
/// a whole rides in [`DocumentExtension`] instead — writing it here too would
/// duplicate one title across thirty-eight scans and leave no way to tell,
/// on re-import, which copy was the original.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MediaMetadataExtension {
    pub version: u8,
    pub file_name: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// This page's transcript, if it has one.
    pub notes: Vec<MediaNoteExtension>,
}

/// The document a page belongs to, and — on its first page — what that
/// document says about itself.
///
/// GEDCOM has no container, so a document is exported dissolved: its pages
/// become ordinary `OBJE` records that any other software reads as a set of
/// files. This line is what puts them back together on the way in. `doc` is a
/// token unique within the file, not an identifier anything outside it knows.
///
/// `meta` travels on `index == 0` alone. Repeating it on every page would grow
/// a forty-page dossier by forty copies of the same JSON, and give the importer
/// forty candidates to reconcile when it should have one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DocumentExtension {
    pub version: u8,
    /// Token grouping the pages of one document within this file.
    pub doc: String,
    /// Zero-based position of this page in the document.
    pub index: i32,
    /// How many pages the document had when it was written.
    pub count: i32,
    pub meta: Option<DocumentMetadataExtension>,
}

/// What a document says about itself: the fields its pages share.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DocumentMetadataExtension {
    pub file_name: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub date_value: Option<String>,
    pub date_qualifier: DateQualifier,
    pub date_value2: Option<String>,
    pub calendar: Calendar,
    pub privacy: Privacy,
    pub source_media_type: SourceMediaType,
    pub document_category: Option<DocumentCategory>,
    pub tags: Vec<String>,
    pub place: Option<MediaPlaceExtension>,
    /// Notes about the document as a whole, as distinct from a page transcript.
    pub notes: Vec<MediaNoteExtension>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MediaPlaceExtension {
    pub name: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MediaNoteExtension {
    pub text: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}
