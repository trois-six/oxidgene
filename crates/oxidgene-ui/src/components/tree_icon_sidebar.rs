//! Shared vertical icon sidebar for tree-related pages.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::print::PrintAction;
use crate::i18n::use_i18n;
use crate::router::{Route, pedigree_route, person_route, push_tree_route};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeSidebarView {
    None,
    Profile,
    Couple,
    Pedigree,
}

#[component]
pub fn TreeIconSidebar(
    active_view: TreeSidebarView,
    selected_person_id: Option<Uuid>,
    /// The couple the couple view opens on for the selected person. Without
    /// one — a person with no known spouse — the couple button is not shown.
    #[props(default)]
    couple_family_id: Option<Uuid>,
    on_profile_view: EventHandler<Option<Uuid>>,
    #[props(default)] on_couple_view: EventHandler<Uuid>,
    on_pedigree_view: EventHandler<Option<Uuid>>,
    on_add_person: EventHandler<()>,
    #[props(default = true)] show_middle_separator: bool,
    #[props(default = true)] show_add_person: bool,
    #[props(default)] children: Element,
) -> Element {
    let i18n = use_i18n();
    // The buttons to the tree's own pages need no callback: every tree
    // page's route names its tree, and each button leads to that page of it.
    let route = use_route::<Route>();
    let tree_id = route.tree_id().map(str::to_string);

    let profile_class = button_class(active_view == TreeSidebarView::Profile);
    let couple_class = button_class(active_view == TreeSidebarView::Couple);
    let pedigree_class = button_class(active_view == TreeSidebarView::Pedigree);

    rsx! {
        nav { class: "isb tree-icon-sidebar",
            button {
                class: "{profile_class}",
                title: "{i18n.t(\"pedigree.profile_view\")}",
                "aria-label": "{i18n.t(\"pedigree.profile_view\")}",
                disabled: selected_person_id.is_none(),
                onclick: move |_| on_profile_view.call(selected_person_id),
                svg {
                    width: "16",
                    height: "16",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2",
                    circle { cx: "12", cy: "8", r: "4" }
                    path { d: "M4 21v-1a6 6 0 0 1 12 0v1" }
                }
            }

            if let Some(family_id) = couple_family_id {
                button {
                    class: "{couple_class}",
                    title: "{i18n.t(\"pedigree.couple_view\")}",
                    "aria-label": "{i18n.t(\"pedigree.couple_view\")}",
                    onclick: move |_| on_couple_view.call(family_id),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "8", cy: "8", r: "3.5" }
                        path { d: "M1.5 21v-1a6.5 6.5 0 0 1 13 0v1" }
                        circle { cx: "17", cy: "8.5", r: "3" }
                        path { d: "M16.5 13.5a6 6 0 0 1 6 6V21" }
                    }
                }
            }

            button {
                class: "{pedigree_class}",
                title: "{i18n.t(\"pedigree.tree_view\")}",
                "aria-label": "{i18n.t(\"pedigree.tree_view\")}",
                onclick: move |_| on_pedigree_view.call(selected_person_id),
                svg {
                    width: "16",
                    height: "16",
                    fill: "none",
                    "viewBox": "0 0 24 24",
                    stroke: "currentColor",
                    "strokeWidth": "2",
                    line { x1: "12", y1: "2", x2: "12", y2: "8" }
                    rect { x: "8", y: "8", width: "8", height: "4", rx: "1" }
                    line { x1: "12", y1: "12", x2: "12", y2: "15" }
                    line { x1: "6", y1: "15", x2: "18", y2: "15" }
                    line { x1: "6", y1: "15", x2: "6", y2: "18" }
                    line { x1: "18", y1: "15", x2: "18", y2: "18" }
                    rect { x: "2", y: "18", width: "8", height: "4", rx: "1" }
                    rect { x: "14", y: "18", width: "8", height: "4", rx: "1" }
                }
            }

            if show_middle_separator {
                div { class: "isb-hr" }
            }

            {children}

            if show_add_person {
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"pedigree.add_person\")}",
                    "aria-label": "{i18n.t(\"pedigree.add_person\")}",
                    onclick: move |_| on_add_person.call(()),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "10", cy: "8", r: "4" }
                        path { d: "M2 21v-1a6 6 0 0 1 12 0v1" }
                        line { x1: "20", y1: "8", x2: "20", y2: "14" }
                        line { x1: "17", y1: "11", x2: "23", y2: "11" }
                    }
                }
            }

            if let Some(tree_id) = tree_id {
                div { class: "isb-hr" }
                TreePageButton { page: TreePage::Dictionary, tree_id: tree_id.clone(), route: route.clone() }
                TreePageButton { page: TreePage::Statistics, tree_id: tree_id.clone(), route: route.clone() }
                TreePageButton { page: TreePage::Tools, tree_id: tree_id.clone(), route: route.clone() }
                PrintAction {}
                TreePageButton { page: TreePage::Settings, tree_id, route }
            }
        }
    }
}

