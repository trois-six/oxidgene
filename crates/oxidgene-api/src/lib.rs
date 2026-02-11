//! OxidGene API layer: REST and GraphQL endpoints.
//!
//! This crate provides:
//! - REST handlers for all CRUD endpoints under `/api/v1`
//! - A router builder to wire up all routes
//! - (Future) GraphQL schema and resolvers

pub mod graphql;
pub mod rest;
pub mod router;

pub use rest::state::AppState;
pub use router::build_router;
