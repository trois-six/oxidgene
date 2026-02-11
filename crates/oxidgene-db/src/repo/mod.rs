//! Repository layer: CRUD operations, pagination, and database utilities.
//!
//! This module provides:
//! - Database connection and migration helpers (`connect`, `run_migrations`)
//! - A generic cursor-based pagination helper
//! - Repository implementations for all entities

mod citation;
mod connection;
mod event;
mod family;
mod family_child;
mod family_spouse;
mod media;
mod media_link;
mod note;
mod pagination;
mod person;
mod person_ancestry;
mod person_name;
mod place;
mod source;
mod tree;

pub use citation::CitationRepo;
pub use connection::{connect, rollback_migrations, run_migrations};
pub use event::{EventFilter, EventRepo};
pub use family::FamilyRepo;
pub use family_child::FamilyChildRepo;
pub use family_spouse::FamilySpouseRepo;
pub use media::MediaRepo;
pub use media_link::MediaLinkRepo;
pub use note::NoteRepo;
pub use pagination::PaginationParams;
pub use person::PersonRepo;
pub use person_ancestry::PersonAncestryRepo;
pub use person_name::PersonNameRepo;
pub use place::PlaceRepo;
pub use source::SourceRepo;
pub use tree::TreeRepo;
