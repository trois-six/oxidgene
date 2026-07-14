//! Shared vertical icon sidebar for tree-related pages.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::i18n::use_i18n;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeSidebarView {
    None,
    Profile,
    Pedigree,
}

#[component]
pub fn TreeIconSidebar(
    active_view: TreeSidebarView,
    selected_person_id: Option<Uuid>,
    on_profile_view: EventHandler<Option<Uuid>>,
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

    let profile_class = if active_view == TreeSidebarView::Profile {
        "isb-btn isb-btn-active"
    } else {
        "isb-btn"
    };
    let pedigree_class = if active_view == TreeSidebarView::Pedigree {
        "isb-btn isb-btn-active"
    } else {
        "isb-btn"
    };

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

            if show_dictionary || show_settings {
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
