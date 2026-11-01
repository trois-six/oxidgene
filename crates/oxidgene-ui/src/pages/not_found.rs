//! 404 Not Found page, and the forbidden page of a tree page the viewer
//! may not open.

use dioxus::prelude::*;

use crate::components::empty_state::EmptyState;
use crate::i18n::use_i18n;
use crate::router::Route;
use crate::ui_observability::{UiPage, use_ui_load_trace};

/// Catch-all page for unknown routes.
#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    let _load_trace = use_ui_load_trace(UiPage::NotFound);
    let i18n = use_i18n();
    let path = format!("/{}", segments.join("/"));
    rsx! {
        EmptyState {
            h1 { {i18n.t("not_found.title")} }
            p { class: "text-muted",
                {i18n.t("not_found.message_prefix")}
                strong { "{path}" }
                {i18n.t("not_found.message_suffix")}
            }
            Link { to: Route::Home {},
                button { class: "btn btn-primary not-found-action",
                    {i18n.t("not_found.go_home")}
                }
            }
        }
    }
}

/// What a page of a tree the viewer sees shows in place of its body when
/// the viewer lacks the permission it needs, opened by its address: the
/// server's `403` (docs/authorization.md §4.3, §11).
#[component]
pub fn Forbidden() -> Element {
    let i18n = use_i18n();
    rsx! {
        EmptyState {
            h1 { {i18n.t("forbidden.title")} }
            p { class: "text-muted", {i18n.t("forbidden.message")} }
            Link { to: Route::Home {},
                button { class: "btn btn-primary not-found-action",
                    {i18n.t("not_found.go_home")}
                }
            }
        }
    }
}
