//! Floating context menu for person nodes in pedigree charts.
//!
//! Shows actions like Edit, Merge, Edit Union, Add Spouse, Add Child,
//! Add Sibling, Delete when the user interacts with a person box.

use dioxus::prelude::*;
use uuid::Uuid;

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
    pub on_action: EventHandler<PersonAction>,
    pub on_close: EventHandler<()>,
}

#[component]
pub fn ContextMenu(props: ContextMenuProps) -> Element {
    let style = format!("left: {}px; top: {}px;", props.x, props.y);
    let mut show_union_sub = use_signal(|| false);

    let union_count = props.unions.len();

    rsx! {
        div {
            class: "context-menu-backdrop",
            onclick: move |_| props.on_close.call(()),
        }
        div {
            class: "context-menu",
            style: style,
            div { class: "context-menu-header", "{props.person_name}" }

            if show_union_sub() {
                // Union sub-list: back arrow + union entries.
                button {
                    class: "context-menu-item context-menu-back",
                    onclick: move |_| show_union_sub.set(false),
                    "\u{2190} Back"
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
                    "Edit individual"
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::Merge),
                    "Merge with\u{2026}"
                }
                if props.has_union {
                    if union_count > 1 {
                        button {
                            class: "context-menu-item",
                            onclick: move |_| show_union_sub.set(true),
                            "Edit union \u{25B8}"
                        }
                    } else {
                        button {
                            class: "context-menu-item",
                            onclick: move |_| props.on_action.call(PersonAction::EditUnion),
                            "Edit union"
                        }
                    }
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSpouse),
                    "Add spouse"
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddChild),
                    "Add child"
                }
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::AddSibling),
                    "Add sibling"
                }
                hr { class: "context-menu-divider" }
                button {
                    class: "context-menu-item context-menu-danger",
                    onclick: move |_| props.on_action.call(PersonAction::Delete),
                    "Delete"
                }
            }
        }
    }
}
