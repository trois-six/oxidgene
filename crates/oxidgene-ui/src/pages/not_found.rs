//! 404 Not Found page.

use dioxus::prelude::*;

use crate::i18n::use_i18n;
use crate::router::Route;

/// Catch-all page for unknown routes.
#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    let i18n = use_i18n();
    let path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "empty-state",
            h1 { {i18n.t("not_found.title")} }
            p { class: "text-muted",
                {i18n.t("not_found.message_prefix")}
                strong { "{path}" }
                {i18n.t("not_found.message_suffix")}
            }
            Link { to: Route::Home {},
                button { class: "btn btn-primary", style: "margin-top: 16px;",
                    {i18n.t("not_found.go_home")}
                }
            }
        }
    }
}
