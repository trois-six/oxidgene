//! The one empty state (`docs/ui-common.md` §4.7): content that genuinely
//! has nothing to show — no trees, no events, no match for a filter.
//!
//! A list still loading or one that failed to load is not empty: it says so
//! with `.loading` or `.error-msg`, never with this.

use dioxus::prelude::*;

/// Nothing to show: an optional icon and title, the explanation (the
/// children), and at most one action leading out of it.
#[component]
pub fn EmptyState(
    /// A decorative picture above the title.
    #[props(default)]
    icon: Option<Element>,
    #[props(default)] title: Option<String>,
    /// The one relevant action, such as clearing a filter.
    #[props(default)]
    action: Option<Element>,
    /// Further classes, e.g. `card` when the state stands in for a card.
    #[props(default)]
    class: String,
    children: Element,
) -> Element {
    rsx! {
        div { class: "empty-state {class}",
            if let Some(icon) = icon {
                div { class: "empty-state-icon", "aria-hidden": "true", {icon} }
            }
            if let Some(title) = title {
                h3 { "{title}" }
            }
            {children}
            if let Some(action) = action {
                div { class: "empty-state-action", {action} }
            }
        }
    }
}
