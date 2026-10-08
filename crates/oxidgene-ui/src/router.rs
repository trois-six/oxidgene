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

    /// The kind of page, as the history lists it before what the page is
    /// about: the page's breadcrumb label where it has one.
    pub fn page_label_key(&self) -> &'static str {
        match self {
            Route::Home {} => "nav_history.page.home",
            Route::TreeDetail { .. } => "pedigree.breadcrumb",
            Route::SearchResults { .. } => "search.title",
            Route::PersonDetail { .. } => "nav_history.page.person",
            Route::PersonHistory { .. } => "history.breadcrumb",
            Route::CoupleDetail { .. } => "nav_history.page.couple",
            Route::Kinship { .. } => "kinship.breadcrumb",
            Route::Dictionary { .. } => "dictionary.breadcrumb",
            Route::Statistics { .. } => "stats.breadcrumb",
            Route::Tools { .. } => "tools.breadcrumb",
            Route::Settings { .. } => "settings.breadcrumb",
            Route::AppSettings {} => "app_settings.title",
            Route::NotFound { .. } => "not_found.title",
        }
    }

    /// Whether the page shows the navbar above everything else; the others
    /// open on their contextual topbar.
    pub fn shows_navbar(&self) -> bool {
        matches!(self, Route::Home {} | Route::AppSettings {})
    }
}

/// A handler pushing the route `route` makes of the tree `tree_id` and the
/// handler's input — a person id, a family id, nothing.
///
/// Built during render, where the router's navigator is reachable.
pub fn push_tree_route<T: 'static>(
    tree_id: &str,
    route: fn(String, T) -> Route,
) -> EventHandler<T> {
    let (nav, tree_id) = (dioxus::router::navigator(), tree_id.to_string());
    EventHandler::new(move |input| {
        nav.push(route(tree_id.clone(), input));
    })
}

/// As [`push_tree_route`], replacing the current entry of the history: for
/// a page whose subject went away, such as a person merged into another.
pub fn replace_tree_route<T: 'static>(
    tree_id: &str,
    route: fn(String, T) -> Route,
) -> EventHandler<T> {
    let (nav, tree_id) = (dioxus::router::navigator(), tree_id.to_string());
    EventHandler::new(move |input| {
        nav.replace(route(tree_id.clone(), input));
    })
}

/// A person's profile in a tree.
pub fn person_route(tree_id: String, person_id: uuid::Uuid) -> Route {
    Route::PersonDetail {
        tree_id,
        person_id: person_id.to_string(),
    }
}

/// A tree's pedigree, centred on `person` when one is given.
pub fn pedigree_route(tree_id: String, person: Option<uuid::Uuid>) -> Route {
    Route::TreeDetail {
        tree_id,
        person: person.map(|person| person.to_string()),
    }
}

/// A couple's page in a tree.
pub fn couple_route(tree_id: String, family_id: uuid::Uuid) -> Route {
    Route::CoupleDetail {
        tree_id,
        family_id: family_id.to_string(),
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

    /// The history lists every kind of page by a label each language has.
    #[test]
    fn every_page_is_named_in_the_history() {
        let english = crate::i18n::Language::english().translations();
        for raw in [
            "/",
            "/trees/t?person=p",
            "/trees/t/search?last=a&first=b&origin=",
            "/trees/t/persons/p",
            "/trees/t/persons/p/history",
            "/trees/t/couples/f",
            "/trees/t/kinship?from=p&to=",
            "/trees/t/dictionary",
            "/trees/t/statistics",
            "/trees/t/tools",
            "/trees/t/settings",
            "/settings",
            "/nowhere",
        ] {
            let route: Route = raw.parse().unwrap();
            let key = route.page_label_key();
            assert!(english.contains_key(key), "{raw}: {key}");
        }
    }

    #[test]
    fn only_the_homepage_and_the_application_settings_show_the_navbar() {
        assert!(Route::Home {}.shows_navbar());
        assert!(Route::AppSettings {}.shows_navbar());
        assert!(
            !Route::Dictionary {
                tree_id: "t".into()
            }
            .shows_navbar()
        );
    }
}
