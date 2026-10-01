//! Shared vertical icon sidebar for tree-related pages.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::print::PrintAction;
use crate::i18n::use_i18n;
use crate::router::Route;

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
    on_dictionary: EventHandler<()>,
    on_settings: EventHandler<()>,
    #[props(default = true)] show_middle_separator: bool,
    #[props(default = true)] show_add_person: bool,
    #[props(default = true)] show_dictionary: bool,
    #[props(default = true)] show_settings: bool,
    #[props(default)] children: Element,
) -> Element {
    let i18n = use_i18n();
    // The statistics button needs no callback: every tree page's route names
    // its tree, and the button leads to that tree's statistics.
    let route = use_route::<Route>();
    let statistics_tree = route.tree_id().map(str::to_string);

    let profile_class = button_class(active_view == TreeSidebarView::Profile);
    let couple_class = button_class(active_view == TreeSidebarView::Couple);
    let pedigree_class = button_class(active_view == TreeSidebarView::Pedigree);

    rsx! {
        nav { class: "isb tree-icon-sidebar",
            button {
                class: "{profile_class}",
                title: "{i18n.t(\"pedigree.profile_view\")}",
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

            if show_dictionary || show_settings || statistics_tree.is_some() {
                div { class: "isb-hr" }
            }

            if show_dictionary {
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"dictionary.breadcrumb\")}",
                    onclick: move |_| on_dictionary.call(()),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        path { d: "M12 7v14" }
                        path { d: "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z" }
                    }
                }
            }

            if let Some(tree_id) = statistics_tree.clone() {
                TreePageButtons { tree_id, route: route.clone() }
            }

            PrintAction {}

            if show_settings {
                button {
                    class: "isb-btn",
                    title: "{i18n.t(\"settings.breadcrumb\")}",
                    onclick: move |_| on_settings.call(()),
                    svg {
                        width: "16",
                        height: "16",
                        fill: "none",
                        "viewBox": "0 0 24 24",
                        stroke: "currentColor",
                        "strokeWidth": "2",
                        circle { cx: "12", cy: "12", r: "3" }
                        path { d: "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" }
                    }
                }
            }
        }
    }
}

/// The sidebar of a tree's tool pages — statistics, tools, search, kinship,
/// history, dictionary, settings: no view of its own and no person to add,
/// its buttons leading to the selected person's profile and pedigree and to
/// the tree's dictionary and settings.
#[component]
pub fn ToolPageSidebar(
    tree_id: String,
    selected_person_id: Option<Uuid>,
    #[props(default = true)] show_dictionary: bool,
    #[props(default = true)] show_settings: bool,
) -> Element {
    let nav = use_navigator();
    let (profile_tree, pedigree_tree, dictionary_tree) =
        (tree_id.clone(), tree_id.clone(), tree_id.clone());
    rsx! {
        TreeIconSidebar {
            active_view: TreeSidebarView::None,
            selected_person_id,
            show_middle_separator: false,
            show_add_person: false,
            show_dictionary,
            show_settings,
            on_profile_view: move |pid: Option<Uuid>| {
                if let Some(pid) = pid {
                    nav.push(Route::PersonDetail {
                        tree_id: profile_tree.clone(),
                        person_id: pid.to_string(),
                    });
                }
            },
            on_pedigree_view: move |pid: Option<Uuid>| {
                nav.push(Route::TreeDetail {
                    tree_id: pedigree_tree.clone(),
                    person: pid.map(|pid| pid.to_string()),
                });
            },
            on_add_person: move |_| {},
            on_dictionary: move |_| {
                nav.push(Route::Dictionary { tree_id: dictionary_tree.clone() });
            },
            on_settings: move |_| {
                nav.push(Route::Settings { tree_id: tree_id.clone() });
            },
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
    let nav = use_navigator();
    let (profile_tree, couple_tree, pedigree_tree, dictionary_tree) = (
        tree_id.clone(),
        tree_id.clone(),
        tree_id.clone(),
        tree_id.clone(),
    );
    rsx! {
        TreeIconSidebar {
            active_view,
            selected_person_id,
            couple_family_id,
            on_profile_view: move |pid: Option<Uuid>| {
                if let (false, Some(pid)) = (active_view == TreeSidebarView::Profile, pid) {
                    nav.push(Route::PersonDetail {
                        tree_id: profile_tree.clone(),
                        person_id: pid.to_string(),
                    });
                }
            },
            on_couple_view: move |family_id: Uuid| {
                if active_view != TreeSidebarView::Couple {
                    nav.push(Route::CoupleDetail {
                        tree_id: couple_tree.clone(),
                        family_id: family_id.to_string(),
                    });
                }
            },
            on_pedigree_view: move |pid: Option<Uuid>| {
                nav.push(Route::TreeDetail {
                    tree_id: pedigree_tree.clone(),
                    person: pid.map(|pid| pid.to_string()),
                });
            },
            on_add_person,
            on_dictionary: move |_| {
                nav.push(Route::Dictionary { tree_id: dictionary_tree.clone() });
            },
            on_settings: move |_| {
                nav.push(Route::Settings { tree_id: tree_id.clone() });
            },
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

/// The buttons to a tree's statistics and tools, found from the route of
/// any of its pages.
#[component]
fn TreePageButtons(tree_id: String, route: Route) -> Element {
    let i18n = use_i18n();
    let nav = use_navigator();
    let statistics_tree = tree_id.clone();
    rsx! {
        button {
            class: button_class(matches!(route, Route::Statistics { .. })),
            title: "{i18n.t(\"stats.breadcrumb\")}",
            onclick: move |_| {
                nav.push(Route::Statistics { tree_id: statistics_tree.clone() });
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                path { d: "M3 3v18h18" }
                path { d: "M7 15l4-4 3 3 5-6" }
            }
        }
        button {
            class: button_class(matches!(route, Route::Tools { .. })),
            title: "{i18n.t(\"tools.breadcrumb\")}",
            onclick: move |_| {
                nav.push(Route::Tools { tree_id: tree_id.clone() });
            },
            svg {
                width: "16",
                height: "16",
                fill: "none",
                "viewBox": "0 0 24 24",
                stroke: "currentColor",
                "strokeWidth": "2",
                path { d: "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z" }
            }
        }
    }
}
