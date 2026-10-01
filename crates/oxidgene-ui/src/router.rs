//! Dioxus router definition for the OxidGene frontend.
//!
//! Uses [`dioxus_router::Routable`] to define typed, compile-time-checked
//! routes.  The [`Route`] enum is consumed by both web and desktop targets.

use dioxus::prelude::*;

use crate::pages::statistics::Statistics;
use crate::pages::tools::Tools;
use crate::pages::{
    app_settings::AppSettings, couple_detail::CoupleDetail, dictionary::Dictionary, home::Home,
    kinship::Kinship, not_found::NotFound, person_detail::PersonDetail,
    person_history::PersonHistory, search_results::SearchResults, settings::Settings,
    tree_detail::TreeDetail,
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

    /// Detail view for a single tree (shows persons, families, etc.).
    /// Optional `person` query param to focus on a specific person.
    #[route("/trees/:tree_id?:person")]
    TreeDetail {
        tree_id: String,
        person: Option<String>,
    },

    /// Search results page for a tree.
    ///
    /// `origin` records which view the search was launched from ("person" for
    /// the person-detail page, empty/anything else for the pedigree view), so
    /// clicking a result returns to that same view.
    #[route("/trees/:tree_id/search?:last&:first&:origin")]
    SearchResults {
        tree_id: String,
        last: String,
        first: String,
        origin: String,
    },

    /// Detail view for a person within a tree.
    #[route("/trees/:tree_id/persons/:person_id")]
    PersonDetail { tree_id: String, person_id: String },

    /// Every recorded version of a person, compared side by side.
    #[route("/trees/:tree_id/persons/:person_id/history")]
    PersonHistory { tree_id: String, person_id: String },

    /// Couple view: both spouses of a family side by side, and what they
    /// share across the two.
    #[route("/trees/:tree_id/couples/:family_id")]
    CoupleDetail { tree_id: String, family_id: String },

    /// How two persons of a tree are related, generation by generation.
    /// `to` is empty until a second person is chosen.
    #[route("/trees/:tree_id/kinship?:from&:to")]
    Kinship {
        tree_id: String,
        from: String,
        to: String,
    },

    /// Dictionary page for a tree: family names, sources, places, occupations
    /// with usage counts.
    #[route("/trees/:tree_id/dictionary")]
    Dictionary { tree_id: String },

    /// Statistics page for a tree: heat map of places, charts per period and
    /// notable records.
    #[route("/trees/:tree_id/statistics")]
    Statistics { tree_id: String },

    /// Tools page for a tree: anomalies, ancestry completeness, potential
    /// duplicates and date tools, one tab each.
    #[route("/trees/:tree_id/tools")]
    Tools { tree_id: String },

    /// Settings page for a tree.
    #[route("/trees/:tree_id/settings")]
    Settings { tree_id: String },

    /// Application-wide settings (theme, language).
    #[route("/settings")]
    AppSettings {},

    /// Catch-all 404 page.
    #[end_layout]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

impl Route {
    /// The tree the page is about, for every page inside one.
    ///
    /// Deliberately exhaustive, without a catch-all arm: a new tree page has
    /// to be listed here, or it builds without the sidebar buttons that
    /// find their tree through this.
    pub fn tree_id(&self) -> Option<&str> {
        match self {
            Route::TreeDetail { tree_id, .. }
            | Route::SearchResults { tree_id, .. }
            | Route::PersonDetail { tree_id, .. }
            | Route::PersonHistory { tree_id, .. }
            | Route::CoupleDetail { tree_id, .. }
            | Route::Kinship { tree_id, .. }
            | Route::Dictionary { tree_id }
            | Route::Statistics { tree_id }
            | Route::Tools { tree_id }
            | Route::Settings { tree_id } => Some(tree_id),
            Route::Home {} | Route::AppSettings {} | Route::NotFound { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Route;

    #[test]
    fn every_page_under_a_tree_names_it() {
        let tree = || "t".to_string();
        let pages = [
            Route::TreeDetail {
                tree_id: tree(),
                person: None,
            },
            Route::SearchResults {
                tree_id: tree(),
                last: String::new(),
                first: String::new(),
                origin: String::new(),
            },
            Route::PersonDetail {
                tree_id: tree(),
                person_id: "p".into(),
            },
            Route::PersonHistory {
                tree_id: tree(),
                person_id: "p".into(),
            },
            Route::CoupleDetail {
                tree_id: tree(),
                family_id: "f".into(),
            },
            Route::Kinship {
                tree_id: tree(),
                from: "p".into(),
                to: String::new(),
            },
            Route::Dictionary { tree_id: tree() },
            Route::Statistics { tree_id: tree() },
            Route::Tools { tree_id: tree() },
            Route::Settings { tree_id: tree() },
        ];
        for page in pages {
            assert_eq!(page.tree_id(), Some("t"), "{page:?}");
        }
        assert_eq!(Route::Home {}.tree_id(), None);
        assert_eq!(Route::AppSettings {}.tree_id(), None);
    }
}
