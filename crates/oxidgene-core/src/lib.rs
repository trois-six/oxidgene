//! OxidGene core domain types, enums, and shared error types.
//!
//! This crate contains the foundational types used across all other OxidGene crates.
//! It has no internal dependencies on other workspace crates.

pub mod access;
pub mod calendar;
pub mod collections;
pub mod enums;
pub mod error;
pub mod history;
pub mod notification;
pub mod privacy;
pub mod projection;
pub mod redaction;
pub mod resources;
pub mod search;
pub mod sharing;
pub mod types;

pub use enums::*;
pub use error::OxidGeneError;
