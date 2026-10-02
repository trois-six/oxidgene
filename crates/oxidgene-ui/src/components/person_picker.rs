//! The one person picker (`docs/ui-common.md` §4.2): the person chosen,
//! drawn as a person-search row, with the buttons changing or clearing the
//! choice, and the shared person search while a new one is being chosen.

use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::search_person::SearchPerson;

/// A choice of one of the tree's persons.
///
/// A required picker — one with no `clear_label` — opens straight on the
/// search while nobody is chosen; an optional one says `empty_label` and
/// offers to choose. Cancelling the search keeps the current choice.
#[component]
pub fn PersonPicker(
    tree_id: Uuid,
    /// The chosen person's row, as the person search draws it; `None`
    /// while nobody is chosen.
    selected: Option<Element>,
    search_placeholder: String,
    change_label: String,
    /// The button clearing the choice; a required picker has none.
    #[props(default)]
    clear_label: Option<String>,
    /// What an optional picker says while nobody is chosen.
    #[props(default)]
    empty_label: Option<String>,
    on_change: EventHandler<Option<Uuid>>,
) -> Element {
    let mut searching = use_signal(|| false);
    let required = clear_label.is_none();
    if searching() || (required && selected.is_none()) {
        return rsx! {
            div { class: "person-picker",
                SearchPerson {
                    tree_id,
                    placeholder: search_placeholder,
                    on_select: move |person_id: Uuid| {
                        searching.set(false);
                        on_change.call(Some(person_id));
                    },
                    on_cancel: move |_| searching.set(false),
                }
            }
        };
    }
    let Some(row) = selected else {
        return rsx! {
            div { class: "person-picker person-picker-empty",
                p { class: "text-muted", {empty_label.unwrap_or_default()} }
                button {
                    class: "btn btn-primary btn-sm",
                    r#type: "button",
                    onclick: move |_| searching.set(true),
                    "{change_label}"
                }
            }
        };
    };
    rsx! {
        div { class: "person-picker person-picker-display",
            div { class: "person-picker-person", {row} }
            div { class: "person-picker-actions",
                button {
                    class: "btn btn-outline btn-sm",
                    r#type: "button",
                    onclick: move |_| searching.set(true),
                    "{change_label}"
                }
                if let Some(clear) = clear_label {
                    button {
                        class: "btn btn-outline btn-sm btn-danger-outline",
                        r#type: "button",
                        onclick: move |_| on_change.call(None),
                        "{clear}"
                    }
                }
            }
        }
    }
}
