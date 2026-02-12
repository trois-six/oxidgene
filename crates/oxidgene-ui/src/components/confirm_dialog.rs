//! Reusable confirmation dialog component.
//!
//! Replaces the 10+ duplicated modal patterns across tree_list, tree_detail,
//! and person_detail pages.

use dioxus::prelude::*;

/// Props for the [`ConfirmDialog`] component.
#[derive(Props, Clone, PartialEq)]
pub struct ConfirmDialogProps {
    /// Dialog title (e.g. "Delete Tree").
    pub title: String,
    /// Body message (e.g. "Are you sure you want to delete ...?").
    pub message: String,
    /// Label for the confirm button (e.g. "Delete").
    #[props(default = "Confirm".to_string())]
    pub confirm_label: String,
    /// CSS class for the confirm button (e.g. "btn btn-danger").
    #[props(default = "btn btn-danger".to_string())]
    pub confirm_class: String,
    /// Optional error message to display inside the dialog.
    #[props(default)]
    pub error: Option<String>,
    /// Called when the user clicks the confirm button.
    pub on_confirm: EventHandler<()>,
    /// Called when the user clicks cancel or the backdrop.
    pub on_cancel: EventHandler<()>,
}

/// A modal confirmation dialog with a backdrop overlay.
///
/// # Example
///
/// ```rust,ignore
/// ConfirmDialog {
///     title: "Delete Tree",
///     message: "Are you sure?",
///     confirm_label: "Delete",
///     confirm_class: "btn btn-danger",
///     error: delete_error(),
///     on_confirm: move |_| { /* handle delete */ },
///     on_cancel: move |_| { /* close dialog */ },
/// }
/// ```
#[component]
pub fn ConfirmDialog(props: ConfirmDialogProps) -> Element {
    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| props.on_cancel.call(()),
            div {
                class: "modal-card",
                // Prevent clicks inside the card from closing the dialog.
                onclick: move |e: Event<MouseData>| e.stop_propagation(),
                h3 { "{props.title}" }
                p { style: "margin: 12px 0;", "{props.message}" }
                if let Some(err) = &props.error {
                    div { class: "error-msg", "{err}" }
                }
                div { class: "modal-actions",
                    button {
                        class: "btn btn-outline",
                        onclick: move |_| props.on_cancel.call(()),
                        "Cancel"
                    }
                    button {
                        class: "{props.confirm_class}",
                        onclick: move |_| props.on_confirm.call(()),
                        "{props.confirm_label}"
                    }
                }
            }
        }
    }
}
