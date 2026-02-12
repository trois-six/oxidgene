//! 404 Not Found page.

use dioxus::prelude::*;

use crate::router::Route;

/// Catch-all page for unknown routes.
#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    let path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "empty-state",
            h1 { "404 — Not Found" }
            p { class: "text-muted", "The page " strong { "{path}" } " does not exist." }
            Link { to: Route::Home {},
                button { class: "btn btn-primary", style: "margin-top: 16px;", "Go Home" }
            }
        }
    }
}
