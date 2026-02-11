//! OxidGene API layer: REST and GraphQL endpoints.
//!
//! This crate provides:
//! - REST handlers for all CRUD endpoints under `/api/v1`
//! - GraphQL schema and resolvers at `/graphql`
//! - A router builder to wire up all routes

pub mod graphql;
pub mod rest;
pub mod router;

pub use graphql::{OxidGeneSchema, build_schema};
pub use rest::state::AppState;
pub use router::build_router;
