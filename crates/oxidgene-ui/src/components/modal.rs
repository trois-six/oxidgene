//! The one modal shell every dialog is drawn in.

use dioxus::prelude::*;

/// The native side of the dialog: `showModal()`, the refusal of the engine's
/// own closing, and the focus given back to the opener.
const MODAL_JS: &str = include_str!("modal.js");

/// Runs one call of the page's modal object on the dialog `id`.
fn modal_call(method: &str, id: &str) {
    let id = serde_json::to_string(id).unwrap_or_default();
    document::eval(&format!("{MODAL_JS}\nwindow.oxidgeneModal.{method}({id});"));
}

/// A modal dialog: the backdrop, and the card holding `children`.
///
/// `on_close` answers a press on the backdrop and the Escape key; neither
/// closes the dialog while `busy`, so a running operation can neither be
/// abandoned half-way nor fired twice. A dialog whose question must be
/// answered sets `close_on_backdrop` to false; Escape still closes it.
///
/// The dialog is a native `dialog` opened with `showModal()`, named by
/// `label`: the engine keeps the focus inside it, makes the page behind
/// inert and draws it above every other layer. It takes the focus when it
/// opens unless one of its fields already has it, so Escape works straight
/// away, and gives it back to the element that had it when it closes. The
/// `dialog` element is the full-window pressable area around the card; the
/// dimmed and blurred scrim is its `::backdrop`.
///
/// The area closes on press, not click: a click fires on the common
/// ancestor of the press and the release, so selecting text in the card and
/// releasing outside would close it.
#[component]
pub fn Modal(
    /// The card's classes, which size and lay it out.
    #[props(default = "modal-card".to_string())]
    class: String,
    /// Classes added to the backdrop.
    #[props(default)]
    backdrop_class: String,
    /// The dialog's accessible name, usually its title.
    label: Option<String>,
    on_close: EventHandler<()>,
    #[props(default)] busy: bool,
    #[props(default = true)] close_on_backdrop: bool,
    /// Keys other than Escape pressed inside the card, such as Enter moving
    /// to the next field of a form.
    on_key: Option<EventHandler<KeyboardEvent>>,
    children: Element,
) -> Element {
    let id = crate::listeners::use_element_id("modal");
    // Before any field of the dialog mounts and takes the focus.
    use_hook(|| modal_call("remember", &id));
    use_drop({
        let id = id.clone();
        move || modal_call("close", &id)
    });
    let open_id = id.clone();
    rsx! {
        dialog {
            id,
            class: "modal-backdrop {backdrop_class}",
            "aria-label": label,
            "aria-busy": busy,
            tabindex: "-1",
            onmousedown: move |_| {
                if close_on_backdrop && !busy {
                    on_close.call(());
                }
            },
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape {
                    // The engine's own closing of a modal dialog on Escape
                    // is the key's default action: Rust decides instead.
                    e.prevent_default();
                    e.stop_propagation();
                    if !busy {
                        on_close.call(());
                    }
                } else if let Some(on_key) = on_key {
                    on_key.call(e);
                }
            },
            onmounted: move |_| modal_call("open", &open_id),
            div {
                class: "{class}",
                onmousedown: move |e: Event<MouseData>| e.stop_propagation(),
                onclick: move |e: Event<MouseData>| e.stop_propagation(),
                {children}
            }
            // Everything outside the dialog is inert and under its
            // backdrop: a toast raised while it is open is drawn in it.
            crate::components::toast::ToastOutlet {}
        }
    }
}
