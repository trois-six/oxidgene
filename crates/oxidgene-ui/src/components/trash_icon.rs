//! The trash can of a delete button: one drawing, so every icon-only delete
//! in the application looks the same. The button around it names the action.

use dioxus::prelude::*;

/// The trash can, sized for the button that holds it.
#[component]
pub fn TrashIcon(#[props(default = "")] class: &'static str) -> Element {
    rsx! {
        svg {
            class: "{class}",
            width: "16",
            height: "16",
            fill: "none",
            "viewBox": "0 0 24 24",
            stroke: "currentColor",
            "strokeWidth": "2",
            "aria-hidden": "true",
            path { d: "M3 6h18" }
            path { d: "M8 6V4h8v2" }
            path { d: "M19 6l-1 14H6L5 6" }
        }
    }
}
