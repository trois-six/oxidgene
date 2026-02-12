//! Clickable person box for pedigree charts and tree visualisations.

use dioxus::prelude::*;
use oxidgene_core::Sex;

use crate::router::Route;
use crate::utils::{sex_icon_class, sex_symbol};

/// Props for [`PersonNode`].
#[derive(Props, Clone, PartialEq)]
pub struct PersonNodeProps {
    /// Display name for the person (already resolved).
    pub name: String,
    /// Sex of the person (used for icon styling).
    pub sex: Sex,
    /// Whether this node represents the currently-viewed person.
    #[props(default = false)]
    pub is_current: bool,
    /// Tree ID (for navigation link).
    pub tree_id: String,
    /// Person ID (for navigation link).
    pub person_id: String,
}

/// A compact person box used in pedigree / ancestry charts.
///
/// Renders as a clickable [`Link`] styled with sex-specific colouring and a
/// `"tree-node"` CSS class (plus `"current"` when `is_current` is set).
#[component]
pub fn PersonNode(props: PersonNodeProps) -> Element {
    let node_class = if props.is_current {
        "tree-node current"
    } else {
        "tree-node"
    };
    let icon_class = format!("sex-icon {}", sex_icon_class(&props.sex));
    let symbol = sex_symbol(&props.sex);

    rsx! {
        Link {
            to: Route::PersonDetail {
                tree_id: props.tree_id.clone(),
                person_id: props.person_id.clone(),
            },
            class: node_class,
            span { class: icon_class, "{symbol}" }
            "{props.name}"
        }
    }
}

/// An empty slot in a pedigree chart that can be clicked to add a person.
#[derive(Props, Clone, PartialEq)]
pub struct EmptySlotProps {
    /// Label for the slot (e.g. "Add Father", "Add Mother").
    pub label: String,
    /// Called when the user clicks the empty slot.
    pub on_click: EventHandler<()>,
}

/// Renders a placeholder box that the user can click to add a missing person
/// (e.g. an unknown parent).
#[component]
pub fn EmptySlot(props: EmptySlotProps) -> Element {
    rsx! {
        button {
            class: "tree-node empty-slot",
            onclick: move |_| props.on_click.call(()),
            span { class: "sex-icon", "+" }
            "{props.label}"
        }
    }
}
