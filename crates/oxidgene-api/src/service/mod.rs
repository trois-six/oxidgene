//! Service layer: shared business logic used by both REST and GraphQL handlers.

pub mod background_job;
pub mod event_date;
pub mod gallery;
pub mod gedcom;
pub mod geneanet;
pub mod geneweb;
pub mod media;
pub mod person_detail;
pub mod portrait;
pub mod purge;
pub mod relation_labels;
pub(crate) mod session_media;
