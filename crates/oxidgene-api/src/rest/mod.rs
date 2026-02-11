//! REST API handlers for OxidGene.
//!
//! This module provides Axum handlers for all REST endpoints under `/api/v1`.

pub mod citation;
pub mod dto;
pub mod error;
pub mod event;
pub mod family;
pub mod family_member;
pub mod media;
pub mod media_link;
pub mod note;
pub mod person;
pub mod person_name;
pub mod place;
pub mod source;
pub mod state;
pub mod tree;
