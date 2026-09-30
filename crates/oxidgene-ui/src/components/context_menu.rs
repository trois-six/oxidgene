//! Floating context menu for person nodes in pedigree charts.
//!
//! Shows actions like Edit, Merge, Edit Union, Add Spouse, Add Child,
//! Add Sibling, Relationship, Delete when the user interacts with a person
//! box.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::i18n::use_i18n;

/// Shared fixed-position surface for contextual action menus.
///
/// It owns the click-outside backdrop, the native context-menu dismissal and
/// the dismissal on a window resize; callers supply only their
/// domain-specific actions.
#[derive(Props, Clone, PartialEq)]
pub struct ContextMenuSurfaceProps {
    pub x: f64,
    pub y: f64,
    /// Extra class for menus with a specialised width or child layout.
    #[props(default)]
    pub menu_class: String,
    pub on_close: EventHandler<()>,
    pub children: Element,
}

#[component]
pub fn ContextMenuSurface(props: ContextMenuSurfaceProps) -> Element {
    let style = format!("left: {}px; top: {}px;", props.x, props.y);
    let menu_class = if props.menu_class.is_empty() {
        "context-menu".to_string()
    } else {
        format!("context-menu {}", props.menu_class)
    };

    // The size the window had when the menu opened. The menu is placed at
    // coordinates measured then, which a resize moves out from under it — so
    // it closes, as a native menu does, rather than float off its anchor.
    let mut opened_at = use_signal(|| None::<(f64, f64)>);

    rsx! {
        div {
            class: "context-menu-backdrop",
            // The backdrop covers the window, so it resizes with it. The
            // first report is the size it opened at, not a resize.
            onresize: move |evt: Event<ResizeData>| {
                let Ok(size) = evt.get_content_box_size() else {
                    return;
                };
                let size = (size.width, size.height);
                match opened_at() {
                    None => opened_at.set(Some(size)),
                    Some(opened) if opened != size => props.on_close.call(()),
                    Some(_) => {}
                }
            },
            onclick: move |evt: Event<MouseData>| {
                evt.stop_propagation();
                props.on_close.call(());
            },
            oncontextmenu: move |evt: Event<MouseData>| {
                evt.prevent_default();
                props.on_close.call(());
            },
        }
        // The menu is a DOM child of whatever opened it, so swallow clicks
        // here: dismissing the menu or picking an action must never also
        // activate the element underneath.
        div {
            class: "{menu_class}",
            style: "{style}",
            onclick: move |evt: Event<MouseData>| evt.stop_propagation(),
            {props.children}
        }
    }
}

/// Actions that can be triggered from the context menu.
#[derive(Debug, Clone, PartialEq)]
pub enum PersonAction {
    Edit,
    Merge,
    AddParents,
    AddSpouse,
    AddChild,
    AddSibling,
    EditUnion,
    EditSpecificUnion(Uuid),
    /// Trace how the person is related to somebody else.
    Kinship,
    /// Make a relative the chart's focus.
    GoTo(Uuid),
    Delete,
}

/// Props for [`ContextMenu`].
#[derive(Props, Clone, PartialEq)]
pub struct ContextMenuProps {
    pub person_name: String,
    pub x: f64,
    pub y: f64,
    #[props(default = false)]
    pub has_union: bool,
    /// List of unions: (family_id, partner_name, marriage_year).
    #[props(default)]
    pub unions: Vec<(Uuid, String, String)>,
    /// Relatives the chart does not draw around the person, to go to: a
    /// heading per kind, then (person, label) for each. Empty, no "Go to…".
    #[props(default)]
    pub go_to: Vec<(String, Vec<(Uuid, String)>)>,
    pub on_action: EventHandler<PersonAction>,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn ContextMenu(props: ContextMenuProps) -> Element {
    let i18n = use_i18n();
    let mut show_union_sub = use_signal(|| false);
    let mut show_go_to = use_signal(|| false);

    let union_count = props.unions.len();

    rsx! {
        ContextMenuSurface {
            x: props.x,
            y: props.y,
            on_close: props.on_close,
            div { class: "context-menu-header", "{props.person_name}" }

            if show_go_to() {
                button {
                    class: "context-menu-item context-menu-back",
                    onclick: move |_| show_go_to.set(false),
                    "\u{2190} {i18n.t(\"common.back\")}"
                }
                for (k, (heading, people)) in props.go_to.iter().enumerate() {
                    div { key: "g-{k}",
                        hr { class: "context-menu-divider" }
                        div { class: "context-menu-subheader", "{heading}" }
                        for (pid, label) in people.iter() {
                            {
                                let pid = *pid;
                                let on_action = props.on_action;
                                rsx! {
                                    button {
                                        key: "{pid}",
                                        class: "context-menu-item",
                                        onclick: move |_| on_action.call(PersonAction::GoTo(pid)),
                                        "{label}"
                                    }
                                }
                            }
                        }
                    }
                }
            } else if show_union_sub() {
                // Union sub-list: back arrow + union entries.
                button {
                    class: "context-menu-item context-menu-back",
                    onclick: move |_| show_union_sub.set(false),
                    "\u{2190} {i18n.t(\"common.back\")}"
                }
                hr { class: "context-menu-divider" }
                for (fid, partner, year) in props.unions.iter() {
                    {
                        let fid = *fid;
                        let label = if year.is_empty() {
                            partner.clone()
                        } else {
                            format!("{partner}  \u{1F48D} {year}")
                        };
                        let on_action = props.on_action;
                        rsx! {
                            button {
                                class: "context-menu-item",
                                onclick: move |_| on_action.call(PersonAction::EditSpecificUnion(fid)),
                                "{label}"
                            }
                        }
                    }
                }
            } else {
                // Main action list.
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Edit),
                    {i18n.t("context.edit_individual")}
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Merge),
                    {i18n.t("context.merge")}
                }
                if props.has_union {
                    if union_count > 1 {
                        button {
                            class: "context-menu-item",
                            onclick: move |_| show_union_sub.set(true),
                            {i18n.t("context.edit_union_submenu")}
                        }
                    } else {
                        button {
                            class: "context-menu-item",
                            onclick: move |_| props.on_action.call(PersonAction::EditUnion),
                            {i18n.t("context.edit_union")}
                        }
                    }
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSpouse),
                    {i18n.t("context.add_spouse")}
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddChild),
                    {i18n.t("context.add_child")}
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSibling),
                    {i18n.t("context.add_sibling")}
                }
                if !props.go_to.is_empty() {
                    hr { class: "context-menu-divider" }
                    button {
                        class: "context-menu-item",
                        onclick: move |_| show_go_to.set(true),
                        {i18n.t("context.go_to")}
                    }
                }
                hr { class: "context-menu-divider" }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Kinship),
                    {i18n.t("context.kinship")}
                }
                hr { class: "context-menu-divider" }
                button {
                    class: "context-menu-item context-menu-danger",
                    onclick: move |_| props.on_action.call(PersonAction::Delete),
                    {i18n.t("common.delete")}
                }
            }
        }
    }
}
