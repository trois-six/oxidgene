//! OxidGene frontend library — shared Dioxus components for web and desktop.
//!
//! This crate provides:
//! - [`api::ApiClient`] — typed HTTP client for the backend REST API
//! - [`router::Route`] — compile-time-checked routing
//! - [`components`] — shared layout and reusable UI widgets
//! - [`pages`] — one component per route
//! - [`theme`] — the colour palette, built in or supplied by the user
//! - [`App`] — top-level application component

pub mod api;
pub mod components;
pub mod geneanet;
pub mod i18n;
pub mod image_host;
pub mod pages;
pub mod prefs;
pub mod router;
pub mod theme;
pub mod ui_observability;
pub mod utils;

use dioxus::prelude::*;

/// Top-level application component.
///
/// Renders the [`router::Route`] router.  The caller must provide an
/// [`api::ApiClient`] in the Dioxus context *before* launching.
#[component]
pub fn App() -> Element {
    rsx! {
        Router::<router::Route> {}
    }
}
