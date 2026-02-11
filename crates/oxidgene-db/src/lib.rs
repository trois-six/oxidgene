//! OxidGene database layer: entities, migrations, and repository.
//!
//! This crate provides:
//! - SeaORM entity models for all 14 database tables
//! - Database migrations via `sea_orm_migration`
//! - A `Migrator` type to run schema changes
//! - (Future) Repository trait implementations for CRUD operations

pub mod entities;
pub mod migration;
pub mod repo;

pub use migration::Migrator;

/// Convenience re-export of `sea_orm` for downstream crates.
pub use sea_orm;
