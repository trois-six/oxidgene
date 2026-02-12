//! Dioxus router definition for the OxidGene frontend.
//!
//! Uses [`dioxus_router::Routable`] to define typed, compile-time-checked
//! routes.  The [`Route`] enum is consumed by both web and desktop targets.

use dioxus::prelude::*;

use crate::pages::{
    home::Home, not_found::NotFound, person_detail::PersonDetail, tree_detail::TreeDetail,
    tree_list::TreeList,
};

/// All application routes.
///
/// The `#[layout(Layout)]` attribute wraps matched routes in the shared
/// [`crate::components::layout::Layout`] component, which renders a
/// navigation bar and an [`Outlet`].
#[derive(Debug, Clone, PartialEq, Routable)]
pub enum Route {
    /// Application layout wrapper — all routes below share this chrome.
    #[layout(crate::components::layout::Layout)]
    //
    /// Home / landing page.
    #[route("/")]
    Home {},

    /// List of all genealogy trees.
    #[route("/trees")]
    TreeList {},

    /// Detail view for a single tree (shows persons, families, etc.).
    #[route("/trees/:tree_id")]
    TreeDetail { tree_id: String },

    /// Detail view for a person within a tree.
    #[route("/trees/:tree_id/persons/:person_id")]
    PersonDetail { tree_id: String, person_id: String },

    /// Catch-all 404 page.
    #[end_layout]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}
