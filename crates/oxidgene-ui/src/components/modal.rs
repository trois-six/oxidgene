//! The one modal shell every dialog is drawn in.

use dioxus::prelude::*;

/// A modal dialog: the backdrop, and the card holding `children`.
///
/// `on_close` answers a press on the backdrop and the Escape key; neither
/// closes the dialog while `busy`, so a running operation can neither be
/// abandoned half-way nor fired twice. A dialog whose question must be
/// answered sets `close_on_backdrop` to false; Escape still closes it.
///
/// The card is a `dialog` with `aria-modal`, named by `label`. It takes the
/// focus when it opens unless one of its fields already has it, so Escape
/// works straight away.
///
/// The backdrop closes on press, not click: a click fires on the common
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
    rsx! {
        div {
            class: "modal-backdrop {backdrop_class}",
            onmousedown: move |_| {
                if close_on_backdrop && !busy {
                    on_close.call(());
                }
            },
            div {
                class: "{class}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": label,
                "aria-busy": busy,
                tabindex: "-1",
                onmousedown: move |e: Event<MouseData>| e.stop_propagation(),
                onclick: move |e: Event<MouseData>| e.stop_propagation(),
                onkeydown: move |e: KeyboardEvent| {
                    if e.key() == Key::Escape {
                        e.stop_propagation();
                        if !busy {
                            on_close.call(());
                        }
                    } else if let Some(on_key) = on_key {
                        on_key.call(e);
                    }
                },
                onmounted: move |_| {
                    document::eval(FOCUS_DIALOG_JS);
                },
                {children}
            }
        }
    }
}

/// Focuses the newest open dialog after the fields it holds had their
/// chance to take the focus themselves.
const FOCUS_DIALOG_JS: &str = r#"
requestAnimationFrame(() => {
    const dialogs = document.querySelectorAll('[role="dialog"][aria-modal="true"]');
    const dialog = dialogs[dialogs.length - 1];
    if (dialog && !dialog.contains(document.activeElement)) {
        dialog.focus({ preventScroll: true });
    }
});
"#;
