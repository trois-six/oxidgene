//! Dioxus router definition for the OxidGene frontend.
//!
//! Uses [`dioxus_router::Routable`] to define typed, compile-time-checked
//! routes.  The [`Route`] enum is consumed by both web and desktop targets.

use dioxus::prelude::*;

pub use routes::Route;

/// The route enum alone, so that the one lint exception its derive needs
/// covers nothing else.
mod routes {
    // `Routable`'s derive (dioxus-router-macro 0.7.10, the latest 0.7)
    // wraps every segment's parse error in its route error with
    // `.map_err(|err| …(err))`. A `String` segment parses infallibly, so
    // `err` is `Infallible` and, from Rust 1.100, `unreachable_code` flags
    // that call. The code is generated, an `#[expect]` would be unfulfilled
    // on the stable toolchain, and an attribute on the enum does not reach
    // the impls the derive writes beside it: an inner allow on this module,
    // which holds the enum and nothing else, is the narrowest scope.
    #![allow(
        unreachable_code,
        reason = "Routable's derive maps the Infallible parse error of String segments"
    )]

    use dioxus::prelude::*;

    use crate::pages::statistics::Statistics;
    use crate::pages::tools::Tools;
    use crate::pages::{
        app_settings::{AppSettings, AppSettingsSection},
        couple_detail::CoupleDetail,
        dictionary::{
            Dictionary, DictionaryPlace, DictionaryPlaces, DictionaryRepositories,
            DictionaryRepository, DictionarySource, DictionarySources, DictionaryTag,
        },
        home::{Home, Invitations},
        kinship::Kinship,
        media_viewer::Media,
        not_found::NotFound,
        person_detail::PersonDetail,
        person_history::PersonHistory,
        search_results::SearchResults,
        settings::Settings,
        tree_detail::TreeDetail,
        unsubscribe::Unsubscribe,
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

        /// Where an invitation's link leads (docs/authorization.md §6.2): the
        /// homepage, whose invitations the user accepts or declines.
        #[route("/invitations")]
        Invitations {},

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

        /// The Dictionary opened on the Tags tab, unfolded on one tag (a chip or
        /// a dot elsewhere, docs/ui-common.md §4.18).
        #[route("/trees/:tree_id/dictionary/tags/:tag")]
        DictionaryTag { tree_id: String, tag: String },

        /// The Dictionary opened on the Sources tab: what a source's link in the
        /// Tags tab follows (docs/ui-dictionary.md §20).
        #[route("/trees/:tree_id/dictionary/sources")]
        DictionarySources { tree_id: String },

        /// The Dictionary opened on the Places tab (the Tags tab links to it).
        #[route("/trees/:tree_id/dictionary/places")]
        DictionaryPlaces { tree_id: String },

        /// The Dictionary opened on the Repositories tab (the Tags tab links to it).
        #[route("/trees/:tree_id/dictionary/repositories")]
        DictionaryRepositories { tree_id: String },

        /// The Dictionary opened on one source's row in the Sources tab, unfolded
        /// and focused (the Tags tab links to it). An unknown id opens the tab's
        /// root (docs/ui-dictionary.md §14).
        #[route("/trees/:tree_id/dictionary/sources/:source_id")]
        DictionarySource { tree_id: String, source_id: String },

        /// The Dictionary opened on one place's row in the Places tab, on its
        /// letter and page, unfolded and focused (the Tags tab links to it).
        #[route("/trees/:tree_id/dictionary/places/:place_id")]
        DictionaryPlace { tree_id: String, place_id: String },

        /// The Dictionary opened on one repository's row in the Repositories tab,
        /// unfolded and focused (the Tags tab links to it).
        #[route("/trees/:tree_id/dictionary/repositories/:repository_id")]
        DictionaryRepository {
            tree_id: String,
            repository_id: String,
        },

        /// One document of a tree, in the media viewer (the Tags tab links to it).
        #[route("/trees/:tree_id/media/:media_id")]
        Media { tree_id: String, media_id: String },

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

        /// The settings: the device's preferences and, in the web build's
        /// `oidc` mode, the signed-in user's account and an administrator's
        /// instance (docs/ui-app-settings.md).
        #[route("/settings")]
        AppSettings {},

        /// The settings, open on the section `section` names.
        #[route("/settings/:section")]
        AppSettingsSection { section: String },

        /// Where a mail's unsubscribe link leads (docs/messaging.md §2.3): the
        /// web build's accounts only, without signing in.
        #[route("/unsubscribe?:token")]
        Unsubscribe { token: String },

        /// Catch-all 404 page.
        #[end_layout]
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
    }
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
            | Route::DictionaryTag { tree_id, .. }
            | Route::DictionarySources { tree_id }
            | Route::DictionaryPlaces { tree_id }
            | Route::DictionaryRepositories { tree_id }
            | Route::DictionarySource { tree_id, .. }
            | Route::DictionaryPlace { tree_id, .. }
            | Route::DictionaryRepository { tree_id, .. }
            | Route::Media { tree_id, .. }
            | Route::Statistics { tree_id }
            | Route::Tools { tree_id }
            | Route::Settings { tree_id } => Some(tree_id),
            Route::Home {}
            | Route::Invitations {}
            | Route::AppSettings {}
            | Route::AppSettingsSection { .. }
            | Route::Unsubscribe { .. }
            | Route::NotFound { .. } => None,
        }
    }

    /// The kind of page, as the history lists it before what the page is
    /// about: the page's breadcrumb label where it has one.
    pub fn page_label_key(&self) -> &'static str {
        match self {
            Route::Home {} | Route::Invitations {} => "nav_history.page.home",
            Route::TreeDetail { .. } => "pedigree.breadcrumb",
            Route::SearchResults { .. } => "search.title",
            Route::PersonDetail { .. } => "nav_history.page.person",
            Route::PersonHistory { .. } => "history.breadcrumb",
            Route::CoupleDetail { .. } => "nav_history.page.couple",
            Route::Kinship { .. } => "kinship.breadcrumb",
            Route::Dictionary { .. }
            | Route::DictionaryTag { .. }
            | Route::DictionarySources { .. }
            | Route::DictionaryPlaces { .. }
            | Route::DictionaryRepositories { .. }
            | Route::DictionarySource { .. }
            | Route::DictionaryPlace { .. }
            | Route::DictionaryRepository { .. } => "dictionary.breadcrumb",
            Route::Media { .. } => "dictionary.tab.media",
            Route::Statistics { .. } => "stats.breadcrumb",
            Route::Tools { .. } => "tools.breadcrumb",
            Route::Settings { .. } => "settings.breadcrumb",
            Route::AppSettings {} | Route::AppSettingsSection { .. } => "app_settings.title",
            Route::Unsubscribe { .. } => "unsubscribe.title",
            Route::NotFound { .. } => "not_found.title",
        }
    }

    /// Whether the page shows the navbar above everything else; the others
    /// open on their contextual topbar.
    pub fn shows_navbar(&self) -> bool {
        matches!(
            self,
            Route::Home {}
                | Route::Invitations {}
                | Route::AppSettings {}
                | Route::AppSettingsSection { .. }
                | Route::Unsubscribe { .. }
        )
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
            Route::DictionaryTag {
                tree_id: tree(),
                tag: "t".into(),
            },
            Route::DictionarySources { tree_id: tree() },
            Route::DictionaryPlaces { tree_id: tree() },
            Route::DictionaryRepositories { tree_id: tree() },
            Route::DictionarySource {
                tree_id: tree(),
                source_id: "s".into(),
            },
            Route::DictionaryPlace {
                tree_id: tree(),
                place_id: "p".into(),
            },
            Route::DictionaryRepository {
                tree_id: tree(),
                repository_id: "r".into(),
            },
            Route::Media {
                tree_id: tree(),
                media_id: "m".into(),
            },
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

    /// The settings open on a section their address names.
    #[test]
    fn the_settings_open_on_a_requested_section() {
        let plain: Route = "/settings".parse().unwrap();
        assert_eq!(plain, Route::AppSettings {});
        let profile = Route::AppSettingsSection {
            section: "profile".into(),
        };
        assert_eq!(profile.to_string(), "/settings/profile");
        assert_eq!("/settings/profile".parse::<Route>().unwrap(), profile);
    }

    /// A failed sign-in comes back to the home page with its error in the
    /// query, which the home route reads past.
    #[test]
    fn the_home_page_takes_a_sign_in_error_in_its_address() {
        let route: Route = "/?auth_error=signup_closed".parse().unwrap();
        assert_eq!(route, Route::Home {});
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
            "/trees/t/dictionary/tags/0195c0de-0000-7000-8000-000000000001",
            "/trees/t/dictionary/sources",
            "/trees/t/dictionary/sources/0195c0de-0000-7000-8000-000000000003",
            "/trees/t/dictionary/places/0195c0de-0000-7000-8000-000000000004",
            "/trees/t/dictionary/repositories/0195c0de-0000-7000-8000-000000000005",
            "/trees/t/media/0195c0de-0000-7000-8000-000000000002",
            "/trees/t/statistics",
            "/trees/t/tools",
            "/trees/t/settings",
            "/settings",
            "/settings/profile",
            "/nowhere",
        ] {
            let route: Route = raw.parse().unwrap();
            let key = route.page_label_key();
            assert!(english.contains_key(key), "{raw}: {key}");
        }
    }

    /// A chip or a dot opens the Tags tab of the Dictionary on its tag, and
    /// the plain Dictionary keeps a URL without a trailing query.
    #[test]
    fn the_dictionary_opens_on_a_requested_tag() {
        let route: Route = "/trees/t/dictionary/tags/0195c0de-0000-7000-8000-000000000001"
            .parse()
            .unwrap();
        assert!(matches!(
            route,
            Route::DictionaryTag { ref tree_id, ref tag }
                if tree_id == "t" && tag == "0195c0de-0000-7000-8000-000000000001"
        ));
        let plain: Route = "/trees/t/dictionary".parse().unwrap();
        assert!(matches!(plain, Route::Dictionary { .. }));
        assert_eq!(plain.to_string(), "/trees/t/dictionary");
    }

    /// A row's route carries the record's id and round-trips through its URL,
    /// next to the tab's own route that opens the tab's root.
    #[test]
    fn a_record_opens_its_row_in_its_tab() {
        let id = "0195c0de-0000-7000-8000-000000000003";
        for (raw, expected) in [
            (
                format!("/trees/t/dictionary/sources/{id}"),
                Route::DictionarySource {
                    tree_id: "t".into(),
                    source_id: id.into(),
                },
            ),
            (
                format!("/trees/t/dictionary/places/{id}"),
                Route::DictionaryPlace {
                    tree_id: "t".into(),
                    place_id: id.into(),
                },
            ),
            (
                format!("/trees/t/dictionary/repositories/{id}"),
                Route::DictionaryRepository {
                    tree_id: "t".into(),
                    repository_id: id.into(),
                },
            ),
        ] {
            let route: Route = raw.parse().unwrap();
            assert_eq!(route, expected, "{raw}");
            assert_eq!(route.to_string(), raw);
        }
        let tab: Route = "/trees/t/dictionary/places".parse().unwrap();
        assert!(matches!(tab, Route::DictionaryPlaces { .. }));
    }

    /// A document's own route round-trips through its URL.
    #[test]
    fn a_document_has_a_route_of_its_own() {
        let route = Route::Media {
            tree_id: "t".into(),
            media_id: "0195c0de-0000-7000-8000-000000000002".into(),
        };
        assert_eq!(
            route.to_string(),
            "/trees/t/media/0195c0de-0000-7000-8000-000000000002"
        );
        assert_eq!(route.to_string().parse::<Route>().unwrap(), route);
    }

    #[test]
    fn only_the_homepage_and_the_settings_show_the_navbar() {
        assert!(Route::Home {}.shows_navbar());
        assert!(Route::AppSettings {}.shows_navbar());
        assert!(
            !Route::Dictionary {
                tree_id: "t".into(),
            }
            .shows_navbar()
        );
    }
}
