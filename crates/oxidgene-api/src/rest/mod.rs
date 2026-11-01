//! REST API handlers for OxidGene.
//!
//! This module provides Axum handlers for all REST endpoints under `/api/v1`.

#[cfg(feature = "auth")]
pub mod access_log;
#[cfg(feature = "auth")]
pub mod admin_tree;
#[cfg(feature = "auth")]
pub mod admin_user;
pub mod authorize;
pub mod citation;
pub mod dictionary;
pub mod dto;
pub mod error;
pub mod event;
#[cfg(feature = "auth")]
pub mod exclusion;
pub mod family;
pub mod family_member;
pub mod file_export;
pub mod file_import;
pub mod gedcom;
pub mod geneanet;
pub mod history;
#[cfg(feature = "auth")]
pub mod me;
pub mod media;
pub mod media_link;
#[cfg(feature = "auth")]
pub mod member;
pub mod note;
pub mod openapi;
pub mod person;
pub mod person_name;
pub mod place;
pub mod profile;
pub mod reference;
pub mod repository;
#[cfg(feature = "auth")]
pub mod sharing;
pub mod source;
pub mod state;
pub mod statistics;
pub mod suggestion;
pub mod tag;
pub mod tools;
pub mod tree;

#[cfg(feature = "auth")]
pub mod user;
pub mod vignette;
