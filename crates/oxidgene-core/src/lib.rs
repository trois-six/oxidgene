//! OxidGene core domain types, enums, and shared error types.
//!
//! This crate contains the foundational types used across all other OxidGene crates.
//! It has no internal dependencies on other workspace crates.

pub mod enums;
pub mod error;
pub mod types;

pub use enums::*;
pub use error::OxidGeneError;
