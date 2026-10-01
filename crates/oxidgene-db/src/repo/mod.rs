//! Repository layer: CRUD operations, pagination, and database utilities.
//!
//! This module provides:
//! - Database connection and migration helpers (`connect`, `run_migrations`)
//! - A generic cursor-based pagination helper
//! - Repository implementations for all entities

mod ancestry;
mod background_job;
pub(crate) mod batch;
mod citation;
mod connection;
mod dictionary;
mod event;
mod event_witness;
mod family;
mod family_child;
mod family_spouse;
mod history;
mod media;
mod media_library;
mod media_link;
mod media_tag;
mod note;
mod pagination;
mod person;

mod person_denorm;
mod person_distinct;
mod person_merge;
mod person_name;
mod person_search;
mod place;
mod snapshot;
mod source;
mod tree;
mod vignette;

pub use ancestry::{AncestryRepo, FamilyLink};
pub use background_job::{
    BackgroundJob, BackgroundJobKind, BackgroundJobRepo, BackgroundJobStatus, NewBackgroundJob,
};
pub use citation::{CitationFilter, CitationRepo};
pub use connection::{
    connect, erase_deleted_content, refresh_statistics, rollback_migrations, run_migrations,
};
pub use dictionary::{
    DictionaryRepo, DictionaryValueEntry, FamilyNameParticleUpdate, FamilyNameRename,
    PersonUsageEntry, SOURCE_DRILL_THRESHOLD,
};
pub use event::{EventFilter, EventRepo};
pub use event_witness::EventWitnessRepo;
pub use family::FamilyRepo;
pub use family_child::FamilyChildRepo;
pub use family_spouse::FamilySpouseRepo;
pub use history::{AuditFilter, HistoryRepo, LatestVersion, NewRecordVersion};
pub use media::{MediaPatch, MediaRepo, UploadedMedia, UploadedMediaMetadata};
pub use media_library::{MediaFacets, MediaFilter, MediaLibraryRepo, MediaTagCount};
pub use media_link::{MediaLinkRepo, MediaLinkRow, MediaLinkTarget};
pub use media_tag::MediaTagRepo;
pub use note::{NoteFilter, NoteRepo};
pub use pagination::PaginationParams;
pub use person::{PersonRepo, PortraitRow};

pub use person_denorm::PersonDenormRepo;
pub use person_distinct::PersonDistinctRepo;
pub use person_merge::PersonMergeRepo;
pub use person_name::{PersonNamePieces, PersonNamePiecesPatch, PersonNameRepo};
pub use person_search::{
    PersonSearchEntry, PersonSearchFilters, PersonSearchPage, PersonSearchRepo, PersonSearchSort,
    RELATIVE_SEP,
};
pub use place::PlaceRepo;
pub use snapshot::{BuiltSnapshot, SnapshotRepo, SnapshotScope, display_names};
pub use source::SourceRepo;
pub use tree::{TreeChanges, TreeRepo};
pub use vignette::{VignetteInput, VignettePatch, VignetteRepo};

/// A database error as the domain reports it.
///
/// Public so that every layer maps a `DbErr` the same way: the API's services
/// open and commit transactions and run a few queries of their own.
pub fn db_err(error: sea_orm::DbErr) -> oxidgene_core::error::OxidGeneError {
    oxidgene_core::error::OxidGeneError::Database(error.to_string())
}
