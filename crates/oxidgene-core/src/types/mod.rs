//! Domain types for OxidGene.
//!
//! These are pure domain models, independent of any database or API framework.
//! They represent the canonical shapes of genealogical data within the application.

mod citation;
mod event;
mod family;
mod media;
mod note;
mod pagination;
mod person;
mod place;
mod source;
mod tree;

pub use citation::Citation;
pub use event::Event;
pub use family::{Family, FamilyChild, FamilySpouse};
pub use media::{Media, MediaLink};
pub use note::Note;
pub use pagination::{Connection, Edge, PageInfo};
pub use person::{Person, PersonAncestry, PersonName};
pub use place::Place;
pub use source::Source;
pub use tree::Tree;
