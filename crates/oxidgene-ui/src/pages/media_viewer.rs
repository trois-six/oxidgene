//! One document of a tree on a route of its own, opened from the Tags tab of
//! the Dictionary (docs/tags.md §6.7). The viewer is the Media tab's.

use dioxus::history::History;
use dioxus::prelude::*;
use uuid::Uuid;

use crate::components::media_gallery::MediaRouteViewer;
use crate::components::tree_page::{ToolPageFrame, use_tree_page};
use crate::i18n::use_i18n;
use crate::nav_history::use_app_history;
use crate::router::Route;
use crate::ui_observability::{UiPage, use_ui_load_trace};

#[component]
pub fn Media(tree_id: String, media_id: String) -> Element {
    use_ui_load_trace(UiPage::Media);
    let i18n = use_i18n();
    let page = use_tree_page(&tree_id);
    let history = use_app_history();
    let nav = dioxus::router::navigator();
    let parsed = tree_id
        .parse::<Uuid>()
        .ok()
        .zip(media_id.parse::<Uuid>().ok());

    // Closing goes back to where the reader came from; a page opened from an
    // address alone has nothing to go back to, so it lands on the Dictionary.
    let back_tree = tree_id.clone();
    let on_close = move |()| match history.as_ref() {
        Some(history) if history.can_go_back() => history.go_back(),
        _ => {
            nav.push(Route::Dictionary {
                tree_id: back_tree.clone(),
            });
        }
    };

    rsx! {
        ToolPageFrame {
            tree_id: tree_id.clone(),
            tree_name: page.name(),
            title: i18n.t("dictionary.tab.media"),
            crumbs: rsx! {
                Link {
                    to: Route::Dictionary { tree_id: tree_id.clone() },
                    class: "td-bc-link",
                    {i18n.t("dictionary.breadcrumb")}
                }
                span { class: "td-bc-sep", "/" }
            },
            selected_person_id: None,
            match parsed {
                Some((tree, media)) => rsx! {
                    MediaRouteViewer { tree_id: tree, media_id: media, on_close }
                },
                None => rsx! { div { class: "error-msg", {i18n.t("dictionary.error")} } },
            }
        }
    }
}
