//! The switch between a list and a grid of the same results
//! (`docs/ui-common.md` §4.16): the home page's trees, the search results.

use dioxus::prelude::*;

/// Two icon buttons, list then grid, the one shown pressed. Each page names
/// its own grid, whose cards differ from page to page.
#[component]
pub fn ViewToggle(
    /// Whether the list is shown, else the grid.
    list: bool,
    list_label: String,
    grid_label: String,
    on_change: EventHandler<bool>,
) -> Element {
    rsx! {
        div { class: "view-toggle",
            for (is_list , label , glyph) in [(true, list_label, "\u{2630}"), (false, grid_label, "\u{25A6}")] {
                button {
                    key: "{is_list}",
                    r#type: "button",
                    class: if list == is_list { "view-toggle-btn active" } else { "view-toggle-btn" },
                    title: "{label}",
                    "aria-label": "{label}",
                    "aria-pressed": list == is_list,
                    onclick: move |_| {
                        if list != is_list {
                            on_change.call(is_list);
                        }
                    },
                    "{glyph}"
                }
            }
        }
    }
}