/// The sidebar of a tree's tool pages — statistics, tools, search, kinship,
/// history, dictionary, settings: no view of its own and no person to add,
/// its buttons leading to the selected person's profile and pedigree.
#[component]
pub fn ToolPageSidebar(tree_id: String, selected_person_id: Option<Uuid>) -> Element {
    let open_profile = push_tree_route(&tree_id, person_route);
    rsx! {
        TreeIconSidebar {
            active_view: TreeSidebarView::None,
            selected_person_id,
            show_middle_separator: false,
            show_add_person: false,
            on_profile_view: move |pid: Option<Uuid>| {
                if let Some(pid) = pid {
                    open_profile.call(pid);
                }
            },
            on_pedigree_view: push_tree_route(&tree_id, pedigree_route),
            on_add_person: move |_| {},
        }
    }
}

/// The sidebar of the person and couple pages: the view on screen stays put,
/// the others open on `selected_person_id` — the couple view on
/// `couple_family_id` — and adding a person is left to the page.
#[component]
pub fn ProfilePageSidebar(
    tree_id: String,
    active_view: TreeSidebarView,
    selected_person_id: Option<Uuid>,
    couple_family_id: Option<Uuid>,
    on_add_person: EventHandler<()>,
) -> Element {
    let open_profile = push_tree_route(&tree_id, person_route);
    let open_couple = push_tree_route(&tree_id, crate::router::couple_route);
    rsx! {
        TreeIconSidebar {
            active_view,
            selected_person_id,
            couple_family_id,
            on_profile_view: move |pid: Option<Uuid>| {
                if let (false, Some(pid)) = (active_view == TreeSidebarView::Profile, pid) {
                    open_profile.call(pid);
                }
            },
            on_couple_view: move |family_id: Uuid| {
                if active_view != TreeSidebarView::Couple {
                    open_couple.call(family_id);
                }
            },
            on_pedigree_view: push_tree_route(&tree_id, pedigree_route),
            on_add_person,
        }
    }
}

/// A sidebar button's class, highlighted when `active`.
fn button_class(active: bool) -> &'static str {
    if active {
        "isb-btn isb-btn-active"
    } else {
        "isb-btn"
    }
}

/// A page of the tree as a whole, which every page of the tree leads to
/// from the sidebar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TreePage {
    Dictionary,
    Statistics,
    Tools,
    Settings,
}

impl TreePage {
    fn route(self, tree_id: String) -> Route {
        match self {
            TreePage::Dictionary => Route::Dictionary { tree_id },
            TreePage::Statistics => Route::Statistics { tree_id },
            TreePage::Tools => Route::Tools { tree_id },
            TreePage::Settings => Route::Settings { tree_id },
        }
    }

    /// Whether `route` is this page, whose button then shows as current.
    fn is_shown(self, route: &Route) -> bool {
        matches!(
            (self, route),
            (TreePage::Dictionary, Route::Dictionary { .. })
                | (TreePage::Statistics, Route::Statistics { .. })
                | (TreePage::Tools, Route::Tools { .. })
                | (TreePage::Settings, Route::Settings { .. })
        )
    }

    fn title_key(self) -> &'static str {
        match self {
            TreePage::Dictionary => "dictionary.breadcrumb",
            TreePage::Statistics => "stats.breadcrumb",
            TreePage::Tools => "tools.breadcrumb",
            TreePage::Settings => "settings.breadcrumb",
        }
    }

    fn icon(self) -> Element {
        let paths: &[&str] = match self {
            TreePage::Dictionary => &[
                "M12 7v14",
                "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z",
            ],
            TreePage::Statistics => &["M3 3v18h18", "M7 15l4-4 3 3 5-6"],
            TreePage::Tools => &[
                "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z",
            ],
            TreePage::Settings => &[
                "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
            ],
        };
        rsx! {
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                if self == TreePage::Settings {
                    circle { cx: "12", cy: "12", r: "3" }
                }
                for d in paths {
                    path { d: *d }
                }
            }
        }
    }
}

/// The button to one of the tree's own pages, highlighted on that page,
/// where pressing it does nothing.
#[component]
fn TreePageButton(page: TreePage, tree_id: String, route: Route) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let shown = page.is_shown(&route);
    rsx! {
        button {
            class: button_class(shown),
            title: i18n.t(page.title_key()),
            "aria-label": i18n.t(page.title_key()),
            "aria-current": if shown { "page" } else { "false" },
            onclick: move |_| {
                if !shown {
                    nav.push(page.route(tree_id.clone()));
                }
            },
            {page.icon()}
        }
    }
}
