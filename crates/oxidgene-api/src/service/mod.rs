//! Service layer: shared business logic used by both REST and GraphQL handlers.

pub mod background_job;
pub mod duplicates;
pub mod event_date;
pub mod family_names;
pub mod gallery;
pub mod gedcom;
pub mod geneanet;
pub mod geneweb;
pub mod history;
pub mod image_bytes;
pub mod kinship;
pub mod media;
pub mod pedigrees;
pub mod person_detail;
pub mod portrait;
pub mod purge;
pub mod relation_labels;
pub(crate) mod session_media;
pub mod statistics;
pub mod suggestions;
