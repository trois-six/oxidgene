//! Floating context menu for person nodes in pedigree charts.
//!
//! Shows actions like Edit, Add Parents, Add Spouse, Add Child, Edit Union,
//! Delete when the user interacts with a person box in the pedigree view.

use dioxus::prelude::*;

/// Actions that can be triggered from the context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonAction {
    /// Open the person edit form.
    Edit,
    /// Add parents for this person.
    AddParents,
    /// Add a spouse / union.
    AddSpouse,
    /// Add a child.
    AddChild,
    /// Edit the union/family this person belongs to as a spouse.
    EditUnion,
    /// Delete this person.
    Delete,
}

/// Props for [`ContextMenu`].
#[derive(Props, Clone, PartialEq)]
pub struct ContextMenuProps {
    /// The display name of the person (shown as the menu header).
    pub person_name: String,
    /// Absolute X position (px) for the menu.
    pub x: f64,
    /// Absolute Y position (px) for the menu.
    pub y: f64,
    /// Whether this person is a spouse in at least one family.
    #[props(default = false)]
    pub has_union: bool,
    /// Called with the chosen [`PersonAction`] when the user clicks a menu item.
    pub on_action: EventHandler<PersonAction>,
    /// Called when the menu should be dismissed (backdrop click).
    pub on_close: EventHandler<()>,
}

/// A floating context menu anchored at an absolute position.
///
/// Renders a backdrop overlay that dismisses the menu on click, plus a card
/// with action buttons.
#[component]
pub fn ContextMenu(props: ContextMenuProps) -> Element {
    let style = format!("left: {}px; top: {}px;", props.x, props.y);

    rsx! {
        // Invisible backdrop to catch clicks outside the menu.
        div {
            class: "context-menu-backdrop",
            onclick: move |_| props.on_close.call(()),
        }
        div {
            class: "context-menu",
            style: style,
            div { class: "context-menu-header", "{props.person_name}" }
            button {
                class: "context-menu-item",
                onclick: move |_| props.on_action.call(PersonAction::Edit),
                "Edit"
            }
            button {
                class: "context-menu-item",
                onclick: move |_| props.on_action.call(PersonAction::AddParents),
                "Add Parents"
            }
            button {
                class: "context-menu-item",
                onclick: move |_| props.on_action.call(PersonAction::AddSpouse),
                "Add Spouse"
            }
            button {
                class: "context-menu-item",
                onclick: move |_| props.on_action.call(PersonAction::AddChild),
                "Add Child"
            }
            if props.has_union {
                button {
                    class: "context-menu-item",
                    onclick: move |_| props.on_action.call(PersonAction::EditUnion),
                    "Edit Union"
                }
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
