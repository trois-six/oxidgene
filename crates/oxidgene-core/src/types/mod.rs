//! Domain types for OxidGene.
//!
//! These are pure domain models, independent of any database or API framework.
//! They represent the canonical shapes of genealogical data within the application.

pub mod age;
mod citation;
mod event;
mod family;
mod kinship;
mod media;
mod note;
mod pagination;
mod person;
mod place;
mod repository;
mod source;
mod surname;
mod tree;

pub use age::AgeAtEvent;
pub use citation::Citation;
pub use event::{Event, EventWitness, QualifiedYear, SpouseAge, year_from_date};
pub use family::{Family, FamilyChild, FamilySpouse};
pub use kinship::{Kinship, KinshipPath, KinshipSegment};
pub use media::{
    DOCUMENT_MIME, ImageCrop, ImageSource, Media, MediaLink, Portrait, PortraitRef, Vignette,
    guess_mime, is_image_mime, is_remote_url, last_path_segment, may_draw_as_image, normalize_mime,
};
pub use note::Note;
pub use pagination::{Connection, Edge, PageInfo};
pub use person::{AncestryLink, Person, PersonName};
pub use place::Place;
pub use repository::{Repository, SourceRepository};
pub use source::Source;
pub use surname::{
    join_surname_particle, split_surname_at_head, split_surname_particle, split_surname_with,
    surname_sort_key,
};
pub use tree::Tree;
pub(crate) use tree::enabled;
