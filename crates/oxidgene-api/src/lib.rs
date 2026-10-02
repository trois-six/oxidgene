//! OxidGene API layer: REST and GraphQL endpoints.
//!
//! This crate provides:
//! - REST handlers for all CRUD endpoints under `/api/v1`
//! - GraphQL schema and resolvers at `/graphql`
//! - A router builder to wire up all routes

pub mod access;
pub mod app_dirs;
mod embedded;
mod error_contract;
#[cfg(feature = "graphql")]
pub mod graphql;
pub mod limits;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod media;
pub mod memory;
pub mod profile;
pub mod reference;
pub mod request_context;
pub mod rest;
pub mod router;
pub mod service;
pub mod startup;
pub mod workdir;

#[cfg(feature = "graphql")]
pub use graphql::{OxidGeneSchema, build_schema};
pub use rest::state::AppState;
pub use router::build_router;
