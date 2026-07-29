//! Denormalized person projections.
//!
//! This module replaced the `oxidgene-cache` crate: instead of caching
//! assembled read models in Redis or in-process, it materializes them into
//! the `person_denorm` table on every mutation, and assembles pedigrees on
//! demand from the `person_ancestry` closure table joined against those rows.
//!
//! - [`builder`] — assembles a projection from raw entities
//! - [`invalidation`] — computes which projections a mutation affects
//! - [`service`] — orchestrates reads, rebuilds and pedigree assembly
//!
//! See `docs/specifications/read-projections.md` for the architecture and the
//! rationale for dropping the cache layer.

pub mod builder;
pub mod invalidation;
pub mod service;

pub use service::ProfileService;
