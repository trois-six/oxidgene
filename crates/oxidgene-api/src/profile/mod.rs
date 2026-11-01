//! Denormalized person projections.
//!
//! This module replaced the `oxidgene-cache` crate: instead of caching
//! assembled read models in Redis or in-process, it materializes them into
//! the `person_denorm` table on every mutation, and assembles pedigrees on
//! demand by walking the family links and joining against those rows.
//!
//! - [`builder`] — assembles a projection from raw entities
//! - [`invalidation`] — computes which projections a mutation affects
//! - `pedigree` — the database-free steps of a pedigree window's assembly
//! - [`service`] — orchestrates reads, rebuilds and pedigree assembly
//!
//! See `docs/data-model.md` for the read-model architecture.

pub mod builder;
pub mod invalidation;
mod pedigree;
mod redacted;
pub mod service;
mod sosa;
#[cfg(feature = "auth")]
mod visibility;

pub use redacted::{person_record, redact_tree, require_reachable, sosa_shown};
pub use service::ProfileService;
